#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod repl;
use repl::*;
mod cmds;
use cmds::*;
use num_traits::*;
use xous_ipc::Buffer;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ReplOp {
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
pub(crate) const SERVER_NAME_REPL: &str = "_REPL demo application_";

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let sid = xns.register_name(SERVER_NAME_REPL, None).expect("can't register server");
    // log::trace!("registered with NS -- {:?}", sid);

    let mut repl = Repl::new(&xns, sid);
    let mut update_repl = true;
    let mut was_callback = false;
    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ReplOp::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String, _>().unwrap();
                log::trace!("repl got input line: {}", s.as_str());
                repl.input(s.as_str()).expect("REPL couldn't accept input string");
                update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(ReplOp::Redraw) => {
                if allow_redraw {
                    repl.redraw().expect("REPL couldn't redraw");
                }
            }
            Some(ReplOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
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
            Some(ReplOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message, treating as callback");
                repl.msg(msg);
                update_repl = true;
                was_callback = true;
            }
        }
        if update_repl {
            repl.update(was_callback).expect("REPL had problems updating");
            update_repl = false;
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
