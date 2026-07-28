#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod icontray;
mod mailapp;

use api::*;
use mailapp::MailApp;
use num_traits::*;

fn main() -> ! {
    // The IMAP path buffers whole messages (headers + body + any inline
    // parts) in RAM while parsing, so give the app a generous stack.
    let stack_size = 1024 * 1024;
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}

fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // Raise the heap ceiling: a full FETCH of a large message (with
    // base64/quoted-printable parts) can transiently allocate well past the
    // default limit.
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
    let sid = xns.register_name(SERVER_NAME_MAIL, None).expect("can't register server");

    // MailApp owns the GAM shell (registers our own UxRegistration, spawns
    // our icontray for the F-key legend, and holds the content canvas), so
    // this loop is just event dispatch. No chat library involved.
    let mut app = MailApp::new(&xns, sid);
    let mut allow_redraw = false;

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MailOp::Redraw) => {
                if allow_redraw {
                    app.redraw();
                }
            }
            Some(MailOp::Rawkeys) => {
                // GAM delivers up to four key chars per event; a function key
                // is a single press, so we only need the first slot. Each
                // handler runs a blocking modal flow; when it returns we
                // repaint the home screen (the flow's transient status /
                // modal has been dismissed by then).
                xous::msg_scalar_unpack!(msg, k1, _, _, _, {
                    let handled = match core::char::from_u32(k1 as u32).unwrap_or('\u{0000}') {
                        F1 => {
                            app.inbox();
                            true
                        }
                        F2 => {
                            app.compose();
                            true
                        }
                        F3 => {
                            app.settings();
                            true
                        }
                        F4 => {
                            app.reply();
                            true
                        }
                        _ => false,
                    };
                    if handled && allow_redraw {
                        app.redraw();
                    }
                });
            }
            Some(MailOp::ChangeFocus) => {
                xous::msg_scalar_unpack!(msg, new_state, _, _, _, {
                    match gam::FocusState::convert_focus_change(new_state) {
                        gam::FocusState::Background => allow_redraw = false,
                        gam::FocusState::Foreground => {
                            allow_redraw = true;
                            app.redraw();
                        }
                    }
                });
            }
            Some(MailOp::Line) => {
                // We take no free-text input on the home screen; drain any
                // committed line the IME might send so its caller is released.
                if let Some(mem) = msg.body.memory_message() {
                    let _ = unsafe { xous_ipc::Buffer::from_memory_message(mem) };
                }
            }
            Some(MailOp::Quit) => {
                log::info!("got Quit");
                break;
            }
            _ => log::warn!("got unknown message"),
        }
    }
    log::info!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    xous::terminate_process(0)
}
