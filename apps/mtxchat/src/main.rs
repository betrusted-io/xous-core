#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use chat::{Chat, Event, POST_TEXT_MAX};
use gam::{MenuItem, MenuPayload};
use locales::t;
use mtxchat::MtxChat;
use num_traits::*;
use xous_ipc::Buffer;

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
    log::trace!("registered with NS -- {:?}", sid);

    let chat = Chat::new(
        gam::APP_NAME_MTXCHAT,
        gam::APP_MENU_0_MTXCHAT,
        Some(xous::connect(sid).unwrap()),
        Some(MtxchatOp::Post as usize),
        Some(MtxchatOp::Event as usize),
        Some(MtxchatOp::Rawkeys as usize),
    );

    let cid = xous::connect(sid).unwrap();
    chat.menu_add(MenuItem {
        name: xous_ipc::String::from_str(t!("mtxchat.room.item", locales::LANG)),
        action_conn: Some(cid),
        action_opcode: MtxchatOp::Menu as u32,
        action_payload: MenuPayload::Scalar([MenuOp::Room as u32, 0, 0, 0]),
        close_on_select: true,
    })
    .expect("failed add menu");
    chat.menu_add(MenuItem {
        name: xous_ipc::String::from_str(t!("mtxchat.login.item", locales::LANG)),
        action_conn: Some(cid),
        action_opcode: MtxchatOp::Menu as u32,
        action_payload: MenuPayload::Scalar([MenuOp::Login as u32, 0, 0, 0]),
        close_on_select: true,
    })
    .expect("failed add menu");
    chat.menu_add(MenuItem {
        name: xous_ipc::String::from_str(t!("mtxchat.logout.item", locales::LANG)),
        action_conn: Some(cid),
        action_opcode: MtxchatOp::Menu as u32,
        action_payload: MenuPayload::Scalar([MenuOp::Logout as u32, 0, 0, 0]),
        close_on_select: true,
    })
    .expect("failed add menu");
    chat.menu_add(MenuItem {
        name: xous_ipc::String::from_str(t!("mtxchat.close.item", locales::LANG)),
        action_conn: Some(cid),
        action_opcode: MtxchatOp::Menu as u32,
        action_payload: MenuPayload::Scalar([MenuOp::Noop as u32, 0, 0, 0]),
        close_on_select: true,
    })
    .expect("failed add menu");

    let mut mtxchat = MtxChat::new(&chat);
    let mut first_focus = true;
    let mut user_post: Option<String> = None;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MtxchatOp::Event) => {
                log::info!("got Chat UI Event");
                xous::msg_scalar_unpack!(msg, event_code, _, _, _, {
                    match FromPrimitive::from_usize(event_code) {
                        Some(Event::Focus) => {
                            if first_focus {
                                first_focus = false;
                                mtxchat.connect();
                            }
                            mtxchat.redraw();
                        }
                        _ => (),
                    }
                });
            }
            Some(MtxchatOp::Menu) => {
                log::info!("got Chat Menu Click");
                xous::msg_scalar_unpack!(msg, menu_code, _, _, _, {
                    match FromPrimitive::from_usize(menu_code) {
                        Some(MenuOp::Login) => {
                            mtxchat.connect();
                        }
                        Some(MenuOp::Logout) => {
                            mtxchat.logout();
                            mtxchat.connect();
                        }
                        Some(MenuOp::Noop) => {}
                        Some(MenuOp::Room) => {
                            if let Some(room) = mtxchat.get_room_id() {
                                mtxchat.listen_over("");
                                mtxchat.dialogue_set(Some(room.as_str()));
                                mtxchat.listen();
                            }
                        }
                        _ => (),
                    }
                });
            }
            Some(MtxchatOp::Post) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer
                    .to_original::<xous_ipc::String<{ POST_TEXT_MAX }>, _>()
                    .unwrap();
                if s.len() > 0 {
                    // capture input instead of calling here, so message can drop and calling server is released
                    user_post = Some(s.to_string());
                }
            }
            Some(MtxchatOp::Rawkeys) => log::info!("got mtxchat rawkeys"),
            Some(MtxchatOp::Quit) => {
                log::error!("got Quit");
                mtxchat.listen_over("");
                break;
            }
            _ => (),
        }
        if let Some(post) = user_post {
            mtxchat.post(&post);
            user_post = None;
        }
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
