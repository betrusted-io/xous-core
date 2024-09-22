#![cfg_attr(target_os = "none", no_std)]
use std::net::IpAddr;

use net::NetIpAddr;
use num_traits::ToPrimitive;
use xous::CID;
use xous_ipc::Buffer;

use crate::api::*;

#[derive(Debug)]
pub struct Dns {
    conn: CID,
}
impl Dns {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(crate::api::SERVER_NAME_DNS)
            .expect("Can't connect to Dns server");
        Ok(Dns { conn })
    }

    /// Checks first to see if the name could be just an IPv4 or IPv6 in string form,
    /// then tries to pass it to the DNS resolver.
    pub fn lookup(&self, name: &str) -> Result<NetIpAddr, DnsResponseCode> {
        if let Ok(simple_ip) = name.parse::<IpAddr>() {
            Ok(NetIpAddr::from(simple_ip))
        } else {
            let alloc_name = String::<DNS_NAME_LENGTH_LIMIT>::from_str(name);
            let mut buf = Buffer::into_buf(alloc_name).or(Err(DnsResponseCode::UnknownError))?;
            buf.lend_mut(self.conn, Opcode::Lookup.to_u32().unwrap())
                .or(Err(DnsResponseCode::UnknownError))?;
            let response = buf.to_original::<DnsResponse, _>().or(Err(DnsResponseCode::UnknownError))?;
            if let Some(addr) = response.addr { Ok(addr) } else { Err(response.code) }
        }
    }

    pub fn flush_cache(&self) -> Result<(), xous::Error> {
        xous::send_message(
            self.conn,
            xous::Message::new_scalar(Opcode::Flush.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Dns {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}
