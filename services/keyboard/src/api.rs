use core::ops::Deref;

use heapless::consts::*;
use heapless::Vec;
use xous::{Message, ScalarMessage};
use rkyv::{RelPtr, Archive, Resolve, Write};
use core::slice;
use core::ops::DerefMut;

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
    pub r: u32,
    pub c: u32,
}
#[repr(packed)]
pub struct KeyRawStates {
    pub keydowns: Vec<RowCol, U16>,
    pub keyups: Vec<RowCol, U16>,
}
impl KeyRawStates {
    pub fn new() -> Self {
        KeyRawStates {
            keydowns: Vec::new(),
            keyups: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn copy(&self) -> KeyRawStates {
        let mut krs = KeyRawStates::new();
        unsafe { // because KeyRawStates is *packed*
            for kd in self.keydowns.iter() {
                krs.keydowns.push(*kd).unwrap();
            }
            for ku in self.keyups.iter() {
                krs.keyups.push(*ku).unwrap();
            }
        }
        krs
    }
}
impl Clone for KeyRawStates {
    fn clone(&self) -> KeyRawStates {
        self.copy()
    }
}
impl Deref for KeyRawStates {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const KeyRawStates as *const u8, core::mem::size_of::<KeyRawStates>())
               as &[u8]
        }
    }
}
impl DerefMut for KeyRawStates {
    fn deref_mut(&mut self) -> &mut[u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut KeyRawStates as *mut u8, core::mem::size_of::<KeyRawStates>())
                as &mut [u8]
        }
    }
}

// warning: this rkyv code is totally untested
pub struct ArchivedKeyRawStates {
    ptr: RelPtr,
    len: u32,
}
#[allow(dead_code)]
impl ArchivedKeyRawStates {
    fn as_keyrawstates(&self) -> KeyRawStates {
        let mut returned_krs: KeyRawStates = KeyRawStates::new();
        let bytes: &[u8];
        unsafe {
            bytes = core::slice::from_raw_parts(self.ptr.as_ptr(), self.len as usize);
        }
        for (dest, src) in returned_krs.deref_mut().iter_mut().zip(bytes.iter()) {
            *dest = *src;
        }
        returned_krs
    }
}
pub struct KeyRawStatesResolver {
    bytes_pos: usize,
}
impl Resolve<KeyRawStates> for KeyRawStatesResolver {
    type Archived = ArchivedKeyRawStates;
    fn resolve(self, pos: usize, value: &KeyRawStates) -> Self::Archived {
        Self::Archived {
            ptr: unsafe {
                rkyv::RelPtr::new(pos + rkyv::offset_of!(ArchivedKeyRawStates, ptr), self.bytes_pos)
            },
            len: value.deref().len() as u32,
        }
    }
}
impl Archive for KeyRawStates {
    type Archived = ArchivedKeyRawStates;
    type Resolver = KeyRawStatesResolver;

    fn archive<W: Write + ?Sized>(&self, writer: &mut W) -> Result<Self::Resolver, W::Error> {
        let bytes_pos = writer.pos();
        writer.write( self.deref())?;
        Ok(Self::Resolver { bytes_pos })
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
