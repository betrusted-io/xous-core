#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use xous::ipc::*;
use api::{Registration, Lookup};
use core::fmt::Write;

pub fn register_name(name: &str) -> Result<bool, xous::Error> {
    let ns_id = xous::SID::from_bytes(b"xous-name-server").unwrap();
    let ns_conn = xous::connect(ns_id).unwrap();

    let registration = Registration::new();
    let mut sendable_registration = Sendable::new(registration)
        .expect("can't create sendable registration structure");
    write!(sendable_registration.name, "{}", name).unwrap();
    sendable_registration.lend_mut(ns_conn, sendable_registration.mid()).expect("nameserver registration failure!");

    Ok(sendable_registration.success)
}

/// note: if this throws an AccessDenied error, you can retry with a request_authenticat_connection() call (to be written)
pub fn request_connection(name: &str) -> Result<xous::CID, xous::Error> {
    let ns_id = xous::SID::from_bytes(b"xous-name-server").unwrap();
    let ns_conn = xous::connect(ns_id).unwrap();

    let lookup = Lookup::new();
    let mut sendable_lookup = Sendable::new(lookup)
    .expect("can't create sendable lookup structure");
    write!(sendable_lookup.name, "{}", name).unwrap();
    sendable_lookup.lend_mut(ns_conn, sendable_lookup.mid()).expect("nameserver lookup failure!");

    if sendable_lookup.success {
        Ok(sendable_lookup.cid)
    } else {
        if sendable_lookup.authenticate_request {
            Err(xous::Error::AccessDenied)
        } else {
            Err(xous::Error::ServerNotFound)
        }
    }
}