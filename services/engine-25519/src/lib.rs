#![cfg_attr(target_os = "none", no_std)]

//! Detailed docs are parked under Structs/Engine25519 down below

/*
#[cfg(any(feature="precursor", feature="renode"))]
pub use curve25519_dalek_hw::*;

#[cfg(not(target_os = "xous"))]
pub use curve25519_dalek::*;
*/

pub mod api;
pub use api::*;
use num_traits::*;
use xous::{Message, CID};
use xous_ipc::Buffer;

static mut ENGINE_CB: Option<fn(JobResult)> = None;

#[doc = include_str!("../README.md")]
pub struct Engine25519 {
    conn: CID,
    cb_sid: Option<[u32; 4]>,
}
impl Engine25519 {
    // this is used to set up a system with async callbacks
    pub fn new_async(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(api::SERVER_NAME_ENGINE25519)
            .expect("Can't connect to Engine25519 server");

        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();
        xous::create_thread_4(
            engine_cb_server,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();

        Ok(Engine25519 { conn, cb_sid: Some(sid.to_array()) })
    }

    // typically, we just want synchronous callbacks. This integrates more nicely into single-threaded code.
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns
            .request_connection_blocking(api::SERVER_NAME_ENGINE25519)
            .expect("Can't connect to Engine25519 server");

        Engine25519 { conn, cb_sid: None }
    }

    /// this is unsafe because the caller must make sure that within their process space, multiple threads
    /// are not spawing concurrent jobs; or, if they do, they all share a common result_callback method:
    /// we always blindly replace result_callback!
    pub unsafe fn spawn_async_job(
        &mut self,
        job: &mut Job,
        result_callback: fn(JobResult),
    ) -> Result<bool, xous::Error> {
        if let Some(cb_sid) = self.cb_sid {
            ENGINE_CB = Some(result_callback); // this is the unsafe bit!
            job.id = Some(cb_sid);
            let mut buf = Buffer::into_buf(*job).or(Err(xous::Error::InternalError))?;
            buf.lend_mut(self.conn, Opcode::RunJob.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

            match buf.to_original().unwrap() {
                JobResult::Started => Ok(true),
                _ => Ok(false),
            }
        } else {
            Err(xous::Error::InvalidSyscall)
        }
    }

    pub fn montgomery_job(&mut self, job: MontgomeryJob) -> Result<[u8; 32], xous::Error> {
        let mut buf = Buffer::into_buf(job).or(Err(xous::Error::OutOfMemory))?;
        match buf.lend_mut(self.conn, Opcode::MontgomeryJob.to_u32().unwrap()) {
            Ok(_) => (),
            Err(e) => {
                if e == xous::Error::ServerNotFound {
                    log::error!(
                        "Looks like another thread called disconnect() on us while we weren't looking: {:?}",
                        e
                    );
                } else {
                    log::error!("couldn't lend buffer: {:?}", e);
                }
                return Err(e);
            }
        }

        match buf.to_original().unwrap() {
            JobResult::SingleResult(r) => Ok(r),
            JobResult::EngineUnavailable => {
                log::debug!("spawn job: engine unavailable");
                Err(xous::Error::ServerQueueFull)
            }
            JobResult::IllegalOpcodeException => {
                log::error!("spawn job: illegal opcode");
                Err(xous::Error::InvalidString)
            }
            _ => {
                log::error!("spawn job: other error");
                Err(xous::Error::UnknownError)
            }
        }
    }

    /// this is a blocking version of spawn_async_job.
    /// if the engine is free, it will block until a result is returned
    /// if the engine is busy, it will return an EngineUnavailable result.
    pub fn spawn_job(&mut self, job: Job) -> Result<[u32; RF_SIZE_IN_U32], xous::Error> {
        if job.id.is_some() {
            log::error!("spawn sync job: don't set a job id if you want a synchronous job!");
            return Err(xous::Error::InvalidSyscall); // the job.id should be None for a sync job. Do not set it to Some, even as a joke.
        }
        let mut buf = Buffer::into_buf(job).or(Err(xous::Error::OutOfMemory))?;
        match buf.lend_mut(self.conn, Opcode::RunJob.to_u32().unwrap()) {
            Ok(_) => (),
            Err(e) => {
                if e == xous::Error::ServerNotFound {
                    log::error!(
                        "Looks like another thread called disconnect() on us while we weren't looking: {:?}",
                        e
                    );
                } else {
                    log::error!("couldn't lend buffer: {:?}", e);
                }
                return Err(e);
            }
        }

        match buf.to_original().unwrap() {
            JobResult::Result(rf) => Ok(rf),
            JobResult::EngineUnavailable => {
                log::debug!("spawn job: engine unavailable");
                Err(xous::Error::ServerQueueFull)
            }
            JobResult::IllegalOpcodeException => {
                log::error!("spawn job: illegal opcode");
                Err(xous::Error::InvalidString)
            }
            _ => {
                log::error!("spawn job: other error");
                Err(xous::Error::UnknownError)
            }
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Engine25519 {
    fn drop(&mut self) {
        // disconnect from the main server, only if there are no more instances active
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // disconnect and destroy the callback server that is specific to this instance
        if let Some(cb_sid) = self.cb_sid {
            let cid = xous::connect(xous::SID::from_array(cb_sid)).unwrap();
            xous::send_message(cid, Message::new_scalar(Return::Quit.to_usize().unwrap(), 0, 0, 0, 0))
                .unwrap();
            unsafe {
                xous::disconnect(cid).unwrap();
            }
        }
    }
}

/// handles callback messages from the engine server, in the library user's process space.
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
            }
            Some(Return::Quit) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
