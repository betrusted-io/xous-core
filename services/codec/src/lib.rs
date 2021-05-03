#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Codec {
    conn: CID,
    frame_sid: Option<xous::SID>,
}
impl Codec {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_CODEC).expect("Can't connect to Codec server");
        Ok(Codec {
            conn,
            frame_sid: None,
        })
    }
    pub fn hook_frame_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.frame_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.frame_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(frame_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::AudioStreamSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn setup_8k_stream(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Setup8kStereo.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}

    }
}

/// handles callback messages that indicate an audio frame has been used up, in the library user's process space.
fn frame_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
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
