use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};

use xous_ipc::Buffer;

pub fn start_shell() {
    std::thread::spawn(move || {
        shell();
    });
}

////////////////// local message passing from Ux Callback
use num_traits::*;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum ShellOpcode {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// redraw our UI
    Redraw,
    /// change focus
    ChangeFocus,
    /// exit the application
    Quit,
}

pub(crate) const SERVER_NAME_SHELLCHAT: &str = "_Shell chat application_"; // used internally by xous-names

fn shell() {
    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let shch_sid = xns.register_name(SERVER_NAME_SHELLCHAT, None).expect("can't register server");

    let mut repl = crate::repl::Repl::new(&xns, shch_sid);
    let mut update_repl = false;
    let mut was_callback = false;

    let mut allow_redraw = true;
    let pddb_init_done = Arc::new(AtomicBool::new(false));
    repl.redraw(pddb_init_done.load(Ordering::SeqCst)).ok();

    // until PDDB is ready
    #[cfg(feature = "pddb")]
    thread::spawn({
        let pddb_init_done = pddb_init_done.clone();
        let main_conn = xous::connect(shch_sid).unwrap();
        move || {
            let pddb = pddb::Pddb::new();
            pddb.mount_attempted_blocking();
            pddb_init_done.store(true, Ordering::SeqCst);
            xous::send_message(
                main_conn,
                xous::Message::new_scalar(ShellOpcode::Redraw.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .ok();
        }
    });

    loop {
        let msg = xous::receive_message(shch_sid).unwrap();
        let shell_op: Option<ShellOpcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("Shellchat got message {:?}", msg);
        match shell_op {
            Some(ShellOpcode::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::trace!("shell got input line: {}", s.as_str());
                #[cfg(feature = "tts")]
                {
                    let mut input = t!("shellchat.input-tts", locales::LANG).to_string();
                    input.push_str(s.as_str());
                    tts.tts_simple(&input).unwrap();
                }
                repl.input(s.as_str()).expect("REPL couldn't accept input string");
                update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(ShellOpcode::Redraw) => {
                if allow_redraw {
                    repl.redraw(pddb_init_done.load(Ordering::SeqCst)).expect("REPL couldn't redraw");
                }
            }
            Some(ShellOpcode::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
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
            Some(ShellOpcode::Quit) => {
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
            repl.update(was_callback, pddb_init_done.load(Ordering::SeqCst))
                .expect("REPL had problems updating");
            update_repl = false;
        }
    }

    // clean up our program
    log::error!("shell loop exit, destroying servers");
    xns.unregister_server(shch_sid).unwrap();
    xous::destroy_server(shch_sid).unwrap();
}
