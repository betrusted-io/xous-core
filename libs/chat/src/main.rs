#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use crate::server;
use api::*;

use xous::{Error, CID, SID};

pub fn main() -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns
        .register_name("Chat UI", None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", sid);

    let chat_sid = xous::create_server().unwrap();
    let chat_cid = xous::connect(chat_sid).unwrap();

    log::info!("Starting chat server",);
    thread::spawn({
        move || {
            server(chat_sid, None, None, None, None);
        }
    });
}
