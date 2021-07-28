#![cfg_attr(target_os = "none", no_std)]

pub mod api;

pub use api::*;
use api::{Return, Opcode};
use xous::{send_message, CID, Message};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

#[derive(Debug)]
pub struct Rtc {
    conn: CID,
    callback_sid: Option<xous::SID>,
}
static mut RTC_CB: Option<fn(DateTime)> = None;
impl Rtc {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_RTC).expect("Can't connect to RTC");
        Ok(Rtc {
          conn,
          callback_sid: None,
        })
    }
    pub fn conn(&self) -> CID {self.conn}
    pub fn getop_set_ux(&self) -> u32 {Opcode::UxSetTime.to_u32().unwrap()}

    pub fn set_rtc_ux(&self) -> Result<(), xous::Error> {
        xous::send_message(self.conn,
            Message::new_scalar(Opcode::UxSetTime.to_usize().unwrap(),
            0, 0, 0, 0
            )
        ).map(|_| ())
    }
    pub fn set_rtc(&self, dt: DateTime) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(dt).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::SetDateTime.to_u32().unwrap()).map(|_| ())
    }
    pub fn unhook_rtc_callback(&mut self) -> Result<(), xous::Error> {
        unsafe{RTC_CB = None};
        if let Some(sid) = self.callback_sid {
            let sid_tuple = sid.to_u32();
            xous::send_message(self.conn,
            Message::new_scalar(Opcode::UnregisterDateTimeCallback.to_usize().unwrap(),
            sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }
    pub fn hook_rtc_callback(&mut self, cb: fn(DateTime)) -> Result<(), xous::Error> {
        log::trace!("hooking rtc callback");
        if unsafe{RTC_CB}.is_some() {
            return Err(xous::Error::MemoryInUse)
        }
        unsafe{RTC_CB = Some(cb)};
        let sid_tuple: (u32, u32, u32, u32);
        if let Some(sid) = self.callback_sid {
            sid_tuple = sid.to_u32();
        } else {
            let sid = xous::create_server().expect("Couldn't create RTC callback server");
            self.callback_sid = Some(sid);
            sid_tuple = sid.to_u32();
            xous::create_thread_4(rtc_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
        }
        xous::send_message(self.conn,
            Message::new_scalar(Opcode::RegisterDateTimeCallback.to_usize().unwrap(),
            sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
        )).unwrap();
        Ok(())
    }

    pub fn request_datetime(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RequestDateTime.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    /// wakeup alarm will force the system on if it is off, but does not trigger an interrupt on the CPU
    pub fn set_wakeup_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::SetWakeupAlarm.to_usize().unwrap(), seconds_from_now as _, 0, 0, 0)
        ).map(|_|())
    }
    pub fn clear_wakeup_alarm(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ClearWakeupAlarm.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    /// the rtc alarm will not turn the system on, but it will trigger an interrupt on the CPU
    pub fn set_rtc_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::SetRtcAlarm.to_usize().unwrap(), seconds_from_now as _, 0, 0, 0)
        ).map(|_|())
    }
    pub fn clear_rtc_alarm(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ClearRtcAlarm.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Rtc {
    fn drop(&mut self) {
        // if we have callbacks, destroy the callback server
        if let Some(sid) = self.callback_sid.take() {
            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                xous::Message::new_scalar(Return::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        // all implementations will need this
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}

fn rtc_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    log::trace!("rtc callback server started");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("rtc callback got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Return::ReturnDateTime) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = buffer.to_original::<DateTime,_>().unwrap();
                unsafe {
                    if let Some(cb) = RTC_CB {
                        cb(dt)
                    } else {
                        // callback happened after we unregistered
                        // this is a race condition, but it's also a harmless side effect
                        // we handle it by just ignoring the message
                        continue;
                    }
                }
            }
            Some(Return::Drop) => {
                break;
            }
            None => {
                log::error!("got unrecognized message in rtc CB server, ignoring");
            }
        }
    }
    log::trace!("rtc callback server exiting");
    xous::destroy_server(sid).expect("can't destroy my server on exit!");
}