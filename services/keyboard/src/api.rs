use xous::{Message, ScalarMessage};
use heapless::Vec;
use heapless::consts::*;

pub const SUBTYPE_REGISTER_BASIC_LISTENER: u8 = 0;
pub const SUBTYPE_REGISTER_RAW_LISTENER: u8 = 1;

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

#[derive(Debug)]
#[repr(C)]
pub struct KeyRawStates {
    mid: usize,
    pub keydowns: Vec<(usize, usize), U16>,
    pub keyups: Vec<(usize, usize), U16>,
}
impl KeyRawStates {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        KeyRawStates {
            mid: xous::names::GID_KEYBOARD_RAW_KEYSTATE_EVENT,
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

/*
#[derive(Debug)]
#[repr(C)]
pub struct KeyStates {
    mid: usize,
    pub keys: Vec<char, U16>,
}
impl KeyStates {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        KeyStates {
            mid: ID_KEYSTATE,
            keys: Vec::new(),
        }
    }

    pub fn copy(&self) -> KeyStates {
        let mut ks = KeyStates::new();
        for sc in self.keys.iter() {
            ks.keys.push(*sc).unwrap();
        }
        ks
    }
}*/

#[derive(Debug, Copy, Clone)]
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
#[derive(Debug)]
pub enum Opcode {
    /// set which keyboard mapping is present
    SelectKeyMap(KeyMap),

    /// request interpreted ScanCodes to be sent
    RegisterListener(xous_names::api::Registration),

    /// request raw keyup/keydown events to be sent
    RegisterRawListener(xous_names::api::Registration),

    /// set repeat delay, rate; both in ms
    SetRepeat(usize, usize),

    /// set chording interval (how long to wait for all keydowns to happen before interpreting as a chord), in ms (for braille keyboards)
    SetChordInterval(usize),

    /// keyboard events (as sent to listeners)
    KeyboardEvent([char; 4]),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::SelectKeyMap(KeyMap::from(m.arg1))),
                1 => Ok(Opcode::SetRepeat(m.arg1, m.arg2)),
                2 => Ok(Opcode::SetChordInterval(m.arg1)),
                xous::names::GID_KEYBOARD_KEYSTATE_EVENT =>
                     Ok(Opcode::KeyboardEvent([
                        if let Some(a) = core::char::from_u32(m.arg1 as u32) { a } else { '\u{0000}' },
                        if let Some(a) = core::char::from_u32(m.arg2 as u32) { a } else { '\u{0000}' },
                        if let Some(a) = core::char::from_u32(m.arg3 as u32) { a } else { '\u{0000}' },
                        if let Some(a) = core::char::from_u32(m.arg4 as u32) { a } else { '\u{0000}' }
                         ])),
                _ => Err("KBD api: unknown Scalar ID"),
            },
            Message::Borrow(m) => {
                if (m.id & 0xFF) as u8 == SUBTYPE_REGISTER_BASIC_LISTENER {
                    Ok(Opcode::RegisterListener({
                        unsafe { *( (m.buf.as_mut_ptr()) as *mut xous_names::api::Registration) }
                    }))
                } else if (m.id & 0xFF) as u8 == SUBTYPE_REGISTER_RAW_LISTENER {
                    Ok(Opcode::RegisterRawListener({
                        unsafe { *( (m.buf.as_mut_ptr()) as *mut xous_names::api::Registration) }
                    }))
                } else {
                    Err("KBD api: unknown Borrow ID")
                }
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
                arg1: delay,
                arg2: rate,
                arg3: 0, arg4: 0,
            }),
            Opcode::SetChordInterval(period) => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: period,
                arg2: 0, arg3: 0, arg4: 0,
            }),
            Opcode::KeyboardEvent(keys) => Message::Scalar(ScalarMessage {
                id: xous::names::GID_KEYBOARD_KEYSTATE_EVENT,
                arg1: keys[0] as u32 as usize,
                arg2: keys[1] as u32 as usize,
                arg3: keys[2] as u32 as usize,
                arg4: keys[3] as u32 as usize,
            }),
            _ => panic!("KBD api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}

/*
enum DecodedRegistration {
    reg: &'static xous_names::api::Regustration,
    envelope: MessageEnvelope,
}

enum DecodedOpcode {
    Registration(DecodedRegistration),
}

impl DecodedOpcode {
    pub fn decode(envelope: MessageEnvelope) -> Result<Self, &'static str> {
        match message.body {
        Message::MutableBorrow(m) => match m.id {
            ID_REGISTER_NAME => Ok(DecodedOpcode::Registration{
                envelope,
                reg: &{*{m.buf.as_ptr() as *const xous_names::api::Registration)}},
            }
            })),
            _ => Err("KBD api: unknown MutableBorrow ID"),
        }
    }
}
 */