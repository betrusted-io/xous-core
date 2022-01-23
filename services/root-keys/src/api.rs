pub(crate) const SERVER_NAME_KEYS: &str     = "_Root key server and update manager_";
#[allow(dead_code)]
pub(crate) const SIG_VERSION: u32 = 1;

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// use to check if we've been initialized
    KeysInitialized,
    /// check that the digital signature on the gateware
    CheckGatewareSignature,
    /// check if the efuse has been locked down
    IsEfuseSecured,
    /// quick check to see if the JTAG can read its IDCODE
    IsJtagWorking,
    /// initiate an AES oracle operation
    AesOracle,
    /// create new FPGA keys; provisioning requires a slave device to be connected that can run the JTAG sequence
    BbramProvision,
    /// clear a cached password
    ClearPasswordCacheEntry,

    TestUx,

    /// attempt to initialize keys on a brand new system. Does nothing if the keys are already provisioned.
    UxTryInitKeys,
    UxInitBootPasswordReturn,
    UxInitUpdatePasswordReturn,

    /// provision a gateware update with our secret data
    UxUpdateGateware,
    UxUpdateGwPasswordReturn,
    UxUpdateGwRun,

    /// self-sign kernel/loader
    UxSelfSignXous,
    UxSignXousPasswordReturn,
    UxSignXousRun,

    /// Ux AES calls
    UxAesEnsurePassword,
    UxAesPasswordPolicy,
    UxAesEnsureReturn,

    /// Ux BBRAM flow
    UxBbramCheckReturn,
    UxBbramPasswordReturn,
    UxBbramRun,

    // General Ux calls
    UxGutter, // NOP for UX calls that require a destination
    #[cfg(feature = "policy-menu")]
    UxGetPolicy,
    #[cfg(feature = "policy-menu")]
    UxPolicyReturn,
    UxTryReboot,
    UxDoReboot,

    /// UX opcodes
    ModalRedraw,
    ModalKeys,
    ModalDrop,

    /// Suspend/resume callback
    SuspendResume,

    Quit
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq)]
pub enum PasswordRetentionPolicy {
    AlwaysKeep,
    EraseOnSuspend,
    AlwaysPurge,
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
    AlignmentError,
    KeyError,
    IntegrityError,
    FlashError,
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
    Encrypt,
    Decrypt,
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