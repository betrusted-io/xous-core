#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use core::fmt::Write;
use xous_ipc::{String, Buffer};
use num_traits::ToPrimitive;

pub struct XousNames {
    conn: xous::CID,
}
impl XousNames {
    pub fn new() -> Result<Self, xous::Error> {
        let conn = xous::connect(xous::SID::from_bytes(b"xous-name-server").unwrap()).expect("Couldn't connect to XousNames");
        Ok(XousNames {
           conn,
        })
    }

    pub fn register_name(&self, name: &str) -> Result<xous::SID, xous::Error> {
        let mut registration_name = String::<64>::new();
        // could also do String::from_str() but in this case we want things to fail if the string is too long.
        write!(registration_name, "{}", name).expect("name probably too long");

        let mut buf = Buffer::into_buf(registration_name).or(Err(xous::Error::InternalError))?;

        buf.lend_mut(
            self.conn,
            api::Opcode::Register.to_u32().unwrap()
        )
        .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::SID(sid_raw) => {
                let sid = sid_raw.into();
                xous::create_server_with_sid(sid).expect("can't auto-register server");
                Ok(sid)
            }
            api::Return::Failure => {
                Err(xous::Error::InternalError)
            }
            _ => unimplemented!("unimplemented return codes")
        }
    }

    /// note: if this throws an AccessDenied error, you can retry with a request_authenticate_connection() call (to be written)
    pub fn request_connection(&self, name: &str) -> Result<xous::CID, xous::Error> {
        let mut lookup_name = xous_ipc::String::<64>::new();
        write!(lookup_name, "{}", name).expect("name problably too long");

        let mut buf = Buffer::into_buf(lookup_name).or(Err(xous::Error::InternalError))?;

        buf.lend_mut(
            self.conn,
            api::Opcode::Lookup.to_u32().unwrap()
        )
        .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::CID(cid) => Ok(cid),
            api::Return::AuthenticateRequest(_) => Err(xous::Error::AccessDenied),
            _ => Err(xous::Error::ServerNotFound),
        }
    }

    /// note: you probably want to use this one, to avoid synchronization issues on startup as servers register asynhcronously
    pub fn request_connection_blocking(&self, name: &str) -> Result<xous::CID, xous::Error> {
        loop {
            match self.request_connection(name) {
                Ok(val) => return Ok(val),
                Err(xous::Error::AccessDenied) => return Err(xous::Error::AccessDenied),
                _ => (),
            }
            xous::yield_slice();
        }
    }
}

impl Drop for XousNames {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}

    }
}