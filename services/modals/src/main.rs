#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod tests;

use xous::SID;

use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;

use std::sync::{Arc, Mutex};
use std::thread;

use gam::modal::*;
#[cfg(feature="tts")]
use tts_frontend::TtsFrontend;
#[cfg(feature="tts")]
use locales::t;
#[cfg(feature="tts")]
const TICK_INTERVAL: u64 = 2500;

use num_traits::*;

#[derive(Debug)]
enum RendererState {
    /// idle state
    None,
    /// running state
    RunRadio(ManagedPromptWithFixedResponse),
    RunCheckBox(ManagedPromptWithFixedResponse),
    RunText(ManagedPromptWithTextResponse),
    RunProgress(ManagedProgress),
    RunNotification(ManagedNotification),
    /// response ready state
    ResponseText(TextEntryPayload),
    ResponseRadio(ItemName),
    ResponseCheckBox(CheckBoxPayload),
    RunDynamicNotification(DynamicNotification),
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum RendererOp {
    InitiateOp,
    UpdateProgress,
    FinishProgress,

    TextEntryReturn,
    RadioReturn,
    CheckBoxReturn,
    NotificationReturn,

    AddModalItem,

    UpdateDynamicNotification,
    CloseDynamicNotification,

    ModalRedraw,
    ModalKeypress,
    ModalDrop,
    Quit,
    Gutter,
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let modals_sid = xns.register_name(api::SERVER_NAME_MODALS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", modals_sid);

    let tt = ticktimer_server::Ticktimer::new().unwrap();

    let renderer_sid = xous::create_server().expect("couldn't create a server for the modal UX renderer");
    let renderer_cid = xous::connect(renderer_sid).expect("couldn't connect to the modal UX renderer");

    #[cfg(feature="tts")]
    let tts = TtsFrontend::new(&xns).unwrap();
    #[cfg(feature="tts")]
    let mut last_tick = tt.elapsed_ms();

    let op = Arc::new(Mutex::new(RendererState::None));
    // create a thread that just handles the redrawing requests
    let _redraw_handle = thread::spawn({
        let op = Arc::clone(&op);
        move || {
            #[cfg(feature="tts")]
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            // build the core data structure here
            let text_action = TextEntry {
                is_password: false,
                visibility: TextEntryVisibility::Visible,
                action_conn: renderer_cid,
                action_opcode: RendererOp::TextEntryReturn.to_u32().unwrap(),
                action_payload: TextEntryPayload::new(),
                validator: None,
            };
            let mut fixed_items = Vec::<ItemName>::new();
            let notification = gam::modal::Notification::new(
                renderer_cid,
                RendererOp::NotificationReturn.to_u32().unwrap()
            );
            let mut gutter = gam::modal::Notification::new(
                renderer_cid,
                RendererOp::Gutter.to_u32().unwrap()
            );
            gutter.set_manual_dismiss(false);
            let mut progress_action = Slider::new(renderer_cid, RendererOp::Gutter.to_u32().unwrap(),
                0, 100, 1, Some("%"), 0, true, true
            );
            let mut last_percentage = 0;
            let mut start_work: u32 = 0;
            let mut end_work: u32 = 100;
            let mut renderer_modal =
                Modal::new(
                    gam::SHARED_MODAL_NAME,
                    ActionType::TextEntry(text_action),
                    Some("Placeholder"),
                    None,
                    GlyphStyle::Regular,
                    8
                );
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
                        log::debug!("InitiateOp called");
                        let mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunText(config) => {
                                log::debug!("initiating text entry modal");
                                #[cfg(feature="tts")]
                                tts.tts_simple(config.prompt.as_str().unwrap()).unwrap();
                                renderer_modal.modify(
                                    Some(ActionType::TextEntry(text_action)),
                                    Some(config.prompt.as_str().unwrap()), false,
                                    None, true, None
                                );
                                renderer_modal.activate();
                                log::debug!("should be active!");
                            },
                            RendererState::RunNotification(config) => {
                                #[cfg(feature="tts")]
                                tts.tts_simple(config.message.as_str().unwrap()).unwrap();
                                renderer_modal.modify(
                                    Some(ActionType::Notification(notification)),
                                    Some(config.message.as_str().unwrap()), false,
                                    None, true, None
                                );
                                renderer_modal.activate();
                            },
                            RendererState::RunProgress(config) => {
                                start_work = config.start_work;
                                end_work = config.end_work;
                                last_percentage = compute_checked_percentage(
                                    config.current_work, start_work, end_work);
                                log::debug!("init percentage: {}, current: {}, start: {}, end: {}", last_percentage, config.current_work, start_work, end_work);
                                progress_action.set_state(last_percentage);
                                #[cfg(feature="tts")]
                                tts.tts_simple(config.title.as_str().unwrap()).unwrap();
                                renderer_modal.modify(
                                    Some(ActionType::Slider(progress_action)),
                                    Some(config.title.as_str().unwrap()), false,
                                    None, true, None
                                );
                                renderer_modal.activate();
                            },
                            RendererState::RunRadio(config) => {
                                let mut radiobuttons = gam::modal::RadioButtons::new(
                                    renderer_cid,
                                    RendererOp::RadioReturn.to_u32().unwrap()
                                );
                                for item in fixed_items.iter() {
                                    radiobuttons.add_item(*item);
                                }
                                fixed_items.clear();
                                #[cfg(feature="tts")]
                                {
                                    tts.tts_blocking(t!("modals.radiobutton", xous::LANG)).unwrap();
                                    tts.tts_blocking(config.prompt.as_str().unwrap()).unwrap();
                                }
                                renderer_modal.modify(
                                    Some(ActionType::RadioButtons(radiobuttons)),
                                    Some(config.prompt.as_str().unwrap()), false,
                                    None, true, None
                                );
                                renderer_modal.activate();
                            },
                            RendererState::RunCheckBox(config) => {
                                let mut checkbox = gam::modal::CheckBoxes::new(
                                    renderer_cid,
                                    RendererOp::CheckBoxReturn.to_u32().unwrap()
                                );
                                for item in fixed_items.iter() {
                                    checkbox.add_item(*item);
                                }
                                fixed_items.clear();
                                #[cfg(feature="tts")]
                                {
                                    tts.tts_blocking(t!("modals.checkbox", xous::LANG)).unwrap();
                                    tts.tts_blocking(config.prompt.as_str().unwrap()).unwrap();
                                }
                                renderer_modal.modify(
                                    Some(ActionType::CheckBoxes(checkbox)),
                                    Some(config.prompt.as_str().unwrap()), false,
                                    None, true, None
                                );
                                renderer_modal.activate();
                            },
                            RendererState::RunDynamicNotification(config) => {
                                let mut top_text = String::new();
                                if let Some(title) = config.title {
                                    #[cfg(feature="tts")]
                                    tts.tts_simple(title.as_str().unwrap()).unwrap();
                                    top_text.push_str(title.as_str().unwrap());
                                }
                                let mut bot_text = String::new();
                                if let Some(text) = config.text {
                                    #[cfg(feature="tts")]
                                    tts.tts_simple(text.as_str().unwrap()).unwrap();
                                    bot_text.push_str(text.as_str().unwrap());
                                }
                                renderer_modal.modify(
                                    Some(ActionType::Notification(gutter)),
                                    Some(&top_text), config.title.is_none(),
                                    Some(&bot_text), config.text.is_none(),
                                    None
                                );
                                renderer_modal.activate();
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
                    Some(RendererOp::UpdateProgress) => msg_scalar_unpack!(msg, current, _, _, _, {
                        let new_percentage = compute_checked_percentage(
                            current as u32, start_work, end_work);
                        log::trace!("percentage: {}, current: {}, start: {}, end: {}", new_percentage, current, start_work, end_work);
                        if new_percentage != last_percentage {
                            last_percentage = new_percentage;
                            progress_action.set_state(last_percentage);
                            #[cfg(feature="tts")]
                            {
                                if tt.elapsed_ms() - last_tick > TICK_INTERVAL {
                                    tts.tts_blocking(t!("progress.increment", xous::LANG)).unwrap();
                                    last_tick = tt.elapsed_ms();
                                }
                            }
                            renderer_modal.modify(
                                Some(ActionType::Slider(progress_action)),
                                None, false,
                                None, false, None
                            );
                            renderer_modal.redraw();
                            xous::yield_slice(); // give time for the GAM to redraw
                        }
                    }),
                    Some(RendererOp::FinishProgress) => {
                        renderer_modal.gam.relinquish_focus().unwrap();
                    },
                    Some(RendererOp::UpdateDynamicNotification) => {
                        let mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunDynamicNotification(config) => {
                                let mut top_text = String::new();
                                if let Some(title) = config.title {
                                    top_text.push_str(title.as_str().unwrap());
                                }
                                let mut bot_text = String::new();
                                if let Some(text) = config.text {
                                    bot_text.push_str(text.as_str().unwrap());
                                }
                                renderer_modal.modify(
                                    None,
                                    Some(&top_text), config.title.is_none(),
                                    Some(&bot_text), config.text.is_none(),
                                    None
                                );
                                renderer_modal.redraw();
                            }
                            _ => {
                                log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                                panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                            }
                        }
                    },
                    Some(RendererOp::CloseDynamicNotification) => {
                        renderer_modal.gam.relinquish_focus().unwrap();
                    },
                    Some(RendererOp::TextEntryReturn) => {
                        let mut mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunText(config) => {
                                log::trace!("validating text entry modal");
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
                            RendererState::None => log::warn!("Text entry detected a fat finger event, ignoring."),
                            _ => {
                                log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                                panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                            }
                        }
                    }
                    Some(RendererOp::NotificationReturn) => {
                        let mut mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunNotification(_) => *mutex_op = RendererState::None,
                            RendererState::None => log::warn!("Notification detected a fat finger event, ignoring."),
                            _ => {
                                log::error!("UX return opcode does not match our current operation in flight: {:?}", mutex_op);
                                panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                            }
                        }
                    },
                    Some(RendererOp::Gutter) => {
                        log::info!("gutter op, doing nothing");
                    },
                    Some(RendererOp::AddModalItem) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let item = buffer.to_original::<ItemName, _>().unwrap();
                        fixed_items.push(item);
                    }
                    Some(RendererOp::RadioReturn) => {
                        let mut mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunRadio(_config) => {
                                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                                let item = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                                *mutex_op = RendererState::ResponseRadio(item.0);
                            }
                            RendererState::ResponseRadio(_) => log::warn!("Radio buttons detected a fat finger event, ignoring."),
                            RendererState::None => log::warn!("Radio buttons detected a fat finger event, ignoring."),
                            _ => {
                                log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                                panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                            }
                        }
                    }
                    Some(RendererOp::CheckBoxReturn) => {
                        let mut mutex_op = op.lock().unwrap();
                        match *mutex_op {
                            RendererState::RunCheckBox(_config) => {
                                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                                let item = buffer.to_original::<CheckBoxPayload, _>().unwrap();
                                *mutex_op = RendererState::ResponseCheckBox(item);
                            }
                            RendererState::ResponseCheckBox(_) => log::warn!("Check boxes detected a fat finger event, ignoring."),
                            RendererState::None => log::warn!("Check boxes detected a fat finger event, ignoring."),
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
                }
            }
            xous::destroy_server(renderer_sid).unwrap();
        }
    });

    if cfg!(feature = "ux_tests") {
        tt.sleep_ms(1000).unwrap();
        tests::spawn_test();
    }

    let mut token_lock: Option<[u32; 4]> = None;
    let trng = trng::Trng::new(&xns).unwrap();
    // this is a random number that serves as a "default" that cannot be guessed
    let default_nonce = [
        trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(),
    ];
    loop {
        let mut msg = xous::receive_message(modals_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::GetMutex) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                let incoming_token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if let Some(token) = token_lock {
                    if token == incoming_token {
                        xous::return_scalar(msg.sender, 1).unwrap();
                    } else {
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }
                } else {
                    token_lock = Some(incoming_token);
                    xous::return_scalar(msg.sender, 1).unwrap();
                }
            }),
            Some(Opcode::PromptWithFixedResponse) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let spec = buffer.to_original::<ManagedPromptWithFixedResponse, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    buffer.replace(ItemName::new("internal error")).unwrap();
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunRadio(spec);
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
                loop {
                    match *op.lock().unwrap() {
                        RendererState::RunRadio(_) => (),
                        RendererState::ResponseRadio(item) => {
                            buffer.replace(item).unwrap();
                            token_lock = None;
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
            Some(Opcode::PromptWithMultiResponse) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let spec = buffer.to_original::<ManagedPromptWithFixedResponse, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    buffer.replace(CheckBoxPayload::new()).unwrap();
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunCheckBox(spec);
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
                loop {
                    match *op.lock().unwrap() {
                        RendererState::RunCheckBox(_) => (),
                        RendererState::ResponseCheckBox(items) => {
                            buffer.replace(items).unwrap();
                            token_lock = None;
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
            Some(Opcode::PromptWithTextResponse) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let spec = buffer.to_original::<ManagedPromptWithTextResponse, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    buffer.replace(TextEntryPayload::new()).unwrap();
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunText(spec);
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
                loop {
                    match *op.lock().unwrap() {
                        RendererState::RunText(_) => (),
                        RendererState::ResponseText(text) => {
                            buffer.replace(text).unwrap();
                            token_lock = None;
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
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<ManagedNotification, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunNotification(spec);
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
                loop {
                    match *op.lock().unwrap() {
                        RendererState::RunNotification(_) => (),
                        RendererState::None => {token_lock = None; break},
                        _ => {
                            log::error!("Illegal state transition in renderer");
                            panic!("Illegal state transition in renderer");
                        }
                    }
                    tt.sleep_ms(100).unwrap(); // don't put the idle in the match/lock(), it'll prevent the other thread from running!
                }
            },
            Some(Opcode::StartProgress) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<ManagedProgress, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunProgress(spec);
                send_message(
                    renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
            },
            Some(Opcode::UpdateProgress) => msg_scalar_unpack!(msg, current, _, _, _, {
                send_message(
                    renderer_cid,
                    Message::new_scalar(RendererOp::UpdateProgress.to_usize().unwrap(), current, 0, 0, 0)
                ).expect("couldn't update progress bar");
            }),
            Some(Opcode::StopProgress) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                send_message(
                    renderer_cid,
                    Message::new_scalar(RendererOp::FinishProgress.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't update progress bar");
                token_lock = None;
            }),
            Some(Opcode::AddModalItem) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let manageditem = buffer.to_original::<ManagedListItem, _>().unwrap();
                if manageditem.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                let fwd_buf = Buffer::into_buf(manageditem.item).unwrap();
                fwd_buf.lend(renderer_cid, RendererOp::AddModalItem.to_u32().unwrap()).expect("couldn't add item");
            }
            Some(Opcode::DynamicNotification) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<DynamicNotification, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunDynamicNotification(spec);
                send_message(
                renderer_cid,
                    Message::new_scalar(RendererOp::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
            },
            Some(Opcode::UpdateDynamicNotification) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<DynamicNotification, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                *op.lock().unwrap() = RendererState::RunDynamicNotification(spec);
                send_message(
                    renderer_cid,
                        Message::new_scalar(RendererOp::UpdateDynamicNotification.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate UX op");
            },
            Some(Opcode::CloseDynamicNotification) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                send_message(
                    renderer_cid,
                    Message::new_scalar(RendererOp::CloseDynamicNotification.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't close dynamic notification");
                token_lock = None;
            }),
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

fn compute_checked_percentage(current: u32, start: u32, end: u32) -> u32 {
    if end <= start {
        100
    } else {
        if current < start {
            0
        } else if current >= end {
            100
        } else {
            // do math in higher precision because we could overflow a u32
            (((current as u64 - start as u64) * 100) / (end as u64 - start as u64)) as u32
        }
    }
}