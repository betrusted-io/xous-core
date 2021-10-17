#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;
use xous::{CID, send_message};
use xous_ipc::Buffer;
use num_traits::*;

pub mod backend;
pub use backend::*;
pub use smoltcp::time::Duration;

/// NetConn is a crate-level structure that just counts the number of connections from this process to
/// the Net server. It's not mean to be created by user-facing code, so the visibility is (crate).
pub(crate) struct NetConn {
    conn: CID,
    //cb_sid: Option<xous::SID>,
    //token: Option<[u32; 4]>,
}
impl NetConn {
    pub(crate) fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_NET).expect("Can't connect to Net server");
        Ok(NetConn {
            conn,
            //cb_sid: None,
            //token: None,
        })
    }
    pub(crate) fn conn(&self) -> CID {
        self.conn
    }
    // `cid` is the connection ID for the callback. The calling function would use `xous::connect(my_private_SID).unwrap()` to generate this number.
    // The SID should never be disclosed to another crate, as it is private to each server, which is why the CID creation call must exist in the caller's code.
/*
    pub fn hook_ping_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(net_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
                token: None,
            };
            let mut buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend_mut(self.conn, Opcode::NetCallbackSubscribe.to_u32().unwrap()).map(|_|())
            let retdata = buf.to_original::<ScalarHook, _>().unwrap();
            self.token = retdata.token;
            Ok(())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
*/
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for NetConn {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}
/*
/// handles callback messages that indicate an net packet has been returned, in the caller's process space
fn net_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => xous::msg_scalar_unpack!(msg, cid, id, free_play, avail_rec, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, free_play, avail_rec, 0, 0)
                ).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
*/