#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;
use xous::{CID, send_message};
use num_traits::*;

#[derive(Debug)]
pub struct Oqc {
    conn: CID,
}
impl Oqc {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_OQC).expect("Can't connect to OQC server");
        Ok(Oqc {
            conn
        })
    }
    pub fn trigger(&self, timeout: usize) {
        send_message(self.conn,
            xous::Message::new_blocking_scalar(Opcode::Trigger.to_usize().unwrap(), timeout, 0, 0, 0,)
        ).expect("couldn't trigger self test");
        // ignore return code, it's just to make sure the caller blocks until the test is done
    }
    pub fn status(&self) -> Option<bool> { // None if still running or not yet run; Some(true) if pass; Some(false) if fail
        let result = send_message(self.conn,
            xous::Message::new_blocking_scalar(Opcode::Status.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't query test status");
        match result {
            xous::Result::Scalar1(val) => {
                match val {
                    0 => return None,
                    1 => return Some(true),
                    2 => return Some(false),
                    _ => return Some(false),
                }
            }
            _ => {
                log::error!("internal error");
                panic!("improper result code on oqc status query");
            }
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Oqc {
    fn drop(&mut self) {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}