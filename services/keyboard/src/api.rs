use heapless::consts::*;
use heapless::Vec;
use xous::{Message, ScalarMessage};

pub const REGISTER_BASIC_LISTENER: u32 = 0x1000_0000;
pub const REGISTER_RAW_LISTENER: u32 = 0x1000_0001;

#[derive(Debug, Default, Copy, Clone)]
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

#[derive(Debug, Default, Copy, Clone)]
pub struct RowCol {
    pub r: u8,
    pub c: u8,
}

#[derive(Debug)]
#[repr(C)]
pub struct KeyRawStates {
    mid: u32,
    pub keydowns: Vec<RowCol, U16>,
    pub keyups: Vec<RowCol, U16>,
}
impl KeyRawStates {
    pub fn mid(&self) -> usize {
        self.mid as usize
    }

    pub fn new() -> Self {
        KeyRawStates {
            mid: xous::names::GID_KEYBOARD_RAW_KEYSTATE_EVENT as u32,
            keydowns: Vec::new(),
            keyups: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn copy(&self) -> KeyRawStates {
        let mut krs = KeyRawStates::new();
        for kd in self.keydowns.iter() {
            krs.keydowns.push(*kd).unwrap();
        }
        for ku in self.keyups.iter() {
            krs.keyups.push(*ku).unwrap();
        }
        krs
    }
}

#[derive(Debug, Copy, Clone, rkyv::Archive)]
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
            _ => KeyMap::Undefined,
        }
    }
}
impl Into<usize> for KeyMap {
    fn into(self) -> usize {
        match self {
            KeyMap::Qwerty => 0,
            KeyMap::Azerty => 1,
            KeyMap::Qwertz => 2,
            KeyMap::Dvorak => 3,
            KeyMap::Braille => 4,
            KeyMap::Undefined => 255,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive)]
pub enum Opcode {
    /// set which keyboard mapping is present
    SelectKeyMap(KeyMap),

    /// request interpreted ScanCodes to be sent
    RegisterListener(xous_names::api::XousServerName),

    /// request raw keyup/keydown events to be sent
    RegisterRawListener(xous_names::api::XousServerName),

    /// set repeat delay, rate; both in ms
    SetRepeat(u32, u32),

    /// set chording interval (how long to wait for all keydowns to happen before interpreting as a chord), in ms (for braille keyboards)
    SetChordInterval(u32),

    /// keyboard events (as sent to listeners)
    KeyboardEvent([char; 4]),

    /// used by host mode emulation to inject keys
    HostModeInjectKey(char),
}

impl core::convert::TryFrom<&Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::SelectKeyMap(KeyMap::from(m.arg1))),
                1 => Ok(Opcode::SetRepeat(m.arg1 as u32, m.arg2 as u32)),
                2 => Ok(Opcode::SetChordInterval(m.arg1 as u32)),
                xous::names::GID_KEYBOARD_KEYSTATE_EVENT => Ok(Opcode::KeyboardEvent([
                    if let Some(a) = core::char::from_u32(m.arg1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(m.arg2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(m.arg3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(m.arg4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ])),
                3 => Ok(Opcode::HostModeInjectKey(
                    if let Some(a) = core::char::from_u32(m.arg1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                )),
                _ => Err("KBD api: unknown Scalar ID"),
            },
            _ => Err("KBD api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::SelectKeyMap(map) => Message::Scalar(ScalarMessage {
                id: 0,
                arg1: map.into(),
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::SetRepeat(delay, rate) => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: delay as usize,
                arg2: rate as usize,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::SetChordInterval(period) => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: period as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::KeyboardEvent(keys) => Message::Scalar(ScalarMessage {
                id: xous::names::GID_KEYBOARD_KEYSTATE_EVENT,
                arg1: keys[0] as u32 as usize,
                arg2: keys[1] as u32 as usize,
                arg3: keys[2] as u32 as usize,
                arg4: keys[3] as u32 as usize,
            }),
            Opcode::HostModeInjectKey(key) => Message::Scalar(ScalarMessage {
                id: 3,
                arg1: key as u32 as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            _ => panic!("KBD api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}
