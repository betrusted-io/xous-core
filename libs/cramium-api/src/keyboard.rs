use core::fmt::Display;

#[derive(Debug, Default, Copy, Clone)]
#[allow(dead_code)]
pub struct ScanCode {
    /// base key value
    pub key: Option<char>,
    /// tap blue shift key, then key
    pub shift: Option<char>,
    /// hold blue shift key, then key
    pub hold: Option<char>,
    /// hold orange shift key, then key
    pub alt: Option<char>,
}

/// Maintainer note: there is a "BackupKeyboardLayout" serializer inside
/// root-keys/api.rs that needs to be updated when this changes.
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum KeyMap {
    Qwerty,
    Azerty,
    Qwertz,
    Dvorak,
    Braille,
    Undefined,
}
impl From<usize> for KeyMap {
    fn from(code: usize) -> Self {
        match code {
            0 => KeyMap::Qwerty,
            1 => KeyMap::Azerty,
            2 => KeyMap::Qwertz,
            3 => KeyMap::Dvorak,
            4 => KeyMap::Braille,
            _ => KeyMap::Qwerty,
        }
    }
}
impl From<KeyMap> for usize {
    fn from(map: KeyMap) -> usize {
        match map {
            // note: these indicese correspond to the position on the keyboard menu
            KeyMap::Qwerty => 0,
            KeyMap::Azerty => 1,
            KeyMap::Qwertz => 2,
            KeyMap::Dvorak => 3,
            KeyMap::Braille => 4,
            KeyMap::Undefined => 255,
        }
    }
}

impl Display for KeyMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Azerty => write!(f, "AZERTY"),
            Self::Qwerty => write!(f, "QWERTY"),
            Self::Qwertz => write!(f, "QWERTZ"),
            Self::Dvorak => write!(f, "Dvorak"),
            Self::Braille => write!(f, "Braille"),
            Self::Undefined => write!(f, "Undefined"),
        }
    }
}

// Opcodes are pinned down to allow for unsafe FFI extraction of key hits
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum KeyboardOpcode {
    /// set which keyboard mapping is present
    SelectKeyMap = 0, //(KeyMap),
    GetKeyMap = 1,

    /// request for ScanCodes
    RegisterListener = 2,

    /// request for updates for *when* keyboard is pressed
    RegisterKeyObserver = 12,

    /// set repeat delay, rate; both in ms
    SetRepeat = 4, //(u32, u32),

    /// set chording interval (how long to wait for all keydowns to happen before interpreting as a chord),
    /// in ms (for braille keyboards)
    SetChordInterval = 5, //(u32),

    /// used by host mode emulation and debug UART to inject keys
    InjectKey = 6, //(char),

    /// used by the interrupt handler to transfer results to the main loop
    HandlerTrigger = 7,

    /// a blocking key listener - blocks until a key is hit
    BlockingKeyListener = 9,
}

// this structure is used to register a keyboard listener.
#[cfg(feature = "std")]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct KeyboardRegistration {
    pub server_name: String,
    pub listener_op_id: usize,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RowCol {
    pub r: u8,
    pub c: u8,
}
impl RowCol {
    #[allow(dead_code)]
    pub fn new(r: u8, c: u8) -> RowCol { RowCol { r, c } }
}

use num_traits::*;
use xous::{Message, send_message};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Keyboard {
    conn: xous::CID,
}
impl Keyboard {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(crate::SERVER_NAME_KBD).expect("Can't connect to KBD");
        Ok(Keyboard { conn })
    }

    /// Listeners get passed the full content of the key press on each key hit.
    #[cfg(feature = "std")]
    pub fn register_listener(&self, server_name: &str, action_opcode: usize) {
        let kr =
            KeyboardRegistration { server_name: String::from(server_name), listener_op_id: action_opcode };
        let buf = Buffer::into_buf(kr).unwrap();
        buf.lend(self.conn, KeyboardOpcode::RegisterListener.to_u32().unwrap())
            .expect("couldn't register listener");
    }

    /// Observers get notified if a key is hit, but the actual keypress is always null.
    /// This is useful for hardware services that need to do e.g. screen wakeup on key hit
    /// but has no need to know the user's actually keystroke contents.
    #[cfg(feature = "std")]
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
    #[cfg(feature = "std")]
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
