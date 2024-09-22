#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use num_traits::ToPrimitive;
use xous::{CID, Error, send_message};
use xous_semver::SemVer;

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

    /// Return the number of milliseconds that have elapsed since boot. The returned
    /// value is guaranteed to always be the same or greater than the previous value,
    /// even through suspend/resume cycles. During suspend, the counter does not
    /// advance, so loops which rely on this value will not perceive any extra time
    /// passing through a suspend/resume cycle.
    ///
    /// This call is expected to be infalliable, and removing the error handling
    /// path makes it a little more efficient in a tight loop.
    ///
    /// # Returns:
    ///
    ///     * A `u64` that is the number of milliseconds elapsed since boot.
    pub fn elapsed_ms(&self) -> u64 {
        let response = send_message(
            self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::ElapsedMs.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Ticktimer: failure to send message to Ticktimer");
        if let xous::Result::Scalar2(upper, lower) = response {
            upper as u64 | ((lower as u64) << 32)
        } else {
            panic!("Ticktimer elapsed_ms(): unexpected return value.");
        }
    }

    /// Sleep for at least `ms` milliseconds. Blocks until the requested time has passed.
    ///
    /// # Arguments:
    ///
    ///     * ms: A `usize` specifying how many milliseconds to sleep for
    pub fn sleep_ms(&self, ms: usize) -> Result<(), Error> {
        send_message(
            self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::SleepMs.to_usize().unwrap(), ms, 0, 0, 0),
        )
        .map(|_| ())
    }

    /// Ping the watchdog timer. Processes may use this to periodically ping the WDT to prevent
    /// the system from resetting itself. Note that every call to `sleep_ms()` also implicitly
    /// pings the WDT, so in more complicated systems an explicit call is not needed.
    pub fn ping_wdt(&self) {
        send_message(
            self.conn,
            xous::Message::new_scalar(api::Opcode::PingWdt.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't send WDT ping");
    }

    /// Query version information embedded in this implementation crate by the build system.
    ///
    /// # Returns:
    ///
    ///     * A `String` containing the version information of the latest build
    pub fn get_version(&self) -> String {
        let alloc = api::VersionString { version: String::new() };
        let mut buf = xous_ipc::Buffer::into_buf(alloc).expect("couldn't convert version request");
        buf.lend_mut(self.conn, api::Opcode::GetVersion.to_u32().unwrap()).expect("couldn't get version");
        let v = buf.to_original::<api::VersionString, _>().expect("couldn't revert buffer");
        v.version
    }

    pub fn get_version_semver(&self) -> SemVer {
        SemVer::from_str(self.get_version().lines().next().unwrap()).unwrap()
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
            xous::Message::new_blocking_scalar(api::Opcode::LockMutex.to_usize().unwrap(), mtx, 0, 0, 0),
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
    pub fn notify_condition(&self, condvar: usize, count: usize) {
        send_message(
            self.conn,
            xous::Message::new_scalar(api::Opcode::NotifyCondition.to_usize().unwrap(), condvar, count, 0, 0),
        )
        .map(|r| r == xous::Result::Scalar1(0))
        .expect("couldn't notify condition");
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Ticktimer {
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
