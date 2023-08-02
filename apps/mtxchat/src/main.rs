#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use chat::{Chat, ChatOp, Event};
use locales::t;
use modals::Modals;
use mtxchat::MtxChat;
use num_traits::*;

fn main() -> ! {
    let stack_size = 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(wrapped_main)
        .unwrap()
        .join()
        .unwrap()
}

fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    const HEAP_LARGER_LIMIT: usize = 2048 * 1024;
    let new_limit = HEAP_LARGER_LIMIT;
    let result = xous::rsyscall(xous::SysCall::AdjustProcessLimit(
        xous::Limits::HeapMaximum as usize,
        0,
        new_limit,
    ));

    if let Ok(xous::Result::Scalar2(1, current_limit)) = result {
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(
            xous::Limits::HeapMaximum as usize,
            current_limit,
            new_limit,
        ))
        .unwrap();
        log::info!("Heap limit increased to: {}", new_limit);
    } else {
        panic!("Unsupported syscall!");
    }

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns
        .register_name(SERVER_NAME_MTXCHAT, None)
        .expect("can't register server");
    // log::trace!("registered with NS -- {:?}", sid);

    let chat = Chat::new(
        gam::APP_NAME_MTXCHAT,
        Some(xous::connect(sid).unwrap()),
        Some(MtxchatOp::Post as usize),
        Some(MtxchatOp::Event as usize),
        Some(MtxchatOp::Rawkeys as usize),
    );

    let mut mtxchat = MtxChat::new(&chat);

    let modals = Modals::new(&xns).expect("can't connect to Modals server");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MtxchatOp::Event) => {
                log::info!("got Chat UI Event");
                xous::msg_scalar_unpack!(msg, event_code, _, _, _, {
                    match FromPrimitive::from_usize(event_code) {
                        Some(Event::Focus) => {
                            while !mtxchat.login() {
                                modals
                                    .show_notification(t!("mtxchat.login.failed", locales::LANG), None)
                                    .expect("notification failed");
                                mtxchat.login_modal();
                            }
                            while !mtxchat.get_room_id() {
                                modals
                                    .show_notification(t!("mtxchat.roomid.failed", locales::LANG), None)
                                    .expect("notification failed");
                                mtxchat.room_modal();
                            }
                            mtxchat.listen();
                        }
                        _ => (),
                    }
                })
            }
            Some(MtxchatOp::Post) => {
                log::info!("TODO Post to Matrix server");
            }
            Some(MtxchatOp::Rawkeys) => {}
            Some(MtxchatOp::Quit) => {
                log::error!("got Quit");
                chat.forward(msg);
                break;
            }
            _ => (),
        }
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
