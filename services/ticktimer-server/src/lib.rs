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
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
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

    pub fn get_version(&self) -> String {
        let alloc = api::VersionString {
            version: xous_ipc::String::new(),
        };
        let mut buf = xous_ipc::Buffer::into_buf(alloc).expect("couldn't convert version request");
        buf.lend_mut(self.conn, api::Opcode::GetVersion.to_u32().unwrap())
            .expect("couldn't get version");
        let v = buf
            .to_original::<api::VersionString, _>()
            .expect("couldn't revert buffer");
        String::from(v.version.as_str().unwrap())
    }

    /// Lock the given Mutex. Blocks until the Mutex is locked.
    ///
    /// Note that Mutexes start out in a `Locked` state and move into an `Unlocked` state by calling
    /// `Unlock` on their pointer. For example, the following will probably block forever:
    ///
    ///     `TickTimer.lock_mutex(1)`
    ///
    /// In order to create a new Mutex, you must first `Unlock` it. For example, the following is
    /// allowed:
    ///
    ///     `TickTimer.unlock_mutex(1)`
    ///     `TickTimer.lock_mutex(1)`
    ///     `TickTimer.unlock_mutex(1)`
    ///
    /// # Arguments:
    ///
    ///     * mtx: A `usize` referring to the Mutex. This is probably a pointer, but can be any `usize`
    pub fn lock_mutex(&self, mtx: usize) {
        send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::LockMutex.to_usize().unwrap(),
                mtx,
                0,
                0,
                0,
            ),
        )
        .expect("couldn't lock mutex");
    }

    /// Unlock the given Mutex. Does not block. If the Mutex is not locked, then it will be
    /// "doubly-unlocked". That is, if you Unlock a mutex twice, then you can Lock it twice
    /// without blocking.
    ///
    /// # Arguments:
    ///
    ///     * mtx: A `usize` referring to the Mutex. This is probably a pointer, but can be any `usize`
    pub fn unlock_mutex(&self, mtx: usize) {
        send_message(
            self.conn,
            xous::Message::new_scalar(api::Opcode::UnlockMutex.to_usize().unwrap(), mtx, 0, 0, 0),
        )
        .expect("couldn't unlock mutex");
    }

    /// Wait for a Condition on the given condvar, with an optional Duration
    /// 
    /// # Arguments:
    ///
    ///     * condvar: A `usize` referring to the Condvar. This is probably a pointer, but can be any `usize`
    ///     * duration: The amount of time to wait for a signal, if any
    /// 
    /// # Returns:
    /// 
    ///     * true: the condition was successfully received
    ///     * false: the condition was not received and the operation itmed out
    pub fn wait_condition(&self, condvar: usize, duration: Option<core::time::Duration>) -> bool {
        send_message(
            self.conn,
            xous::Message::new_scalar(
                api::Opcode::WaitForCondition.to_usize().unwrap(),
                condvar,
                duration.map(|d| d.as_millis() as usize).unwrap_or(0),
                0,
                0,
            ),
        )
        .map(|r| r == xous::Result::Scalar1(0))
        .expect("couldn't wait for condition")
    }

    /// Notify a condition to one or more Waiters
    /// 
    /// # Arguments:
    ///
    ///     * condvar: A `usize` referring to the Condvar. This is probably a pointer, but can be any `usize`
    ///     * count: The number of Waiters to wake up
    /// 
    pub fn notify_condition(&self, condvar: usize, count: usize) {
        send_message(
            self.conn,
            xous::Message::new_scalar(
                api::Opcode::NotifyCondition.to_usize().unwrap(),
                condvar,
                count,
                0,
                0,
            ),
        )
        .map(|r| r == xous::Result::Scalar1(0))
        .expect("couldn't notify condition");
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Ticktimer {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
