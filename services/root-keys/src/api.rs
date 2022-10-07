use core::ops::{Deref, DerefMut};
use core::mem::size_of;

pub(crate) const SERVER_NAME_KEYS: &str     = "_Root key server and update manager_";
#[allow(dead_code)]
pub(crate) const SIG_VERSION: u32 = 1;

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// use to check if we've been initialized
    KeysInitialized = 0,
    /// check that the digital signature on the gateware
    CheckGatewareSignature = 1,
    /// check if the efuse has been locked down
    IsEfuseSecured = 2,
    /// quick check to see if the JTAG can read its IDCODE
    IsJtagWorking = 3,
    /// initiate an AES oracle operation
    AesOracle = 4,
    /// initiate key wrapper operation
    AesKwp = 5,
    /// create new FPGA keys; provisioning requires a slave device to be connected that can run the JTAG sequence
    BbramProvision = 6,
    /// clear a cached password
    ClearPasswordCacheEntry = 7,

    TestUx = 8,

    /// attempt to initialize keys on a brand new system. Does nothing if the keys are already provisioned.
    UxTryInitKeys = 9,
    UxInitUpdateFirstPasswordReturn = 10,
    UxInitUpdatePasswordReturn = 11,

    /// provision a gateware update with our secret data
    UxUpdateGateware = 12,
    UxUpdateGwPasswordReturn = 13,
    UxUpdateGwRun = 14,

    /// self-sign kernel/loader
    UxSelfSignXous = 15,
    UxSignXousPasswordReturn = 16,
    UxSignXousRun = 17,

    /// Ux AES calls
    UxAesEnsurePassword = 18,
    UxAesPasswordPolicy = 19,
    UxAesEnsureReturn = 20,

    /// Ux BBRAM flow
    UxBbramCheckReturn = 21,
    UxBbramPasswordReturn = 22,
    UxBbramRun = 23,

    // General Ux calls
    UxGutter = 24, // NOP for UX calls that require a destination
    #[cfg(feature = "policy-menu")]
    UxGetPolicy = 25,
    #[cfg(feature = "policy-menu")]
    UxPolicyReturn = 26,
    UxTryReboot = 27,
    UxDoReboot = 28,

    /// UX opcodes
    ModalRedraw = 29,
    ModalKeys = 30,
    ModalDrop = 31,

    /// Suspend/resume callback
    SuspendResume = 32,

    Quit = 33,

    /// makes a "blind copy" of the staged gateware. This will destroy root keys,
    /// but the call will not succeed if the image was provisioned. This has
    /// an explicit user approval step.
    UxBlindCopy = 34,
    /// report the semver of the staged gateware
    StagedSemver = 35,
    /// A zero-touch version of UxBlindCopy.
    /// Run an update of a staged gateware that is newer. Will silently fail if
    /// root keys exist, or if the staged gateware is invalid.
    TryNoKeySocUpdate = 36,
    /// Query if the "don't bother me again for an update" flag is set
    ShouldPromptForUpdate = 37,
    /// Set the "don't bother me again for an update"
    SetPromptForUpdate = 38,
    /// Create a backup block (including UX flow that discloses our AES key!)
    CreateBackup = 39,
    UxCreateBackupPwReturn = 40,
    /// Query if a backup exists to be restored
    ShouldRestore = 41,
    /// Perform the restore operation (including UX to acquire the AES key)
    DoRestore = 42,
    UxDoRestorePwReturn = 43,
    /// A check to see if the zero-key was used on boot
    IsZeroKey = 44,
    /// Erase the backup block
    EraseBackupBlock = 45,
    /// Checks to see if "don't ask me about updates" is set
    IsDontAskSet = 46,
    /// Resets the dont ask bit. Mainly for use by the OQC testing routine
    ResetDontAsk = 47,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq)]
pub enum PasswordRetentionPolicy {
    AlwaysKeep = 0,
    EraseOnSuspend = 1,
    AlwaysPurge = 2,
}

/// Enumerate the possible password types dealt with by this manager.
/// Note that the discriminant of the enum is used to every-so-slightly change the salt going into bcrypt
/// I don't think it hurts; more importantly, it also prevents an off-the-shelf "hashcat" run from
/// being used to brute force both passwords in a single go, as the salt has to be (slightly)
/// recomputed for each type of password.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PasswordType {
    Boot = 1,
    Update = 2,
}
#[cfg_attr(not(any(feature="precursor", feature="renode")), allow(dead_code))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RootkeyResult {
    AlignmentError = 0,
    KeyError = 1,
    IntegrityError = 2,
    FlashError = 3,
    /// enclave is in the wrong state to do the requested operation
    StateError = 4,
}

/// AES operation definitions
pub use cipher::{BlockCipher, consts::U16};
use keyboard::KeyMap;
use zeroize::Zeroize;

/// 128-bit AES block
#[allow(dead_code)]
pub type Block = cipher::generic_array::GenericArray<u8, cipher::consts::U16>;
/// 16 x 128-bit AES blocks to be processed in bulk
#[allow(dead_code)]
pub type ParBlocks = cipher::generic_array::GenericArray<Block, cipher::consts::U16>;

pub const PAR_BLOCKS: usize = 16;
/// Selects which key to use for the decryption/encryption oracle.
/// currently only one type is available, the User key, but dozens more
/// could be accommodated.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq, Copy, Clone)]
pub enum AesRootkeyType {
    User0 = 0x28,
    NoneSpecified = 0xff,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
pub enum AesBlockType {
    SingleBlock([u8; 16]),
    ParBlock([[u8; 16]; PAR_BLOCKS]),
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
pub enum AesOpType {
    Encrypt = 0,
    Decrypt = 1,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct AesOp {
    /// the caller can try to request "any" index, but it's checked inside the oracle first.
    pub key_index: u8,
    pub block: AesBlockType,
    pub aes_op: AesOpType,
}
impl AesOp {
    pub fn clear(&mut self) {
        match self.block {
            AesBlockType::SingleBlock(mut blk) => {
                for b in blk.iter_mut() {
                    *b = 0;
                }
            }
            AesBlockType::ParBlock(mut blks) => {
                for blk in blks.iter_mut() {
                    for b in blk.iter_mut() {
                        *b = 0;
                    }
                }
            }
        }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq, Copy, Clone)]
pub enum KeywrapError {
    InvalidDataSize,
    InvalidKekSize,
    InvalidOutputSize,
    IntegrityCheckFailed,
    /// this is a bodge to return an error code that upgrades from a faulty early version of AES-KWP
    /// only works for 256-bit keys, but that is also all we used.
    /// The return tuple is: (unwrapped key, correctly wrapped key)
    UpgradeToNew(([u8; 32], [u8; 40])),
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq)]
pub enum KeyWrapOp {
    Wrap = 0,
    Unwrap = 1,
}

use std::error::Error;
impl Error for KeywrapError {}

use std::fmt;

impl fmt::Display for KeywrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            KeywrapError::InvalidDataSize => f.write_str("Invalid data size"),
            KeywrapError::InvalidKekSize => f.write_str("Invalid key size"),
            KeywrapError::InvalidOutputSize => f.write_str("Invalid output size"),
            KeywrapError::IntegrityCheckFailed => f.write_str("Authentication failed"),
            KeywrapError::UpgradeToNew((_k, _wk)) => f.write_str("Legacy migration detected! New wrapped key transmitted to caller"),
        }
    }
}

pub(crate) const MAX_WRAP_DATA: usize = 2048;
/// Note regression in v0.9.9: we had to return an array type in the KeywrapError enum that
/// has a signature for an array that is 40 bytes long, which is bigger than Rust's devire
/// can deal with. So, unfortunately, the result of this does *not* get zeroized on drop :(
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
// #[zeroize(drop)]
pub (crate) struct KeyWrapper {
    pub data: [u8; MAX_WRAP_DATA + 8],
    // used to specify the length of the data used in the fixed-length array above
    pub len: u32,
    pub key_index: u8,
    pub op: KeyWrapOp,
    pub result: Option<KeywrapError>,
    // used by the unwrap side
    pub expected_len: u32,
}

// the BackupHeader type is serialized into u8 before going through rkyv.
// a bit inefficient but convenient, because we need an Option<> of the
// BackupHeader and not the header itself.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub (crate) struct BackupHeaderIpc {
    pub data: Option<[u8; core::mem::size_of::<BackupHeader>()]>,
}
impl Default for BackupHeaderIpc {
    fn default() -> Self {
        BackupHeaderIpc { data: None::<[u8; core::mem::size_of::<BackupHeader>()]> }
    }
}

pub const BACKUP_VERSION: u32 = 0x00_01_00_00;

#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum BackupOp {
    /// This is the value that's kept inside the BackupDataPt
    Archive = 0,
    /// backup and restore can be manipulated by the OS without updating the ciphertext
    Backup = 1,
    Restore = 2,
    RestoreDna = 3,
}

#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum BackupLanguage {
    En = 0,
    EnTts = 1,
    Ja = 2,
    Zh = 3,
}
impl Default for BackupLanguage {
    fn default() -> Self {
        match xous::LANG {
            "en" => BackupLanguage::En,
            "en-tts" => BackupLanguage::EnTts,
            "ja" => BackupLanguage::Ja,
            "zh" => BackupLanguage::Zh,
            _ => BackupLanguage::En,
        }
    }
}
impl From::<BackupLanguage> for [u8; 4] {
    fn from(l: BackupLanguage) -> [u8; 4] {
        (l as u32).to_le_bytes()
    }
}
#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
/// We keep a separate version of this for serialization/deserialization because
/// we need to handle "unknown/undefined" layouts in a different way from the keyboard
/// crate. Plus this helps avoid a dependency on the keyboard crate as well.
pub enum BackupKeyboardLayout {
    Qwerty = 0,
    Dvorak = 1,
    Qwertz = 2,
    Azerty = 3,
    Braille = 4,
    Hangul = 5,
    // codes above 16384 are reserved for user layouts
}
impl From::<KeyMap> for BackupKeyboardLayout {
    fn from(map: KeyMap) -> BackupKeyboardLayout {
        match map {
            KeyMap::Qwerty => BackupKeyboardLayout::Qwerty,
            KeyMap::Azerty => BackupKeyboardLayout::Azerty,
            KeyMap::Dvorak => BackupKeyboardLayout::Dvorak,
            KeyMap::Qwertz => BackupKeyboardLayout::Qwertz,
            KeyMap::Braille => BackupKeyboardLayout::Braille,
            KeyMap::Undefined => BackupKeyboardLayout::Qwerty,
        }
    }
}
impl Into::<KeyMap> for BackupKeyboardLayout {
    fn into(self) -> KeyMap {
        match self {
            BackupKeyboardLayout::Qwerty => KeyMap::Qwerty,
            BackupKeyboardLayout::Braille => KeyMap::Braille,
            BackupKeyboardLayout::Dvorak => KeyMap::Dvorak,
            BackupKeyboardLayout::Qwertz => KeyMap::Qwertz,
            BackupKeyboardLayout::Azerty => KeyMap::Azerty,
            BackupKeyboardLayout::Hangul => KeyMap::Undefined,
        }
    }
}
impl Default for BackupKeyboardLayout {
    fn default() -> Self {
        BackupKeyboardLayout::Qwerty
    }
}
impl From::<BackupKeyboardLayout> for [u8; 4] {
    fn from(l: BackupKeyboardLayout) -> [u8; 4] {
        (l as u32).to_le_bytes()
    }
}
impl From::<[u8; 4]> for BackupKeyboardLayout {
    fn from(b: [u8; 4]) -> BackupKeyboardLayout {
        let code = u32::from_le_bytes(b);
        match code {
            0 => BackupKeyboardLayout::Qwerty,
            1 => BackupKeyboardLayout::Dvorak,
            2 => BackupKeyboardLayout::Qwertz,
            3 => BackupKeyboardLayout::Azerty,
            4 => BackupKeyboardLayout::Braille,
            5 => BackupKeyboardLayout::Hangul,
            _ => BackupKeyboardLayout::Qwerty,
        }
    }
}

#[repr(C, align(8))]
#[derive(Copy, Clone, Debug)]
pub struct BackupHeader {
    pub version: u32,
    // the `ver`s are all serialized SemVers. To be done by the caller.
    pub xous_ver: [u8; 16],
    pub soc_ver: [u8; 16],
    pub ec_ver: [u8; 16],
    pub wf200_ver: [u8; 16],
    pub timestamp: u64,
    pub language: [u8; 4],
    pub kbd_layout: [u8; 4],
    pub dna: [u8; 8],
    pub _reserved: [u8; 48],
    pub op: BackupOp,
}
impl Default for BackupHeader {
    fn default() -> Self {
        BackupHeader {
            version: BACKUP_VERSION,
            xous_ver: [0u8; 16],
            soc_ver: [0u8; 16],
            ec_ver: [0u8; 16],
            wf200_ver: [0u8; 16],
            timestamp: 0,
            language: BackupLanguage::default().into(), // this is "correct by default"
            kbd_layout: BackupKeyboardLayout::default().into(), // this has to be adjusted
            dna: [0u8; 8],
            _reserved: [0u8; 48],
            op: BackupOp::Archive,
        }
    }
}
impl Deref for BackupHeader {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const BackupHeader as *const u8, size_of::<BackupHeader>())
                as &[u8]
        }
    }
}
impl DerefMut for BackupHeader {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut BackupHeader as *mut u8, size_of::<BackupHeader>())
                as &mut [u8]
        }
    }
}