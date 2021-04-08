#![cfg_attr(target_os = "none", no_std)]

pub mod api;

pub use api::*;
use api::{Return, Opcode};
use xous::{send_message, CID, Message};
use xous_ipc::Buffer;
use num_traits::ToPrimitive;

#[derive(Debug)]
pub struct Rtc {
    conn: CID,
    callback_sid: Option<xous::SID>,
    llio: llio::Llio,
}
static mut RTC_CB: Option<fn(DateTime)> = None;
impl Rtc {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_RTC).expect("Can't connect to RTC");
        Ok(Rtc {
          conn,
          callback_sid: None,
          llio: llio::Llio::new(&xns).expect("Can't connect to LLIO on behalf of RTC library"),
        })
    }

    pub fn set_rtc(&self, dt: DateTime) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(dt).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::SetDateTime.to_u32().unwrap()).map(|_| ())
    }
    pub fn unhook_rtc_callback(&mut self) -> Result<(), xous::Error> {
        if let Some(sid) = self.callback_sid.take() {
            // tell my handler thread to quit
            let cid = xous::connect(sid).expect("can't connect to CB server for disconnect message");
            let msg = Return::Drop;
            let buf = Buffer::into_buf(msg).expect("can't send convert drop message");
            buf.lend(self.conn, 0).expect("can't send Drop message to CB server"); // there is only one message type, so ID field is disregarded
            unsafe{xous::disconnect(cid).expect("can't disconnect from CB server");}
            xous::destroy_server(sid).expect("can't destroy CB server");
        }
        self.callback_sid = None;
        unsafe{RTC_CB = None};
        Ok(())
    }
    pub fn hook_rtc_callback(&mut self, cb: fn(DateTime)) -> Result<(), xous::Error> {
        log::trace!("hooking rtc callback");
        if unsafe{RTC_CB}.is_some() {
            return Err(xous::Error::MemoryInUse)
        }
        unsafe{RTC_CB = Some(cb)};
        if self.callback_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.callback_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(rtc_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            xous::send_message(self.conn,
                Message::new_scalar(Opcode::RegisterDateTimeCallback.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }
    // this simply forwards the hook on to the LLIO library, which actually owns the Event peripheral where the interrupt is generated
    pub fn hook_rtc_alarm_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        self.llio.hook_rtc_alarm_callback(id, cid)
    }

    pub fn request_datetime(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RequestDateTime.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    /// wakeup alarm will force the system on if it is off, but does not trigger an interrupt on the CPU
    pub fn set_wakeup_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::SetWakeupAlarm.to_usize().unwrap(), seconds_from_now as _, 0, 0, 0)
        ).map(|_|())
    }
    pub fn clear_wakeup_alarm(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::ClearWakeupAlarm.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    /// the rtc alarm will not turn the system on, but it will trigger an interrupt on the CPU
    pub fn set_rtc_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::SetRtcAlarm.to_usize().unwrap(), seconds_from_now as _, 0, 0, 0)
        ).map(|_|())
    }
    pub fn clear_rtc_alarm(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::ClearRtcAlarm.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
}

impl Drop for Rtc {
    fn drop(&mut self) {
        // if we have callbacks, destroy the callback server
        if let Some(sid) = self.callback_sid.take() {
            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            let msg = Return::Drop;
            let buf = Buffer::into_buf(msg).unwrap();
            buf.lend(self.conn, 0).unwrap(); // there is only one message type, so ID field is disregarded
            unsafe{xous::disconnect(cid).unwrap();}
            xous::destroy_server(sid).unwrap();
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        // all implementations will need this
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}

fn rtc_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    log::trace!("rtc callback server started");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("rtc callback got msg: {:?}", msg);
        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
        let response = buffer.to_original::<Return,_>().unwrap();
        match response {
            Return::ReturnDateTime(dt) => {
                unsafe {
                    if let Some(cb) = RTC_CB {
                        cb(dt)
                    } else {
                        break;
                    }
                }
            }
            Return::Drop => {
                break;
            }
        }
    }
}