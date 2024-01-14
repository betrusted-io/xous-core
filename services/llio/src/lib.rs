#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;
pub use api::*;

pub mod i2c_lib;
pub use i2c_lib::I2c;
pub mod llio_lib;
use core::sync::atomic::{AtomicU32, Ordering};

pub use llio_lib::Llio;
static TIME_REFCOUNT: AtomicU32 = AtomicU32::new(0);

pub struct LocalTime {
    conn: xous::CID,
    warn_count: u32,
    time_init: bool,
}
impl LocalTime {
    pub fn new() -> LocalTime {
        TIME_REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xous::connect(xous::SID::from_bytes(b"timeserverpublic").unwrap()).unwrap();
        LocalTime { conn, warn_count: 0, time_init: false }
    }

    /// Returns the local time as milliseconds since EPOCH, assuming the time zone is set
    /// This is provided because we don't have a `libc` to do time zone lookups with `Chrono`.
    pub fn get_local_time_ms(&mut self) -> Option<u64> {
        if !self.time_init {
            match xous::send_message(
                self.conn,
                xous::Message::new_blocking_scalar(
                    6, // WallClockTimeInit -- this should not change because it's a libstd mapping
                    0, 0, 0, 0,
                ),
            )
            .expect("couldn't get init status")
            {
                xous::Result::Scalar1(is_init) => {
                    if is_init == 0 {
                        if self.warn_count < 10 || (self.warn_count & 0xff) == 0 {
                            log::warn!("Time offsets are not initialized, can't report local time");
                        }
                        self.warn_count += 1;
                        return None;
                    }
                }
                _ => {
                    log::error!("error retrieving time");
                    return None;
                }
            }
        }
        // if we got here, time was initialized. Set a flag so we don't check again in the future.
        // this reduces the chatter of messages that may routinely happen, e.g. for getting seconds updates on
        // the RTC.
        self.time_init = true;
        match xous::send_message(self.conn, xous::Message::new_blocking_scalar(4, 0, 0, 0, 0))
            .expect("couldn't get time")
        {
            xous::Result::Scalar2(hi, lo) => Some((hi as u64) << 32 | (lo as u64)),
            _ => {
                log::error!("error retrieving time");
                return None;
            }
        }
    }
    // Note: to get the UTC time since EPOCH, use the std::SystemTime::now()
}
impl Drop for LocalTime {
    fn drop(&mut self) {
        if TIME_REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
