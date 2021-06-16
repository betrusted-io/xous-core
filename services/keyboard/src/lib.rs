#![cfg_attr(target_os = "none", no_std)]

use num_traits::*;

pub mod api;

use api::{Opcode, KeyboardRegistration};
use xous::{send_message, Message};
use xous_ipc::{Buffer, String};

#[derive(Debug)]
pub struct Keyboard {
    conn: xous::CID,
}
impl Keyboard {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_KBD).expect("Can't connect to KBD");
        Ok(Keyboard {
            conn,
          })
    }

    pub fn register_listener(&self,
        // the public name of the destination server. The calling server must guarantee it has sufficient connection priveleges with xous-names to accept the keyboard's incoming connection.
        server_name: &str,
        // if Some(u32), the enum Opcode ID of the command for the incoming scancodes (adjusted already for keyup/down, repeat, shift, etc.)
        listener_id: Option<u32>,
        // if Some(u32), the enum Opcode ID of the command for incoming raw scancodes (just raw row/col locations and up/down)
        raw_listener_id: Option<u32>) -> Result<(), xous::Error> {

        let registration = KeyboardRegistration {
            server_name: String::<64>::from_str(server_name),
            listener_op_id: listener_id,
            rawlistener_op_id: raw_listener_id,
        };
        let buf = Buffer::into_buf(registration).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RegisterListener.to_u32().unwrap()).map(|_| ())
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
        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
