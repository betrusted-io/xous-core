#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use api::*;
mod kbd;
use kbd::*;
use num_traits::*;
use xous::{CID, msg_scalar_unpack, Message, send_message};

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    use utralib::generated::*;
    use crate::*;


    pub struct UsbTest {
        pub(crate) conn: CID,
    }

    impl UsbTest {
        pub fn new(sid: xous::SID) -> UsbTest {
            let mut usbtest = UsbTest {
                conn: xous::connect(sid).unwrap(),
            };
            usbtest
        }

        pub fn suspend(&mut self) {
            // self.susres_manager.suspend();
        }
        pub fn resume(&mut self) {
            // self.susres_manager.resume();
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    pub struct UsbTest {
    }

    impl UsbTest {
        pub fn new() -> UsbTest {
            UsbTest {
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
    }
}


#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::UsbTest;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let usbtest_sid = xns.register_name(api::SERVER_NAME_USBTEST, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", usbtest_sid);

    let mut usbtest = UsbTest::new(usbtest_sid);
    let mut kbd = Keyboard::new(usbtest_sid);

    log::trace!("ready to accept requests");

    std::thread::spawn({
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let mut keepalive = 0;
            loop {
                tt.sleep_ms(2500).unwrap();
                log::info!("keepalive {}", keepalive);
                keepalive += 1;
            }
        }
    });

    // register a suspend/resume listener
    let cid = xous::connect(usbtest_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        None,
        &xns,
        api::Opcode::SuspendResume as u32,
        cid
    ).expect("couldn't create suspend/resume object");

    let mut cmdline = String::new();
    loop {
        let msg = xous::receive_message(usbtest_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                kbd.suspend();
                usbtest.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                kbd.resume();
                usbtest.resume();
            }),
            Some(Opcode::DoCmd) => {
                log::info!("got command line: {}", cmdline);
                if let Some((cmd, args)) = cmdline.split_once(' ') {
                    // command and args
                    match cmd {
                        "test" => {
                            log::info!("got test command with arg {}", args);
                        }
                        _ => {
                            log::info!("unrecognied command {}", cmd);
                        }
                    }
                } else {
                    // just the command
                    match cmdline.as_str() {
                        "help" => {
                            log::info!("wouldn't that be nice...");
                        }
                        _ => {
                            log::info!("unrecognized command");
                        }
                    }
                }
                cmdline.clear();
            }
            // this is via UART
            Some(Opcode::KeyboardChar) => msg_scalar_unpack!(msg, k, _, _, _, {
                let key = {
                    let bs_del_fix = if k == 0x7f {
                        0x08
                    } else {
                        k
                    };
                    core::char::from_u32(bs_del_fix as u32).unwrap_or('\u{0000}')
                };
                if key != '\u{0000}' {
                    if key != '\u{000d}' {
                        cmdline.push(key);
                    } else {
                        send_message(cid, Message::new_scalar(
                            Opcode::DoCmd.to_usize().unwrap(), 0, 0, 0, 0
                        )).unwrap();
                    }
                }
            }),
            // this is via physical keyboard
            Some(Opcode::HandlerTrigger) => {
                let rawstates = kbd.update();
                // interpret scancodes
                let kc: Vec<char> = kbd.track_keys(&rawstates);
                // handle keys, if any
                for &key in kc.iter() {
                    if key != '\u{000d}' {
                        cmdline.push(key);
                    } else {
                        send_message(cid, Message::new_scalar(
                            Opcode::DoCmd.to_usize().unwrap(), 0, 0, 0, 0
                        )).unwrap();
                    }
                }
            },
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(usbtest_sid).unwrap();
    xous::destroy_server(usbtest_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
