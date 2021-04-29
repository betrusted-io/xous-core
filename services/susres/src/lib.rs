#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use xous::{send_message, CID, Message, msg_scalar_unpack};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

pub struct Susres {
    conn: CID,
    suspend_cb_sid: Option<xous::SID>,
    execution_gate_conn: CID,
}
impl Susres {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SUSRES).expect("Can't connect to SUSRES");
        let execution_gate_conn = xns.request_connection_blocking(api::SERVER_NAME_EXEC_GATE).expect("Can't connect to the execution gate");
        Ok(Susres {
            conn,
            suspend_cb_sid: None,
            execution_gate_conn,
        })
    }
    pub fn hook_suspend_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.suspend_cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.suspend_cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(suspend_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::SuspendEventSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn initiate_suspend(&mut self) -> Result<(), xous::Error> {
        log::trace!("suspend initiated");
        send_message(self.conn,
            Message::new_scalar(Opcode::SuspendRequest.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
    pub fn suspend_until_resume(&mut self, token: usize) -> Result<(), xous::Error> {
        log::trace!("telling the server we're ready to suspend");
        // first tell the susres server that we're ready to suspend
        send_message(self.conn,
            Message::new_scalar(Opcode::SuspendReady.to_usize().unwrap(), token, 0, 0, 0)
        ).map(|_|())?;
        log::trace!("blocking until suspend");
        // now block until we've resumed
        send_message(self.execution_gate_conn,
            Message::new_blocking_scalar(ExecGateOpcode::SuspendingNow.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }
}
fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid,
        Message::new_scalar(SuspendEventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(cid).unwrap();}
}
impl Drop for Susres {
    fn drop(&mut self) {
        if let Some(sid) = self.suspend_cb_sid.take() {
            drop_conn(sid);
        }
        unsafe{xous::disconnect(self.conn).unwrap();}

    }
}
/// handles callback messages that indicate a USB interrupt has happened, in the library user's process space.
fn suspend_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SuspendEventCallback::Event) => msg_scalar_unpack!(msg, cid, id, token, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, token, 0, 0, 0)
                ).unwrap();
            }),
            Some(SuspendEventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}