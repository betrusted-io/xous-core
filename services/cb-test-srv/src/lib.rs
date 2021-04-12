#![cfg_attr(target_os = "none", no_std)]
pub mod api;
use api::*;
use xous::{send_message, CID, Message, msg_scalar_unpack};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

pub struct CbTestServer {
    conn: CID,
    tick_cb_sid: Option<xous::SID>,
    add_cb_sid: Option<xous::SID>,
}
static mut ADD_CB: Option<fn(u32)> = None;
impl CbTestServer {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME).expect("Can't connect to callback test server");
        Ok(CbTestServer {
          conn,
          tick_cb_sid: None,
          add_cb_sid: None,
        })
    }
    pub fn add(&mut self, a: u32) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Add.to_usize().unwrap(), a as _, 0, 0, 0,)
        ).map(|_|())
    }
    pub fn hook_tick_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.tick_cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.tick_cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(tick_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::RegisterTickListener.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn hook_add_callback(&mut self, cb: fn(u32)) -> Result<(), xous::Error> {
        log::trace!("hooking add callback");
        if unsafe{ADD_CB}.is_some() {
            return Err(xous::Error::MemoryInUse)
        }
        unsafe{ADD_CB = Some(cb)};
        if self.add_cb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.add_cb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(add_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            xous::send_message(self.conn,
                Message::new_scalar(Opcode::RegisterAddListener.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }
    pub fn unhook_add_callback(&mut self) -> Result<(), xous::Error> {
        if let Some(sid) = self.add_cb_sid.take() {
            // tell my handler thread to quit
            log::trace!("connect for unhook");
            let cid = xous::connect(sid).expect("can't connect to CB server for disconnect message");
            log::trace!("sending drop to conn {}", cid);
            send_message(cid,
                Message::new_scalar(AddCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            log::trace!("disconnecting unhook connection");
            unsafe{
                match xous::disconnect(cid) {
                    Ok(_) => log::trace!("disconnected unhook connection"),
                    Err(e) => log::error!("unhook add got error: {:?}", e),
                };
            }
        }
        log::trace!("nullifying local state");
        self.add_cb_sid = None;
        unsafe{ADD_CB = None};
        Ok(())
    }
}
fn drop_conn(sid: xous::SID, id: usize) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid,
        Message::new_scalar(id, 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(cid).unwrap();}
}
impl Drop for CbTestServer {
    fn drop(&mut self) {
        if let Some(sid) = self.add_cb_sid.take() {
            drop_conn(sid, AddCallback::Drop.to_usize().unwrap());
        }
        if let Some(sid) = self.tick_cb_sid.take() {
            drop_conn(sid, TickCallback::Drop.to_usize().unwrap());
        }
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}


fn tick_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(TickCallback::Tick) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(TickCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

fn add_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    log::trace!("add callback server started");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("add callback got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(AddCallback::Sum) => msg_scalar_unpack!(msg, sum, _, _, _, {
                unsafe {
                    if let Some(cb) = ADD_CB {
                        cb(sum as u32)
                    } else {
                        break;
                    }
                }
            }),
            Some(AddCallback::Drop) => {
                break;
            }
            None => {
                log::error!("got unrecognized message in add CB server, ignoring");
            }
        }
    }
    log::trace!("add callback server exiting");
    xous::destroy_server(sid).expect("can't destroy my server on exit!");
}