use bytemuck::{Pod, Zeroable};

/// Total reserved space for the signature block
pub const SIGBLOCK_LEN: usize = 768;
/// The jump instruction and the signature itself are not protected
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
    /// Swap region
    Swap = 0x80_00,
    UpdatedSwap = 0x80_01,
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
impl AsRef<[u8]> for SignatureInFlash {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
impl AsMut<[u8]> for SignatureInFlash {
    fn as_mut(&mut self) -> &mut [u8] { bytemuck::bytes_of_mut(self) }
}
impl Default for SignatureInFlash {
    fn default() -> Self {
        Self {
            _jal_instruction: 0,
            signature: [0u8; SIGNATURE_LENGTH],
            sealed_data: SealedFields::default(),
            padding: [0u8; PADDING_LEN],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Pubkey {
    pub pk: [u8; PUBLIC_KEY_LENGTH],
    pub tag: [u8; 4],
    /// `aad_len` also specifies the signing protocol. If it is `0`, then a pure `ed25519ph`
    /// signature is assumed. If it is greater than `0`, then it's assumed to be a
    /// FIDO2/WebAuthn signature format using ed25519, where the signature is computed as:
    /// `signature = Ed25519.sign(authenticatorData || SHA-256(clientData))`
    /// `authenticatorData` is a field that is at least 37 bytes in size, and can be much
    /// larger if optional extentions are enabled. 59 bytes are made available since this
    /// pads nicely into the record format, and gives us some wiggle room for compatibility
    pub aad_len: u8,
    pub aad: [u8; 59],
}
unsafe impl Zeroable for Pubkey {}
unsafe impl Pod for Pubkey {}
impl Default for Pubkey {
    fn default() -> Self { Self { pk: [0u8; PUBLIC_KEY_LENGTH], tag: [0u8; 4], aad_len: 0, aad: [0u8; 59] } }
}
impl Pubkey {
    pub fn populate_from(&mut self, record: &Pubkey) {
        self.pk.copy_from_slice(&record.pk);
        self.tag.copy_from_slice(&record.tag);
        self.aad_len = record.aad_len;
        self.aad.copy_from_slice(&record.aad);
    }
}

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
    pub pubkeys: [Pubkey; 4],
}

impl AsRef<[u8]> for SealedFields {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
impl Default for SealedFields {
    fn default() -> Self {
        Self {
            version: 0,
            signed_len: 0,
            function_code: 0,
            reserved: 0,
            min_semver: [0u8; 16],
            semver: [0u8; 16],
            pubkeys: [Pubkey::default(); 4],
        }
    }
}

#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct SwapSourceHeader {
    pub version: u32,
    pub partial_nonce: [u8; 8],
    pub mac_offset: u32,
    pub aad_len: u32,
    // aad is limited to 64 bytes!
    pub aad: [u8; 64],
}
impl AsRef<[u8]> for SwapSourceHeader {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
impl Default for SwapSourceHeader {
    fn default() -> Self {
        Self { version: 0, partial_nonce: [0; 8], mac_offset: 0, aad_len: 0, aad: [0; 64] }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Default)]
pub struct SwapDescriptor {
    pub ram_offset: u32,
    pub ram_size: u32,
    pub name: u32,
    pub key: [u8; 32],
    pub flash_offset: u32,
}
impl AsRef<[u8]> for SwapDescriptor {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
