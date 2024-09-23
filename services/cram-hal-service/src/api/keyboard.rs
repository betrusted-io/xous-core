use std::fmt::Display;

pub const SERVER_NAME_KBD: &str = "_Matrix keyboard driver_";

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
pub(crate) enum KeyboardOpcode {
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

// this structure is used to register a keyboard listener. Currently, we only accept
// one trusted listener (enforced by name server and structurally in the code),
// which is the GAM.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub(crate) struct KeyboardRegistration {
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
