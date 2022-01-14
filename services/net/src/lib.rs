#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use com::Ipv4Conf;
use xous::{CID, send_message, Message};
use xous_ipc::Buffer;
use num_traits::*;

pub mod protocols;
pub use protocols::*;
pub use smoltcp::time::Duration;
pub use api::*;
pub use smoltcp::wire::IpEndpoint;

/// NetConn is a crate-level structure that just counts the number of connections from this process to
/// the Net server. It's not mean to be created by user-facing code, so the visibility is (crate).
pub(crate) struct NetConn {
    conn: CID,
}
impl NetConn {
    pub(crate) fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_NET).expect("Can't connect to Net server");
        Ok(NetConn {
            conn,
        })
    }
    pub(crate) fn conn(&self) -> CID {
        self.conn
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for NetConn {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}

pub struct NetManager {
    netconn: NetConn,
}
impl NetManager {
    pub fn new() -> NetManager {
        NetManager {
            netconn: NetConn::new(&xous_names::XousNames::new().unwrap()).expect("can't connect to Net Server"),
        }
    }
    pub fn get_ipv4_config(&self) -> Option<Ipv4Conf> {
        let storage = Some(Ipv4Conf::default().encode_u16());
        let mut buf = Buffer::into_buf(storage).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.netconn.conn(), Opcode::GetIpv4Config.to_u32().unwrap()).expect("Couldn't execute GetIpv4Config opcode");
        let maybe_config = buf.to_original().expect("couldn't restore config structure");
        if let Some(config) = maybe_config {
            let ipv4 = Ipv4Conf::decode_u16(&config);
            Some(ipv4)
        } else {
            None
        }
    }
    pub fn reset(&self) {
        send_message(
            self.netconn.conn(),
            Message::new_blocking_scalar(Opcode::Reset.to_usize().unwrap(), 0, 0, 0, 0),
        ).expect("couldn't send reset");
    }
}