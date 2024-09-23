#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use api::*;
use num_traits::FromPrimitive;
use xous::{CID, Error, SID};

pub fn main() -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name("Chat UI", None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", sid);

    let chat_sid = xous::create_server().unwrap();
    let chat_cid = xous::connect(chat_sid).unwrap();

    let busy_bumper = xous::create_server().unwrap();
    let busy_bumper_cid = xous::connect(busy_bumper).unwrap();

    log::info!("starting idle animation runner");
    let run_busy_animation = Arc::new(AtomicBool::new(false));
    thread::spawn({
        let run_busy_animation = run_busy_animation.clone();
        move || {
            busy_animator(busy_bumper, busy_bumper_cid, chat_cid, run_busy_animation);
        }
    });
    log::info!("Starting chat server",);
    thread::spawn({
        move || {
            server(chat_sid, None, None, run_busy_animation, busy_bumper_cid);
        }
    });
    xous::terminate_process(0)
}
