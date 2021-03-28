#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use core::fmt::Write;

pub struct XousNames {
    conn: xous::CID,
}
impl XousNames {
    pub fn new() -> Result<Self, Error> {
        let conn = xous::connect(xous::SID::from_bytes(b"xous-name-server").unwrap()).expect("Couldn't connect to XousNames");
        Ok(XousNames {
           conn,
        })
    }
    pub fn register_name(&self, name: &str) -> Result<xous::SID, xous::Error> {

    }
}

pub fn register_name(name: &str) -> Result<xous::SID, xous::Error> {
    // Ensure we have a connection to the nameserver. If one exists, this is a no-op.
    let ns_id = xous::SID::from_bytes(b"xous-name-server").unwrap();
    let ns_conn = xous::connect(ns_id).unwrap();

    let mut registration_name = xous_ipc::String::<64>::new();
    write!(registration_name, "{}", name).expect("namserver: name probably too long");
    let request = api::Request::Register(registration_name);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&request).expect("nameserver: couldn't archive name")
    };
    let mut xous_buffer = writer.into_inner();

    xous_buffer
        .lend_mut(ns_conn, pos as u32)
        .expect("nameserver registration failure");

    let archived = unsafe { rkyv::archived_value::<api::Request>(xous_buffer.as_ref(), pos) };

    if let rkyv::Archived::<api::Request>::SID(sid) = archived {
        let sid = sid.into();
        xous::create_server_with_sid(sid).expect("can't auto-register server");
        Ok(sid)
    } else if let rkyv::Archived::<api::Request>::Failure = archived {
        return Err(xous::Error::InternalError);
    } else {
        panic!("Invalid response from the server -- corruption occurred");
    }
}

/// note: if this throws an AccessDenied error, you can retry with a request_authenticat_connection() call (to be written)
pub fn request_connection(name: &str) -> Result<xous::CID, xous::Error> {
    let ns_id = xous::SID::from_bytes(b"xous-name-server").unwrap();
    let ns_conn = xous::connect(ns_id).unwrap();

    let mut lookup_name = xous_ipc::String::<64>::new();
    write!(lookup_name, "{}", name).expect("nameserver: name problably too long");
    let request = api::Request::Lookup(lookup_name);

    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&request).expect("nameserver: couldn't archive name")
    };
    let mut xous_buffer = writer.into_inner();

    xous_buffer
        .lend_mut(ns_conn, pos as u32)
        .expect("nameserver lookup failure!");

    let archived = unsafe { rkyv::archived_value::<api::Request>(xous_buffer.as_ref(), pos) };
    match archived {
        rkyv::Archived::<api::Request>::CID(cid) => Ok(*cid),
        rkyv::Archived::<api::Request>::AuthenticateRequest(_) => Err(xous::Error::AccessDenied),
        _ => Err(xous::Error::ServerNotFound),
    }
}

/// note: you probably want to use this one, to avoid synchronization issues on startup as servers register asynhcronously
pub fn request_connection_blocking(name: &str) -> Result<xous::CID, xous::Error> {
    loop {
        match request_connection(name) {
            Ok(val) => return Ok(val),
            Err(xous::Error::AccessDenied) => return Err(xous::Error::AccessDenied),
            _ => (),
        }
        xous::yield_slice();
    }
}
