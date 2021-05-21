#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Codec {
    conn: CID,
}
impl Codec {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_CODEC).expect("Can't connect to Codec server");
        Ok(Codec {
            conn
        })
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Codec {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}