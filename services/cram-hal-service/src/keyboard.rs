use num_traits::*;
use xous::{Message, send_message};
use xous_ipc::Buffer;

use crate::api::keyboard::*;

#[derive(Debug)]
pub struct Keyboard {
    conn: xous::CID,
}
impl Keyboard {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(crate::api::SERVER_NAME_KBD).expect("Can't connect to KBD");
        Ok(Keyboard { conn })
    }

    pub fn register_listener(&self, server_name: &str, action_opcode: usize) {
        let kr =
            KeyboardRegistration { server_name: String::from(server_name), listener_op_id: action_opcode };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, KeyboardOpcode::RegisterListener.to_u32().unwrap())
            .expect("couldn't register listener");
    }

    pub fn register_observer(&self, server_name: &str, action_opcode: usize) {
        let kr =
            KeyboardRegistration { server_name: String::from(server_name), listener_op_id: action_opcode };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, KeyboardOpcode::RegisterKeyObserver.to_u32().unwrap())
            .expect("couldn't register listener");
    }

    pub fn set_vibe(&self, _enable: bool) -> Result<(), xous::Error> {
        // no vibe on cramium target, ignore API call
        Ok(())
    }

    pub fn set_keymap(&self, map: KeyMap) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(KeyboardOpcode::SelectKeyMap.to_usize().unwrap(), map.into(), 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn get_keymap(&self) -> Result<KeyMap, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(KeyboardOpcode::GetKeyMap.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(code)) => Ok(code.into()),
            _ => Err(xous::Error::InternalError),
        }
    }

    /// Blocks until a key is hit. Does not block the keyboard server, just the caller.
    /// Returns a `Vec::<char>`, as the user can press more than one key at a time.
    /// The specific order of a simultaneous key hit event is not defined.
    pub fn get_keys_blocking(&self) -> Vec<char> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(KeyboardOpcode::BlockingKeyListener.to_usize().unwrap(), 0, 0, 0, 0),
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
            Ok(_) | Err(_) => panic!("internal error: Incorrect return type"),
        }
    }

    pub fn inject_key(&self, c: char) {
        send_message(
            self.conn,
            Message::new_scalar(KeyboardOpcode::InjectKey.to_usize().unwrap(), c as u32 as usize, 0, 0, 0),
        )
        .unwrap();
    }

    /// Reveal the connection ID for use with unsafe FFI calls
    pub fn conn(&self) -> xous::CID { self.conn }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Keyboard {
    fn drop(&mut self) {
        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using
        // the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
