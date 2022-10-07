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
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xous::connect(xous::SID::from_bytes(b"xous-name-server").unwrap())
            .expect("Couldn't connect to XousNames");
        Ok(XousNames { conn })
    }

    /// Searches the name table and removes a given SID from the table.
    /// It's considered "secure" because you'd have to guess a random 128-bit SID
    /// to destroy someone else's SID.
    pub fn unregister_server(&self, sid: xous::SID) -> Result<(), xous::Error> {
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

    /// Register a server with a plaintext `name`. When specified, xous-names will
    /// limit the number of connections brokered to the value in `max_conns`. This
    /// effectively blocks further services from connecting to the server in a
    /// Trust-On-First-Use (TOFU) model.
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

    /// Request a connection to the server with `name`. If the connection is allowed,
    /// a 128-bit token is provided (in the form of a `[u32; 4]`) which can be used
    /// later on to disconnect from the server, effectively decrementing the total
    /// number of counts in against the `max_count` limit.
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
    /// Disconnects from server with `name`. Must provide the same `token` returned on
    /// connection, or else the call will be disregarded.
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
    /// Requests a permanent connection to server with `name`. Xous names brokers the
    /// entire connection, so the return value is the process-local CID (connection ID);
    /// the 128-bit server ID is never revealed.
    ///
    /// This call will fail if the server has not yet started up, which is a common
    /// problem during the boot process as the server start order is not guaranteed. Refer to
    /// `request_connection_blocking()` for a call that will automatically retry.
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

    /// Requests a permanent connection to server with `name`. Xous names brokers the
    /// entire connection, so the return value is the process-local CID (connection ID);
    /// the 128-bit server ID is never revealed.
    ///
    /// This call avoids synchronization issues on startup as servers register asynhcronously
    /// by retrying the connection call until the server appears.
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

    /// Returns `true` if every server that specified a `max_conn` count has filled
    /// every slot available. Once all the limited slots are filled, the system has
    /// finished TOFU initialization and can begin regular operations.
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
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for XousNames {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
