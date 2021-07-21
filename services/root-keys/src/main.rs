#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use xous::{CID, msg_scalar_unpack, send_message, Message};

use num_traits::*;

#[cfg(target_os = "none")]
mod implementation;
#[cfg(target_os = "none")]
use implementation::*;

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct RootKeys {
    }

    impl RootKeys {
        pub fn new() -> RootKeys {
            RootKeys {
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
    use crate::implementation::RootKeys;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
       Connections allowed to the keys server:
          1. Shellchat (to originate update test requests)
          2. (future) PDDB
    */
    let keys_sid = xns.register_name(api::SERVER_NAME_KEYS, Some(1)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", keys_sid);

    let mut keys = RootKeys::new(&xns);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(keys_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(keys_sid).unwrap();
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
            None => {
                log::error!("couldn't convert opcode");
                break
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
