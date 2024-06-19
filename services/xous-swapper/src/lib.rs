/// public userspace & swapper handler -> swapper userspace ABI
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Userspace request to GC some physical pages
    GarbageCollect,
    /// Test messages
    #[cfg(feature = "swap-userspace-testing")]
    Test0,
    None,
}

pub const SWAPPER_PUBLIC_NAME: &'static str = "_swapper server_";

pub struct Swapper {
    conn: xous::CID,
}
impl Swapper {
    pub fn new() -> Result<Self, xous::Error> {
        let xns = xous_api_names::XousNames::new().expect("couldn't connect to xous names");
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(SWAPPER_PUBLIC_NAME).expect("Can't connect to TRNG server");
        Ok(Swapper { conn })
    }

    /// Attempts to free `page_count` pages of RAM.
    pub fn garbage_collect_pages(&self, page_count: usize) -> usize {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(Opcode::GarbageCollect as usize, page_count, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(free_pages, _, _, _, _)) => free_pages,
            _e => {
                log::warn!("Garbage collect call failed with internal error: {:?}", _e);
                0
            }
        }
        // no result is given, but the call blocks until the GC call has completed in the swapper.
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Swapper {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
