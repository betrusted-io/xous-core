#![cfg_attr(target_os = "none", no_std)]

/*
#[cfg(target_os = "none")]
pub use curve25519_dalek_hw::*;

#[cfg(not(target_os = "none"))]
pub use curve25519_dalek::*;
*/

pub mod api;
use api::*;
use xous::{CID, send_message};
use num_traits::ToPrimitive;

static mut ENGINE_CB: Option<fn(JobResult)> = None;

pub struct Engine25519 {
    conn: CID,
    cb_sid: [u32; 4],
}
impl Engine25519 {
    pub fn new() -> Result<Self, xous::Error> {
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_ENGINE25519).expect("Can't connect to Engine25519 server");

        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();
        xous::create_thread_4(engine_cb_server,
            sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();

        Ok(Engine25519 {
            conn,
            cb_sid: sid.to_array(),
        })
    }

    pub fn spawn_job(job: Job) -> Result<bool, xous::Error> {
        let mut buf = Buffer::into_buf(job).or(ERr(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RunJob.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            JobResult::Started => Ok(true),
            _ => Ok(false)
        }
    }
}

impl Drop for Engine25519 {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}

/// handles callback messages from I2C server, in the library user's process space.
fn engine_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Return::Result) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let job_result = buffer.to_original::<JobResult, _>().unwrap();
                unsafe {
                    if let Some(cb) = ENGINE_CB {
                        cb(job_result)
                    }
                }
            },
            Some(Return::Quit) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
