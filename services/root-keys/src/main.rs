#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use xous::{CID, msg_scalar_unpack, send_message, Message, msg_blocking_scalar_unpack};
use xous_ipc::{String, Buffer};

use num_traits::*;

use gam::modal::*;

use locales::t;

#[cfg(target_os = "none")]
mod implementation;
#[cfg(target_os = "none")]
use implementation::*;

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use crate::ROOTKEY_MODAL_NAME;
    use gam::modal::*;
    use xous_ipc::String;

    pub struct RootKeys {
        gam: gam::Gam,
        ticktimer: ticktimer_server::Ticktimer,
    }

    impl RootKeys {
        pub fn new(xns: &xous_names::XousNames) -> RootKeys {
            RootKeys {
                gam: gam::Gam::new(&xns).expect("couldn't allocate GAM for testing"),
                ticktimer: ticktimer_server::Ticktimer::new().expect("couldn't get ticktimer"),
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }

        pub fn try_init_keys(&mut self, _maybe_progress: Option<xous::SID>) {
        }

        pub fn test_ux(&mut self, arg: usize) {
            match arg {
                0 => self.gam.raise_modal(ROOTKEY_MODAL_NAME).expect("couldn't raise modal"),
                1 => self.gam.relinquish_focus().expect("couldn't hide modal"),
                _ => log::info!("test_ux got unrecognized arg: {}", arg),
            };
        }
    }
}

pub(crate) fn rootkeys_ux_thread() {
    let xns = xous_names::XousNames::new().unwrap();
    let main_conn = xns.request_connection_blocking(api::SERVER_NAME_KEYS).expect("rootkeys password thread can't connect to main thread");

    let password_action = gam::modal::TextEntry {
        is_password: true,
        visibility: gam::modal::TextEntryVisibility::LastChars,
        action_conn: main_conn,
        action_opcode: Opcode::PasswordModalEntry.to_u32().unwrap(),
        action_payload: TextEntryPayload::new(),
        validator: None,
    };
    log::trace!("building ux thread modal");
    let mut modal = gam::Modal::new(
        crate::ROOTKEY_MODAL_NAME,
        gam::ActionType::TextEntry(password_action),
        Some(t!("rootpass.top", xous::LANG)),
        None,
        GlyphStyle::Small,
        4
    );

    loop {
        let msg = xous::receive_message(modal.sid).unwrap();
        log::trace!("ux message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ModalOpcode::Redraw) => {
                modal.redraw();
            },
            Some(ModalOpcode::Rawkeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                modal.key_event(keys);
            }),
            Some(ModalOpcode::Quit) => {
                break;
            },
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("password modal thread exit, destroying servers");
    // do we want to add a deregister_ux call to the system?
    xous::destroy_server(modal.sid).unwrap();
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::RootKeys;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
       Connections allowed to the keys server:
          1. Shellchat (to originate update test requests)
          2. Password entry UX thread
          2. (future) PDDB
    */
    let keys_sid = xns.register_name(api::SERVER_NAME_KEYS, Some(2)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", keys_sid);

    let mut keys = RootKeys::new(&xns);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(keys_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    xous::create_thread_0(rootkeys_ux_thread).expect("couldn't start rootkeys UX thread");

    loop {
        let msg = xous::receive_message(keys_sid).unwrap();
        log::trace!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                keys.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                keys.resume();
            }),
            Some(Opcode::TryInitKeys) => msg_scalar_unpack!(msg, _, _, _, _, {
                keys.try_init_keys(None);
            }),
            Some(Opcode::TryInitKeysWithProgress) => msg_scalar_unpack!(msg, s0, s1, s2, s3, {
                let sid = xous::SID::from_u32(s0 as u32, s1 as u32, s2 as u32, s3 as u32);
                keys.try_init_keys(Some(sid));
            }),
            Some(Opcode::TestUx) => msg_blocking_scalar_unpack!(msg, arg, _, _, _, {
                keys.test_ux(arg);
                xous::return_scalar(msg.sender, 0).expect("couldn't unblock sender");
            }),
            Some(Opcode::PasswordModalEntry) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();
                log::info!("got pw entry: {}", plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after printing
                buf.volatile_clear();
            }
            Some(Opcode::Quit) => {
                log::warn!("password thread received quit, exiting.");
                break
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(keys_sid).unwrap();
    xous::destroy_server(keys_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
