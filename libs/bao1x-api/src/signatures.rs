use bytemuck::{Pod, Zeroable};
pub const SIGBLOCK_LEN: usize = 768; // this is adjusted inside builder.rs, in the sign-image invocation
// The jump instruction and the signature itself are not protected
pub const UNSIGNED_LEN: usize = size_of::<u32>() + SIGNATURE_LENGTH;

// These are vendored in so we don't have a circular dependency on ed25519 crate
pub const SIGNATURE_LENGTH: usize = 64; // length of an ed25519 signature.
pub const PUBLIC_KEY_LENGTH: usize = 32; // length of an ed25519 public key.
/// These are notional and subject to change
#[repr(u32)]
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum FunctionCode {
    /// Should never be used
    Invalid = 0,
    /// Code for a valid boot0 region
    Boot0 = 1,
    /// Code for a valid boot1 region
    Boot1 = 2,
    /// Expected code when being passed a boot1 region update. Self-signing/sealing implementations
    /// can change this code to Boot1; externally signed implementations would accept UpdatedBoot1
    /// as interchangeable with Boot1
    UpdatedBoot1 = 3,
    /// Loader
    Loader = 4,
    UpdatedLoader = 5,
    /// Baremetal options
    Baremetal = 6,
    UpdatedBaremetal = 7,
    /// Kernel region
    Kernel = 0x1_00,
    UpdatedKernel = 0x1_01,
    /// Application region
    App = 0x10_0000,
    UpdatedApp = 0x10_0001,
    /// Developer use - can go in any region, but only usable with secrets wiped.
    Developer = 0x1000_0000,
}

pub const BAOCHIP_SIG_VERSION: u32 = 0x1_00;
pub const PADDING_LEN: usize = SIGBLOCK_LEN - size_of::<SealedFields>() - SIGNATURE_LENGTH - size_of::<u32>();
/// Representation of the signature block in memory.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SignatureInFlash {
    /// This is a field that contains a jump instruction such that the CPU boot passes over
    /// this structure on the way to the boot code.
    pub _jal_instruction: u32,
    /// The actual signature.
    pub signature: [u8; SIGNATURE_LENGTH],
    /// All data from this point onward are included in the signature computation.
    pub sealed_data: SealedFields,
    /// Padding to target length
    pub padding: [u8; PADDING_LEN],
}
unsafe impl Zeroable for SignatureInFlash {}
unsafe impl Pod for SignatureInFlash {}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SealedFields {
    /// Version number of this signature record
    pub version: u32,
    /// Length of the signed code.
    pub signed_len: u32,
    /// Function code of the signed block. Used to provide metadata about what is inside the signed
    /// block. See `FunctionCode`.
    pub function_code: u32,
    /// Reserved for future use
    pub reserved: u32,
    /// Minimum version of target code that can process this update. This allows us to break
    /// compatibility with extremely old code bases, should we need to.
    pub min_semver: [u8; 16],
    /// Version of the Xous tree in Semver format that was used to build the signed artifact
    pub semver: [u8; 16],
    /// The public keys used to check the signature. In the case of boot0, this is committed to a write-only
    /// partition, so it is immutable. It is also used to check the boot1 block.
    ///
    /// An array of keys are provided. An all-0 key is to be disregarded. Furthermore, one-way counters
    /// in slots 124-127 correspond to keys 0-3 here. A key is to be considered revoked/disregarded
    /// if the one-way counter does not have a 0 value.
    ///
    /// Valid keys are checked in ascending slot order. Any valid signature is considered to be allowed for
    /// boot.
    ///
    /// Key slot 3 is reserved for the developer public key. It is the last key checked, and if it is the
    /// only valid key and it is not revoked with the one-way counter mechanism, the device root secret
    /// is erased and boot is allowed to proceed.
    ///
    /// If no valid keys are found, the device is effectively bricked and goes into a "die" state.
    pub pubkeys: [[u8; PUBLIC_KEY_LENGTH]; 4],
}

impl AsRef<[u8]> for SealedFields {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
