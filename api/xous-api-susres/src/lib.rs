#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(not(target_os = "none"), allow(unused_imports))]
#![cfg_attr(not(target_os = "none"), allow(unused_variables))]

pub mod api;
pub use api::*;
use num_traits::{FromPrimitive, ToPrimitive};
use xous::{CID, Message, msg_scalar_unpack, send_message};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Susres {
    conn: CID,
    suspend_cb_sid: Option<xous::SID>,
}
impl Susres {
    #[cfg(any(feature = "precursor", feature = "renode", feature = "bao1x"))]
    /// When created, the `susres` object can be configured with a `SuspendOrder` to enforce
    /// sequencing rules in shutdown. It also requires arguments to define a callback which is
    /// pinged when a suspend event arrives. The callback takes the form of a `CID, discriminant`
    /// pair, where the CID is the local connection ID to the caller (in other words, a self-connection
    /// to the caller), and the discriminant is the number placed into the Xous message's `body.id()`
    /// field. This is typically just the descriminant of the main loop's opcode enum for the
    /// suspend-resume opcode.
    pub fn new(
        order: Option<SuspendOrder>,
        xns: &xous_names::XousNames,
        cb_discriminant: u32,
        cid: CID,
    ) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SUSRES).expect("Can't connect to SUSRES");

        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();
        xous::create_thread_4(
            suspend_cb_server,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();
        let hookdata = ScalarHook {
            sid: sid_tuple,
            id: cb_discriminant,
            cid,
            order: order.unwrap_or(SuspendOrder::Normal),
        };
        log::debug!("hooking {:?}", hookdata);
        let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
        buf.lend(conn, Opcode::SuspendEventSubscribe.to_u32().unwrap())?;

        Ok(Susres { conn, suspend_cb_sid: Some(sid) })
    }

    // suspend/resume is not implemented in hosted mode, and will break if you try to do it.
    // the main reason this was done is actually it seems hosted mode can't handle the level
    // of concurrency introduced by suspend/resume, as its underlying IPC mechanisms are quite
    // different and have a lot of overhead; it seems like the system goes into a form of deadlock
    // during boot when all the hosted mode servers try to connect. This isn't an issue on real hardware.
    #[cfg(not(target_os = "xous"))]
    /// When created, the `susres` object can be configured with a `SuspendOrder` to enforce
    /// sequencing rules in shutdown. It also requires arguments to define a callback which is
    /// pinged when a suspend event arrives. The callback takes the form of a `CID, discriminant`
    /// pair, where the CID is the local connection ID to the caller (in other words, a self-connection
    /// to the caller), and the discriminant is the number placed into the Xous message's `body.id()`
    /// field. This is typically just the descriminant of the main loop's opcode enum for the
    /// suspend-resume opcode.
    pub fn new(
        _ordering: Option<SuspendOrder>,
        xns: &xous_names::XousNames,
        cb_discriminant: u32,
        cid: CID,
    ) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        Ok(Susres { conn: 0, suspend_cb_sid: None })
    }

    /// Creates a connection to the `susres` server, but without a callback. This is useful
    /// for services that are suspend-insensitive, but need to manipulate the state of
    /// the machine (such as initiating a suspend).
    pub fn new_without_hook(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SUSRES)?;
        Ok(Susres { conn, suspend_cb_sid: None })
    }

    /// This call initiates a suspend. It will sequence through the suspend events; and
    /// if any services are unable to suspend within a defined time-out window, the call
    /// will fail with a `xous::Error::Timeout`.
    ///
    /// NB: services such as the SHA engine and
    /// the Curve25519 engine can deny a suspend because they have large internal state
    /// that can't be saved to battery-backed RAM.
    pub fn initiate_suspend(&self) -> Result<(), xous::Error> {
        log::trace!("suspend initiated");
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::SuspendRequest.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(result)) => {
                if result == 1 {
                    Ok(())
                } else {
                    // indicate that we couldn't initiate the suspend
                    Err(xous::Error::Timeout)
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    /// This call is used by services that are suspend-sensitive. They are used to
    /// acknowledge the callback from the suspend sequencer; calling this function
    /// basically tells the sequencer "I'm ready to suspend immediately". Likewise,
    /// this call blocks until the system resumes.
    ///
    /// Note that from the perspective of the caller, this call magically appears
    /// like it returns almost immediately, because, the time spent in suspend does
    /// not increment the ticktimer or system time. The only evidence of a suspend
    /// would be a difference in the current RTC timestamp.
    pub fn suspend_until_resume(&mut self, token: usize) -> Result<bool, xous::Error> {
        if self.suspend_cb_sid.is_none() {
            // this happens if you created without a hook
            return Err(xous::Error::UseBeforeInit);
        }
        log::debug!("token {} pid {} suspending", token, xous::process::id()); // <-- use this to debug s/r
        xous::yield_slice();
        // first tell the susres server that we're ready to suspend
        send_message(
            self.conn,
            Message::new_scalar(Opcode::SuspendReady.to_usize().unwrap(), token, 0, 0, 0),
        )
        .map(|_| ())?;
        log::trace!("blocking until suspend");

        // sometime between here and when this next message unblocks, the power went out...

        // now block until we've resumed
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::SuspendingNow.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())?;

        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WasSuspendClean.to_usize().unwrap(), token, 0, 0, 0),
        )
        .expect("couldn't query if my suspend was successful");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 {
                log::debug!("resume pid {} clean", xous::process::id()); // <-- use this to debug s/r
                Ok(true)
            } else {
                log::debug!("resume pid {} dirty", xous::process::id()); // <-- use this to debug s/r
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    /// This is a call that a service can make to inform the suspend sequencer that
    /// it is currently suspendable (or not suspendable). This is typically used to
    /// book-end calls to hardware that contains large amount of state that cannot
    /// be efficiently saved to battery-backed RAM.
    pub fn set_suspendable(&mut self, allow_suspend: bool) -> Result<(), xous::Error> {
        if allow_suspend {
            send_message(self.conn, Message::new_scalar(Opcode::SuspendAllow.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        } else {
            send_message(self.conn, Message::new_scalar(Opcode::SuspendDeny.to_usize().unwrap(), 0, 0, 0, 0))
                .map(|_| ())
        }
    }

    /// Passing `true` causes the whole SOC including peripherals to receive a reset signal
    /// `false` causes only the CPU to reboot, while the peripherals retain state. Generally you want `true`.
    pub fn reboot(&self, whole_soc: bool) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::RebootRequest.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())?;

        if whole_soc {
            send_message(
                self.conn,
                Message::new_scalar(Opcode::RebootSocConfirm.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .map(|_| ())
        } else {
            send_message(
                self.conn,
                Message::new_scalar(Opcode::RebootCpuConfirm.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .map(|_| ())
        }
    }

    /// Pulls power from the SoC without attempting to save state
    pub fn immediate_poweroff(&self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::PowerOff.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }
}
fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid, Message::new_scalar(SuspendEventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0))
        .unwrap();
    unsafe {
        xous::disconnect(cid).unwrap();
    }
}
use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Susres {
    fn drop(&mut self) {
        if let Some(sid) = self.suspend_cb_sid.take() {
            drop_conn(sid);
        }
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

fn suspend_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let mut print_once = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SuspendEventCallback::Event) => msg_scalar_unpack!(msg, cid, id, token, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                if !print_once {
                    // dump this only once so we have a PID->token map in the debug logs, but don't dump it
                    // every time as it slows down the suspend
                    log::info!("PID {} has s/r token {}", xous::current_pid().unwrap().get(), token); // <-- use this to debug s/r
                    print_once = true;
                }
                send_message(cid as u32, Message::new_scalar(id, token, 0, 0, 0)).unwrap();
            }),
            Some(SuspendEventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
