#![cfg_attr(target_os = "none", no_std)]

use num_traits::{ToPrimitive, FromPrimitive};

pub mod api;

use api::{Opcode, Callback, KeyRawStates};
use xous::{send_message, Message, msg_scalar_unpack};
use xous_ipc::Buffer;

static mut KBD_EVENT_CB: Option<fn([char; 4])> = None;
static mut KBD_RAW_CB: Option<fn(KeyRawStates)> = None;

#[derive(Debug)]
pub struct Keyboard {
    conn: xous::CID,
    event_cb_sid: Option<xous::SID>,
    raw_cb_sid: Option<xous::SID>,
}
impl Keyboard {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_KBD).expect("Can't connect to KBD");
        Ok(Keyboard {
            conn,
            event_cb_sid: None,
            raw_cb_sid: None,
          })
    }

    pub fn hook_keyboard_events(&mut self, cb: fn([char; 4])) -> Result<(), xous::Error> {
        if unsafe{KBD_EVENT_CB}.is_some() {
            return Err(xous::Error::MemoryInUse) // can't hook it twice
        }
        unsafe{KBD_EVENT_CB = Some(cb)};
        if self.event_cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.event_cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(event_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            send_message(self.conn,
                Message::new_scalar(Opcode::RegisterListener.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }

    pub fn hook_raw_events(&mut self, cb: fn(KeyRawStates)) -> Result<(), xous::Error> {
        if unsafe{KBD_RAW_CB}.is_some() {
            return Err(xous::Error::MemoryInUse) // can't hook it twice
        }
        unsafe{KBD_RAW_CB = Some(cb)};
        if self.raw_cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.raw_cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(raw_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            send_message(self.conn,
                Message::new_scalar(Opcode::RegisterRawListener.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }

    pub fn set_vibe(&self, enable: bool) -> Result<(), xous::Error> {
        let ena =
            if enable { 1 }
            else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::Vibe.to_usize().unwrap(),
            ena, 0, 0, 0,)
        ).map(|_| ())
    }

    #[cfg(not(target_os = "none"))]
    pub fn hostmode_inject_key(&self, c: char) {
        send_message(self.conn,
            Message::new_scalar(Opcode::InjectKey.to_usize().unwrap(),
               c as u32 as usize, 0, 0, 0
        )).unwrap();
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Keyboard {
    fn drop(&mut self) {
        // if we have callbacks, destroy the callback server
        if let Some(sid) = self.event_cb_sid.take() {
            // no need to tell the upstream server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            send_message(cid,
                Message::new_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }

        if let Some(sid) = self.raw_cb_sid.take() {
            // no need to tell the upstream server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            send_message(cid,
                Message::new_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}


/// handles callback messages from the keyboard server, in the library user's process space.
fn event_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Callback::KeyEvent) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                unsafe {
                    if let Some(cb) = KBD_EVENT_CB {
                        cb(keys)
                    }
                }
            }),
            Some(Callback::Drop) => {
                break; // this exits the loop and kills the thread
            },
            Some(Callback::KeyRawEvent) => panic!("wrong callback type issued: want KeyEvent, got KeyRawEvent!"),
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}


/// handles callback messages from the keyboard server, in the library user's process space.
fn raw_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Callback::KeyEvent) => panic!("wrong callback type issued: want KeyRawEvent, got KeyEvent!"),
            Some(Callback::Drop) => {
                break; // this exits the loop and kills the thread
            },
            Some(Callback::KeyRawEvent) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let krs = buffer.to_original::<KeyRawStates, _>().unwrap();
                unsafe {
                    if let Some(cb) = KBD_RAW_CB {
                        cb(krs)
                    }
                }
            },
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}