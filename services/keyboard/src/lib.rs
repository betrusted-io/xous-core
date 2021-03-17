#![cfg_attr(target_os = "none", no_std)]

use core::convert::TryInto;

pub mod api;

pub fn request_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    let s = xous::String::<64>::from_str(name);
    let request = api::Opcode::RegisterListener(s);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&request).expect("couldn't archive RegisterListener request")
    };
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(kbd_conn, pos.try_into().unwrap())
}

pub fn request_raw_events(name: &str, kbd_conn: xous::CID) -> Result<xous::Result, xous::Error> {
    let s = xous::String::<64>::from_str(name);
    let request = api::Opcode::RegisterRawListener(s);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&request).expect("couldn't archive RegisterRawListener request")
    };
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(kbd_conn, pos.try_into().unwrap())

}
