#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::*;
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Trng {
    conn: CID,
    error_sid: Option<xous::SID>,
}
impl Trng {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_TRNG).expect("Can't connect to TRNG server");
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        Ok(Trng {
            conn,
            error_sid: None,
        })
    }
    pub fn get_u32(&self) -> Result<u32, xous::Error> {
        let response = send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::GetTrng.to_usize().unwrap(),
                1 /* count */, 0, 0, 0, )
            ).expect("TRNG|LIB: can't get_u32");
        if let xous::Result::Scalar2(trng, _) = response {
            Ok(trng as u32)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }
    pub fn get_u64(&self) -> Result<u64, xous::Error> {
        let response = send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::GetTrng.to_usize().unwrap(),
                2 /* count */, 0, 0, 0, )
        ).expect("TRNG|LIB: can't get_u32");
    if let xous::Result::Scalar2(lo, hi) = response {
            Ok( lo as u64 | ((hi as u64) << 32) )
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }
    pub fn hook_error_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.error_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.error_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(error_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = api::ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, api::Opcode::ErrorSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn get_health_tests(&self) -> Result<api::HealthTests, xous::Error> {
        let ht = api::HealthTests::default();
        let mut buf = Buffer::into_buf(ht).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::HealthStats.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(buf.to_original().unwrap())
    }
    pub fn get_error_stats(&self) -> Result<api::TrngErrors, xous::Error> {
        let errs = api::TrngErrors::default();
        let mut buf = Buffer::into_buf(errs).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::ErrorStats.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(buf.to_original().unwrap())
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Trng {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}

fn error_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::EventCallback::Event) => xous::msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    xous::Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(api::EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}