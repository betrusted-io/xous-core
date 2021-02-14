#![cfg_attr(target_os = "none", no_std)]

use core::convert::TryInto;

use xous::buffer;
use rkyv::Write;

pub mod api;

pub fn request_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    let s: xous_names::api::XousServerName = name.try_into()?;
    let request = api::Opcode::RegisterListener(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&request).expect("couldn't archive RegisterListener request");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend(kbd_conn, pos.try_into().unwrap())
}

pub fn request_raw_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    let s: xous_names::api::XousServerName = name.try_into()?;
    let request = api::Opcode::RegisterRawListener(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&request).expect("couldn't archive RegisterListener request");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend(kbd_conn, pos.try_into().unwrap())

}
