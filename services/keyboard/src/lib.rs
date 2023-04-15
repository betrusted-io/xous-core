#![cfg_attr(target_os = "none", no_std)]

use num_traits::*;

pub mod api;

pub use api::*;
use xous::{send_message, Message};
use xous_ipc::{Buffer, String};

#[derive(Debug)]
pub struct Keyboard {
    conn: xous::CID,
}
impl Keyboard {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_KBD).expect("Can't connect to KBD");
        Ok(Keyboard {
            conn,
          })
    }

    pub fn register_listener(&self, server_name: &str, action_opcode: usize) {
        let kr = KeyboardRegistration {
            server_name: String::<64>::from_str(server_name),
            listener_op_id: action_opcode
        };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, Opcode::RegisterListener.to_u32().unwrap())
        .expect("couldn't register listener");
    }

    pub fn register_raw_listener(&self, server_name: &str, action_opcode: usize) {
        let kr = KeyboardRegistration {
            server_name: String::<64>::from_str(server_name),
            listener_op_id: action_opcode
        };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, Opcode::RegisterRawListener.to_u32().unwrap())
        .expect("couldn't register listener");
    }

    pub fn register_observer(&self, server_name: &str, action_opcode: usize) {
        let kr = KeyboardRegistration {
            server_name: String::<64>::from_str(server_name),
            listener_op_id: action_opcode
        };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, Opcode::RegisterKeyObserver.to_u32().unwrap())
        .expect("couldn't register listener");
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

    pub fn set_keymap(&self, map: KeyMap) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::SelectKeyMap.to_usize().unwrap(),
            map.into(),
            0, 0, 0)
        ).map(|_| ())
    }
    pub fn get_keymap(&self) -> Result<KeyMap, xous::Error> {
        match send_message(self.conn,
            Message::new_blocking_scalar(Opcode::GetKeyMap.to_usize().unwrap(),
            0, 0, 0, 0)
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                Ok(code.into())
            }
            _ => Err(xous::Error::InternalError)
        }
    }
    /// Blocks until a key is hit. Does not block the keyboard server, just the caller.
    /// Returns a `Vec::<char>`, as the user can press more than one key at a time.
    /// The specific order of a simultaneous key hit event is not defined.
    pub fn get_keys_blocking(&self) -> Vec<char> {
        match send_message(self.conn,
            Message::new_blocking_scalar(
                Opcode::BlockingKeyListener.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ) {
            Ok(xous::Result::Scalar2(k1, k2)) => {
                let mut ret = Vec::<char>::new();
                if let Some(c) = core::char::from_u32(k1 as u32) {
                    ret.push(c)
                }
                if let Some(c) = core::char::from_u32(k2 as u32) {
                    ret.push(c)
                }
                ret
            }
            Ok(_) | Err(_) => panic!("internal error: Incorrect return type")
        }
    }
    /// Reveal the connection ID for use with unsafe FFI calls
    pub fn conn(&self) -> xous::CID { self.conn }

    #[cfg(not(target_os = "xous"))]
    pub fn hostmode_inject_key(&self, c: char) {
        send_message(self.conn,
            Message::new_scalar(Opcode::InjectKey.to_usize().unwrap(),
               c as u32 as usize, 0, 0, 0
        )).unwrap();
    }

    /// This is used to shove keys into the keyboard module as if you were typing on the
    /// physical keyboard.
    #[cfg(feature="inject-api")]
    pub fn inject_key(&self, c: char) {
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
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
