#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
use xous::{CID, send_message};
use num_traits::*;

pub struct U2f {
    conn: CID,
}
impl U2f {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().expect("couldn't connect to XousNames");
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_U2F).expect("Can't connect to U2f server");
        U2f {
            conn
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for U2f {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}