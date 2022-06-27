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
    UxInitBootPasswordReturn = 10,
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
#[cfg_attr(not(any(target_os = "none", target_os = "xous")), allow(dead_code))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RootkeyResult {
    AlignmentError = 0,
    KeyError = 1,
    IntegrityError = 2,
    FlashError = 3,
}

/// AES operation definitions
pub use cipher::{BlockCipher, consts::U16};
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

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize, Eq, PartialEq, Copy, Clone)]
pub enum KeywrapError {
    /// Input is too big.
    TooBig = 0,
    /// Input is too small.
    TooSmall = 1,
    /// Ciphertext has invalid padding.
    Unpadded = 2,
    /// The ciphertext is not valid for the expected length.
    InvalidExpectedLen = 3,
    /// The ciphertext couldn't be authenticated.
    AuthenticationFailed = 4,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize, Eq, PartialEq)]
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
            KeywrapError::TooBig => f.write_str("Input too big"),
            KeywrapError::TooSmall => f.write_str("Input too small"),
            KeywrapError::Unpadded => f.write_str("Padding error"),
            KeywrapError::InvalidExpectedLen => f.write_str("Invalid expected lengthr"),
            KeywrapError::AuthenticationFailed => f.write_str("Authentication failed"),
        }
    }
}

pub(crate) const MAX_WRAP_DATA: usize = 2048;
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
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