#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use xous::ipc::*;
use core::fmt::Write;

fn request_core(name: &str, kbd_conn: xous::CID, subtype: u8) -> Result<xous::Result, xous::Error> {
    let registration = xous_names::api::Registration::new();
    let mut sendable_registration = Sendable::new(registration)
        .expect("can't create sendable registration structure");
    sendable_registration.set_subtype(subtype);
    write!(sendable_registration.name, "{}", name).unwrap();
    sendable_registration.lend_mut(kbd_conn, sendable_registration.mid()).expect("keyboard event request registration failure!");

    if sendable_registration.success {
        Ok(xous::Result::Ok)
    } else {
        Err(xous::Error::InternalError)
    }
}

pub fn request_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    request_core(name, kbd_conn, api::SUBTYPE_REGISTER_BASIC_LISTENER)
}

pub fn request_raw_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    request_core(name, kbd_conn, api::SUBTYPE_REGISTER_RAW_LISTENER)
}
