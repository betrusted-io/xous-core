#![cfg_attr(target_os = "none", no_std)]

//! Detailed docs are parked under Structs/XousNames down below

pub mod api;

use api::Disconnect;
use core::fmt::Write;
use num_traits::ToPrimitive;
use xous_ipc::{Buffer, String};

#[doc = include_str!("../README.md")]
#[derive(Debug)]
pub struct XousNames {
    conn: xous::CID,
}
impl XousNames {
    pub fn new() -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xous::connect(xous::SID::from_bytes(b"xous-name-server").unwrap())
            .expect("Couldn't connect to XousNames");
        Ok(XousNames { conn })
    }

    pub fn unregister_server(&self, sid: xous::SID) -> Result<(), xous::Error> {
        // searches the name table and removes a given SID from the table.
        // It's considered "secure" because you'd have to guess a random 128-bit SID
        // to destroy someone else's SID.

        // note that with the current implementation, the destroy call will have to be an O(N) search through
        // the server table, but this is OK as we expect <100 servers on a device
        let s = sid.to_array();
        let response = xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::Unregister.to_usize().unwrap(),
                s[0] as usize,
                s[1] as usize,
                s[2] as usize,
                s[3] as usize,
            ),
        )
        .expect("unregistration failed");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 {
                Ok(())
            } else {
                Err(xous::Error::ServerNotFound)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn register_name(
        &self,
        name: &str,
        max_conns: Option<u32>,
    ) -> Result<xous::SID, xous::Error> {
        let mut registration = api::Registration {
            name: String::<64>::new(),
            conn_limit: max_conns,
        };
        // could also do String::from_str() but in this case we want things to fail if the string is too long.
        write!(registration.name, "{}", name).expect("name probably too long");

        let mut buf = Buffer::into_buf(registration).or(Err(xous::Error::InternalError))?;

        buf.lend_mut(self.conn, api::Opcode::Register.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::SID(sid_raw) => {
                let sid = sid_raw.into();
                xous::create_server_with_sid(sid).expect("can't auto-register server");
                Ok(sid)
            }
            api::Return::Failure => Err(xous::Error::InternalError),
            _ => unimplemented!("unimplemented return codes"),
        }
    }

    pub fn request_connection_with_token(
        &self,
        name: &str,
    ) -> Result<(xous::CID, Option<[u32; 4]>), xous::Error> {
        let mut lookup_name = xous_ipc::String::<64>::new();
        write!(lookup_name, "{}", name).expect("name problably too long");
        let mut buf = Buffer::into_buf(lookup_name).or(Err(xous::Error::InternalError))?;

        buf.lend_mut(self.conn, api::Opcode::Lookup.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::CID((cid, token)) => Ok((cid, token)),
            // api::Return::AuthenticateRequest(_) => Err(xous::Error::AccessDenied),
            _ => Err(xous::Error::ServerNotFound),
        }
    }
    pub fn disconnect_with_token(&self, name: &str, token: [u32; 4]) -> Result<(), xous::Error> {
        let disconnect = Disconnect {
            name: String::<64>::from_str(name),
            token,
        };
        let mut buf = Buffer::into_buf(disconnect).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::Disconnect.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::Success => Ok(()),
            _ => Err(xous::Error::ServerNotFound),
        }
    }

    pub fn request_connection(&self, name: &str) -> Result<xous::CID, xous::Error> {
        let mut lookup_name = xous_ipc::String::<64>::new();
        write!(lookup_name, "{}", name).expect("name problably too long");

        let mut buf = Buffer::into_buf(lookup_name).or(Err(xous::Error::InternalError))?;

        buf.lend_mut(self.conn, api::Opcode::Lookup.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::CID((cid, _)) => Ok(cid),
            // api::Return::AuthenticateRequest(_) => Err(xous::Error::AccessDenied),
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
            log::info!("connection to {} could not be established, retrying", name);
            xous::yield_slice();
        }
    }

    pub fn trusted_init_done(&self) -> Result<bool, xous::Error> {
        let response = xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::TrustedInitDone.to_usize().unwrap(),
                0,
                0,
                0,
                0,
            ),
        )
        .expect("couldn't query trusted_init_done");
        if let xous::Result::Scalar1(result) = response {
            if result == 1 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    // todo:
    // pub fn authenticated_connection(&self, name: &str, key: Authkey)
    // this function will create an authenticated connection, if such are allowed
    // it's intended for use by dynamically-loaded third-party apps. As of Xous 0.8 this isn't supported, so it's just a "todo"
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for XousNames {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
