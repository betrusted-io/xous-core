#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use api::{Return, Opcode}; // if you prefer to map the api into your local namespace
use xous::{send_message, Error, CID, Message, msg_scalar_unpack};
use xous_ipc::{String, Buffer};
use num_traits::{ToPrimitive, FromPrimitive};

pub struct Rtc {
    conn: CID,
    callback_sid: Option<xous::SID>,
}
static mut RTC_CB: Option<fn(DateTime)> = None;
impl Rtc {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_RTC).expect("Can't connect to RTC");
        Ok(Rtc {
          conn,
          callback_sid: None,
        })
    }

    pub fn set_rtc(&self, dt: DateTime) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(dt).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::SetDateTime.to_u32().unwrap()).map(|_| ())
    }
    pub fn rtc_callback(&mut self, cb: fn(DateTime)) -> Result<(), xous::Error> {
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
    pub fn request_datetime(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RequestDateTime.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    pub fn set_alarm(&self, seconds_from_now: u8) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::SetWakeupAlarm.to_usize().unwrap(), seconds_from_now as _, 0, 0, 0)
        ).map(|_|())
    }
    pub fn clear_alarm(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::ClearWakeupAlarm.to_usize().unwrap(), 0, 0, 0, 0)
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
    loop {
        let msg = xous::receive_message(sid).unwrap();
        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
        let response = buffer.to_original::<Return,_>().unwrap();
        match response {
            Return::ReturnDateTime(dt) => {
                unsafe {
                    if let Some(cb) = RTC_CB {
                        cb(dt)
                    }
                }
            }
            Return::Drop => {
                break;
            }
        }
    }
}