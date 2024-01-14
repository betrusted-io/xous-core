#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod mtxcli;
use mtxcli::*;
mod cmds;
use cmds::*;
use num_traits::*;
use xous_ipc::Buffer;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum MtxcliOp {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// redraw our UI
    Redraw,
    /// change focus
    ChangeFocus,
    /// exit the application
    Quit,
}

// This name should be (1) unique (2) under 64 characters long and (3) ideally descriptive.
pub(crate) const SERVER_NAME_MTXCLI: &str = "_Matrix cli_";

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
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let sid = xns.register_name(SERVER_NAME_MTXCLI, None).expect("can't register server");
    // log::trace!("registered with NS -- {:?}", sid);

    let mut mtxcli = Mtxcli::new(&xns, sid);
    let mut update_mtxcli = true;
    let mut was_callback = false;
    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MtxcliOp::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::trace!("mtxcli got input line: {}", s.as_str());
                mtxcli.input(s.as_str()).expect("MTXCLI couldn't accept input string");
                update_mtxcli = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(MtxcliOp::Redraw) => {
                if allow_redraw {
                    mtxcli.redraw().expect("MTXCLI couldn't redraw");
                }
            }
            Some(MtxcliOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                    }
                }
            }),
            Some(MtxcliOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message, treating as callback");
                mtxcli.msg(msg);
                update_mtxcli = true;
                was_callback = true;
            }
        }
        if update_mtxcli {
            mtxcli.update(was_callback).expect("MTXCLI had problems updating");
            update_mtxcli = false;
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
