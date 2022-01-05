#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use num_traits::ToPrimitive;
use xous::{send_message, Error, CID};

#[derive(Debug)]
pub struct Ticktimer {
    conn: CID,
}
impl Ticktimer {
    pub fn new() -> Result<Self, Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap())?;
        Ok(Ticktimer { conn })
    }

    /// note special case for elapsed_ms() is "infalliable". it really should never fail so get rid of the Error
    pub fn elapsed_ms(&self) -> u64 {
        let response = send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::ElapsedMs.to_usize().unwrap(),
                0,
                0,
                0,
                0,
            ),
        )
        .expect("Ticktimer: failure to send message to Ticktimer");
        if let xous::Result::Scalar2(upper, lower) = response {
            upper as u64 | ((lower as u64) << 32)
        } else {
            panic!(
                "Ticktimer elapsed_ms(): unexpected return value: {:#?}",
                response
            );
        }
    }

    pub fn sleep_ms(&self, ms: usize) -> Result<(), Error> {
        send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::SleepMs.to_usize().unwrap(),
                ms,
                0,
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn ping_wdt(&self) {
        send_message(
            self.conn,
            xous::Message::new_scalar(api::Opcode::PingWdt.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't send WDT ping");
    }
}
use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Ticktimer {
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
