#![cfg_attr(target_os = "none", no_std)]

pub mod api;

pub fn request_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    xous_names::request_core(name, kbd_conn, api::SUBTYPE_REGISTER_BASIC_LISTENER)
}

pub fn request_raw_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    xous_names::request_core(name, kbd_conn, api::SUBTYPE_REGISTER_RAW_LISTENER)
}
