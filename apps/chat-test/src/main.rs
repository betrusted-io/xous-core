#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod tests;

use api::*;
use chat::{Chat, Event, POST_TEXT_MAX};
use num_traits::*;
use xous_ipc::Buffer;

fn main() -> ! {
    let stack_size = 1024 * 1024;
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}

fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    const HEAP_LARGER_LIMIT: usize = 2048 * 1024;
    let new_limit = HEAP_LARGER_LIMIT;
    let result =
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(xous::Limits::HeapMaximum as usize, 0, new_limit));

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
    let sid = xns.register_name(SERVER_NAME_MTXCHAT, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", sid);

    let chat = Chat::new(
        gam::APP_NAME_CHAT_TEST,
        gam::APP_MENU_0_CHAT_TEST,
        Some(xous::connect(sid).unwrap()),
        Some(ChatTestOp::Post as usize),
        Some(ChatTestOp::Event as usize),
        Some(ChatTestOp::Rawkeys as usize),
    );
    // not used yet, but probably will use in the future
    let _cid = xous::connect(sid).unwrap();

    tests::test_ui(&chat);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ChatTestOp::Event) => {
                log::info!("got Chat UI Event");
                xous::msg_scalar_unpack!(msg, event_code, _, _, _, {
                    match FromPrimitive::from_usize(event_code) {
                        Some(Event::Focus) => {
                            chat.redraw();
                        }
                        _ => (),
                    }
                });
            }
            Some(ChatTestOp::Menu) => {
                log::info!("got Chat Menu Click");
                xous::msg_scalar_unpack!(msg, menu_code, _, _, _, {
                    let code: Option<MenuOp> = FromPrimitive::from_usize(menu_code);
                    log::info!("Got menu code {:?}", code);
                });
            }
            Some(ChatTestOp::Post) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.to_original::<String<{ POST_TEXT_MAX }>, _>().unwrap();
                if s.len() > 0 {
                    // capture input instead of calling here, so message can drop and calling server is
                    // released
                    log::info!("got Post {:?}", s.to_string());
                }
            }
            Some(ChatTestOp::Rawkeys) => log::info!("got rawkeys"),
            Some(ChatTestOp::Quit) => {
                log::error!("got Quit");
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
