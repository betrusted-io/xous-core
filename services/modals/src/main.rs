#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod tests;
use tests::*;

use xous::{SID, CID};

use xous::{msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;

use std::sync::{Arc, Mutex};
use std::thread;
use core::sync::atomic::{AtomicBool, Ordering};

use gam::modal::*;
use locales::t;

use num_traits::*;

enum RendererState {
    /// idle state
    None,
    /// running state
    RunFixed(ManagedPromptWithFixedResponse),
    RunText(ManagedPromptWithTextResponse),
    RunProgress(ManagedProgress),
    RunNotification(ManagedNotification),
    /// response ready state
    ResponseText(TextEntryPayload),
    ResponseFixed([Option<ItemName>; MAX_ITEMS]),
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum RendererOp {
    InitiateOp,

    TextEntryReturn,
    RadioReturn,
    NotificationReturn,

    ModalRedraw,
    ModalKeypress,
    ModalDrop,
    Quit,
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let modals_sid = xns.register_name(api::SERVER_NAME_MODALS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", modals_sid);

    let tt = ticktimer_server::Ticktimer::new().unwrap();

    let renderer_sid = xous::create_server().expect("couldn't create a server for the modal UX renderer");
    let renderer_cid = xous::connect(renderer_sid).expect("couldn't connect to the modal UX renderer");

    let op = Arc::new(Mutex::new(RendererState::None));
    // create a thread that just handles the redrawing requests
    let redraw_handle = thread::spawn({
        let op = Arc::clone(&op);
        move || {
            // build the core data structure here
            let mut text_action = TextEntry {
                is_password: false,
                visibility: TextEntryVisibility::Visible,
                action_conn: renderer_cid,
                action_opcode: RendererOp::TextEntryReturn.to_u32().unwrap(),
                action_payload: TextEntryPayload::new(),
                validator: None,
            };
            let mut radiobox = gam::modal::RadioButtons::new(
                renderer_cid,
                RendererOp::RadioReturn.to_u32().unwrap()
            );
            let mut notification = gam::modal::Notification::new(
                renderer_cid,
                RendererOp::NotificationReturn.to_u32().unwrap()
            );
            let mut progress_action = Slider::new(renderer_cid, RendererOp::NotificationReturn.to_u32().unwrap(),
                0, 100, 10, Some("%"), 0, true, true
            );
            let mut renderer_modal =
                Modal::new(
                    crate::api::SHARED_MODAL_NAME,
                    ActionType::TextEntry(text_action),
                    Some("Placeholder"),
                    None,
                    GlyphStyle::Small,
                    8
                );
            let mut progressbar = ProgressBar::new(&mut renderer_modal, &mut progress_action);
            renderer_modal.spawn_helper(renderer_sid, renderer_modal.sid,
                RendererOp::ModalRedraw.to_u32().unwrap(),
                RendererOp::ModalKeypress.to_u32().unwrap(),
                RendererOp::ModalDrop.to_u32().unwrap(),
            );

            loop {
                let msg = xous::receive_message(renderer_sid).unwrap();
                log::debug!("renderer message: {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(RendererOp::InitiateOp) => {
                        log::info!("InitiateOp called");
                        let mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunText(config) => {
                                log::info!("initiating text entry modal");
                                renderer_modal.modify(
                                    Some(ActionType::TextEntry(text_action)),
                                    Some(config.prompt.as_str().unwrap()), false,
                                    None, false, None
                                );
                                renderer_modal.activate();
                                log::info!("should be active!");
                            },
                            RendererState::None => {
                                log::error!("Operation initiated with no argument specified. Ignoring request.");
                                continue;
                            }
                            _ => {
                                log::warn!("unimplemented arm in renderer match");
                                unimplemented!();
                            }
                        }
                    },
                    Some(RendererOp::TextEntryReturn) => {
                        let mut mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunText(config) => {
                                log::info!("validating text entry modale");
                                let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                                let text = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();
                                if let Some(validator_sid) = config.validator {
                                    let cid = xous::connect(SID::from_array(validator_sid)).unwrap();
                                    let validation = Validation {
                                        text,
                                        opcode: config.validator_op,
                                    };
                                    let mut buf = Buffer::into_buf(validation).expect("couldn't convert validator structure");
                                    buf.lend_mut(cid, ValidationOp::Validate.to_u32().unwrap()).expect("validation call failed");
                                    let response = buf.to_original::<Option<xous_ipc::String::<256>>, _>().expect("couldn't unpack validation response");
                                    unsafe{xous::disconnect(cid).unwrap();}
                                    if let Some(err) = response {
                                        // try again
                                        renderer_modal.modify(
                                            Some(ActionType::TextEntry(text_action)),
                                            Some(config.prompt.as_str().unwrap()), false,
                                            Some(err.as_str().unwrap()), false, None
                                        );
                                        renderer_modal.activate();
                                    } else {
                                        // the change in mutex_op enum type will signal the state change to the caller
                                        *mutex_op = RendererState::ResponseText(text);
                                    }
                                } else {
                                    *mutex_op = RendererState::ResponseText(text);
                                }
                            }
                            _ => {
                                log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                                panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                            }
                        }
                    }
                    Some(RendererOp::ModalRedraw) => {
                        renderer_modal.redraw();
                    },
                    Some(RendererOp::ModalKeypress) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                        let keys = [
                            if let Some(a) = core::char::from_u32(k1 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k2 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k3 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k4 as u32) {a} else {'\u{0000}'},
                        ];
                        renderer_modal.key_event(keys);
                    }),
                    Some(RendererOp::ModalDrop) => { // this guy should never quit, it's a core OS service
                        panic!("Password modal for PDDB quit unexpectedly");
                    },
                    Some(RendererOp::Quit) => {
                        log::warn!("received quit on PDDB password UX renderer loop");
                        xous::return_scalar(msg.sender, 0).unwrap();
                        break;
                    },
                    None => {
                        log::error!("Couldn't convert opcode: {:?}", msg);
                    }
                    _ => {
                        unimplemented!();
                    }
                }
            }
            xous::destroy_server(renderer_sid).unwrap();
        }
    });

    loop {
        let mut msg = xous::receive_message(modals_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::PromptWithFixedResponse) => {

            },
            Some(Opcode::PromptWithMultiResponse) => {

            },
            Some(Opcode::PromptWithTextResponse) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let spec = buffer.to_original::<ManagedPromptWithTextResponse, _>().unwrap();
                {
                    *op.lock().unwrap() = RendererState::RunText(spec);
                }
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
                loop {
                    match *op.lock().unwrap() {
                        RendererState::RunText(_) => (),
                        RendererState::ResponseText(text) => {
                            buffer.replace(text).unwrap();
                            break;
                        },
                        _ => {
                            log::error!("Illegal state transition in renderer");
                            panic!("Illegal state transition in renderer");
                        }
                    }
                    tt.sleep_ms(100).unwrap(); // don't put the idle in the match/lock(), it'll prevent the other thread from running!
                }
            },
            Some(Opcode::Notification) => {

            },
            Some(Opcode::StartProgress) => {

            },
            Some(Opcode::UpdateProgress) => {

            },
            Some(Opcode::StopProgress) => {

            },
            Some(Opcode::Quit) => {
                log::warn!("Shared modal UX handler exiting.");
                break
            }
            None => {
                log::error!("couldn't convert opcode");
            }

        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    send_message(renderer_cid, Message::new_blocking_scalar(RendererOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(renderer_cid).unwrap()};
    xous::destroy_server(renderer_sid).unwrap();
    xns.unregister_server(modals_sid).unwrap();
    xous::destroy_server(modals_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
