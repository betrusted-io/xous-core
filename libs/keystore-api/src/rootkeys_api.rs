use core::mem::size_of;
use core::ops::{Deref, DerefMut};

pub use rkyv_enum::*;

use crate::rkyv_enum;
use crate::*;

#[allow(dead_code)]
pub const SERVER_NAME_KEYS: &str = "_Root key server and update manager_";
#[allow(dead_code)]
pub const SIG_LOADER_VERSION: u32 = 1; // standard ed25519 signature
#[allow(dead_code)]
pub const SIG_KERNEL_VERSION: u32 = 2; // ed25519ph signature

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
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
    /// create new FPGA keys; provisioning requires a slave device to be connected that can run the JTAG
    /// sequence
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

    /// Initiates the efuse burn process. Any user abort for the flow must be done
    /// prior to this call; the flow within rootkeys is intended to be non-abortable.
    #[cfg(feature = "efuse")]
    BurnEfuse = 48,
    #[cfg(feature = "efuse")]
    EfuseRun = 49,
    #[cfg(feature = "efuse")]
    EfusePasswordReturn = 50,
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
#[cfg_attr(not(any(feature = "precursor", feature = "renode")), allow(dead_code))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RootkeyResult {
    AlignmentError = 0,
    KeyError = 1,
    IntegrityError = 2,
    FlashError = 3,
    /// enclave is in the wrong state to do the requested operation
    StateError = 4,
}

// ---------------- BACKUPS -----------------
use keyboard::KeyMap;

// the BackupHeader type is serialized into u8 before going through rkyv.
// a bit inefficient but convenient, because we need an Option<> of the
// BackupHeader and not the header itself.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BackupHeaderIpc {
    pub data: Option<[u8; core::mem::size_of::<BackupHeader>()]>,
    pub checksums: Option<Checksums>,
}
impl Default for BackupHeaderIpc {
    fn default() -> Self {
        BackupHeaderIpc { data: None::<[u8; core::mem::size_of::<BackupHeader>()]>, checksums: None }
    }
}

pub const BACKUP_VERSION: u32 = 0x00_01_00_01;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub const BACKUP_VERSION_MASK: u32 = 0xFF_FF_00_00; // mask off bits that are cross-compatible

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
        match locales::LANG {
            "en" => BackupLanguage::En,
            "en-tts" => BackupLanguage::EnTts,
            "ja" => BackupLanguage::Ja,
            "zh" => BackupLanguage::Zh,
            _ => BackupLanguage::En,
        }
    }
}
impl From<BackupLanguage> for [u8; 4] {
    fn from(l: BackupLanguage) -> [u8; 4] { (l as u32).to_le_bytes() }
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
impl From<KeyMap> for BackupKeyboardLayout {
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
impl From<BackupKeyboardLayout> for KeyMap {
    fn from(layout: BackupKeyboardLayout) -> KeyMap {
        match layout {
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
    fn default() -> Self { BackupKeyboardLayout::Qwerty }
}
impl From<BackupKeyboardLayout> for [u8; 4] {
    fn from(l: BackupKeyboardLayout) -> [u8; 4] { (l as u32).to_le_bytes() }
}
impl From<[u8; 4]> for BackupKeyboardLayout {
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

pub const HEADER_TOTAL_SIZE: u32 = 4096;
/// Size of a checksummed block in pages. 0x100 = 256 pages,
/// or 1 MiB for a checksummed block. This is specified in 4kiB pages
/// because it really doesn't make sense to checksum anything smaller
/// than that, and it allows us to grow the size of a single checksummed
/// block to well over 4GiB.
pub const CHECKSUM_BLOCKLEN_PAGE: u32 = 0x100;
/// The total number of checksums required to cover the length of the PDDB
/// divided by the length of a checksummed page. This number does not count
/// the single additional checksum that is included to check the integrity of the
/// plaintext + ciphertext header itself.
///
/// Total number of checksums has to divide evenly into the size of the PDDB region
pub const TOTAL_CHECKSUMS: u32 = xous::PDDB_LEN / (CHECKSUM_BLOCKLEN_PAGE * 4096);

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
    /// Length of a checksummed region, in 4kiB pages
    pub checksum_len_page: [u8; 4],
    /// Number of checksummed pages
    ///
    /// The location of the first header is computed as follows:
    /// `header_start_address + header_total_size - (total_checksums) * 16`
    /// The backup data starts at `header_start_address + header_total_size`
    ///
    /// In other words, checksums are aligned to the highest address in the header
    /// while the header plaintext+ciphertext data is aligned to the lowest address
    /// in the header region.
    pub total_checksums: [u8; 4],
    /// Total size of the header in bytes, including unused space
    pub header_total_size: [u8; 4],
    pub _reserved: [u8; 36],
    pub op: BackupOp,
}
impl Default for BackupHeader {
    fn default() -> Self {
        assert!(
            xous::PDDB_LEN & ((CHECKSUM_BLOCKLEN_PAGE * 0x1000) - 1) == 0,
            "PDDB_LEN is not an integer multiple of CHECKSUM_LEN_PAGE"
        );
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
            checksum_len_page: CHECKSUM_BLOCKLEN_PAGE.to_le_bytes(),
            total_checksums: TOTAL_CHECKSUMS.to_le_bytes(),
            header_total_size: HEADER_TOTAL_SIZE.to_le_bytes(),
            _reserved: [0u8; 36],
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
