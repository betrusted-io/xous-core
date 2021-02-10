#![cfg_attr(target_os = "none", no_std)]

use core::convert::TryInto;

use api::{REGISTER_BASIC_LISTENER, REGISTER_RAW_LISTENER};

pub mod api;

fn request(name: &str, kbd_conn: xous::CID, id: u32) -> Result<xous::Result, xous::Error> {
    let s: xous_names::api::XousServerName = name.try_into()?;
    let sendable = xous::ipc::Sendable::new(s)?;
    sendable.lend(kbd_conn, id)
}

pub fn request_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    request(name, kbd_conn, api::REGISTER_BASIC_LISTENER)
}

pub fn request_raw_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    request(name, kbd_conn, api::REGISTER_RAW_LISTENER)
}
