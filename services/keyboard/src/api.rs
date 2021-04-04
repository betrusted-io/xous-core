pub const SERVER_NAME_KBD: &str      = "_Matrix keyboard driver_";

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

// maximum number of key events to track simultaneously.
pub const MAX_KEYS: usize = 16;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default, Copy, Clone, PartialEq, Eq)]
pub struct RowCol {
    pub r: u8,
    pub c: u8,
}
/// RowColVec is implemented here instead of using heapless::Vec because we can't
/// derive the rkyv traits on heapless::Vec. By making a janky vector type here
/// our IPC doesn't rely on dangerous serialization techniques like casting to
/// raw u8 slices which rely on the implicit shape of packed structures...
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct RowColVec {
    storage: [Option<RowCol>; MAX_KEYS],
    iter: usize,
}
impl RowColVec {
    pub fn new() -> Self {
        RowColVec {
            storage: [None; MAX_KEYS],
            iter: 0
        }
    }
    pub fn len(&self) -> usize {
        return MAX_KEYS
    }
    pub fn get(&self, i: usize) -> Option<RowCol> {
        if i < MAX_KEYS {
            self.storage[i]
        } else {
            None
        }
    }
    pub fn set(&mut self, i: usize, data: Option<RowCol>) {
        if i < MAX_KEYS {
            self.storage[i]= data;
        }
    }
    // used by the interrupt handler, we can't deal with errors anyways
    pub fn add_unchecked(&mut self, key: RowCol) {
        for s in self.storage.iter_mut() {
            if *s == None {
                *s = Some(key);
                break;
            }
        }
    }
    // returns True if key is unique and added; False if already exists in storage
    pub fn add_rc(&mut self, key: RowCol) -> Result<bool, xous::Error> {
        // first, check if the rc is in the array
        for &s in self.storage.iter() {
            if let Some(rc) = s {
                if rc == key {
                    return Ok(false)
                }
            }
        }
        // if we got here, rc was not in the arary
        let mut added = false;
        for s in self.storage.iter_mut() {
            if *s == None {
                *s = Some(key);
                added = true;
                break;
            }
        }
        if added {
            Ok(true)
        } else {
            Err(xous::Error::OutOfMemory)
        }
    }
    // returns True if the key was removed; false if the key did not exist already
    pub fn remove_rc(&mut self, key: RowCol) -> bool {
        for s in self.storage.iter_mut() {
            if let Some(rc) = s {
                if *rc == key {
                    *s = None;
                    return true
                }
            }
        }
        return false
    }
    pub fn contains(&self, key: RowCol) -> bool {
        for s in self.storage.iter() {
            if let Some(rc) = s {
                if *rc == key {
                    return true
                }
            }
        }
        false
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct KeyRawStates {
    pub keydowns: RowColVec,
    pub keyups: RowColVec,
}
impl KeyRawStates {
    pub fn new() -> Self {
        KeyRawStates {
            keydowns: RowColVec::new(),
            keyups: RowColVec::new(),
        }
    }
}

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

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// set which keyboard mapping is present
    SelectKeyMap, //(KeyMap),

    /// request interpreted ScanCodes to be sent
    RegisterListener, //(String::<64>),

    /// request raw keyup/keydown events to be sent
    RegisterRawListener, //(String::<64>),

    /// set repeat delay, rate; both in ms
    SetRepeat, //(u32, u32),

    /// set chording interval (how long to wait for all keydowns to happen before interpreting as a chord), in ms (for braille keyboards)
    SetChordInterval, //(u32),

    /// used by host mode emulation to inject keys
    HostModeInjectKey, //(char),

    /// used by the interrupt handler to transfer results to the main loop
    HandlerTrigger,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Callback {
    KeyEvent,
    KeyRawEvent,
    Drop,
}

