use bytemuck::{Pod, Zeroable};

use crate::*;

/// Total reserved space for the signature block
pub const SIGBLOCK_LEN: usize = 768;
/// The jump instruction and the signature itself are not protected
pub const UNSIGNED_LEN: usize = SignatureInFlash::sealed_data_offset();

// These are vendored in so we don't have a circular dependency on ed25519 crate
pub const SIGNATURE_LENGTH: usize = 64; // length of an ed25519 signature.
pub const PUBLIC_KEY_LENGTH: usize = 32; // length of an ed25519 public key.
pub const AAD_LENGTH: usize = 60; // at least 37 bytes, rounded up to the nearest 32-byte boundary

pub const PUBLIC_KEY_PQ_LENGTH: usize = 32; // length of a slh-dsa-128-24 key
// the signature is so large, it has to be post-pended to the firmware blob. Thus the position
// of the signature varies from run to run
pub const SIGNATURE_PQ_LENGTH: usize = 3856; // length of a slh-dsa-12824 signature

/// These are notional and subject to change
#[repr(u32)]
#[derive(num_enum::TryFromPrimitive, PartialEq, Eq, Clone, Copy)]
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
}

impl FunctionCode {
    pub fn to_anti_rollback_counter(&self) -> Option<usize> {
        match self {
            Self::Boot0 => Some(BOOT0_ANTI_ROLLBACK),
            Self::Boot1 | Self::UpdatedBoot1 => Some(BOOT1_ANTI_ROLLBACK),
            Self::Loader | Self::UpdatedLoader => Some(LOADER_ANTI_ROLLBACK),
            Self::Kernel | Self::UpdatedKernel => Some(KERNEL_ANTI_ROLLBACK),
            Self::Swap | Self::UpdatedSwap => Some(SWAP_ANTI_ROLLBACK),
            Self::App | Self::UpdatedApp => Some(APP_ANTI_ROLLBACK),
            Self::Baremetal | Self::UpdatedBaremetal => Some(BAREMETAL_ANTI_ROLLBACK),
            _ => None,
        }
    }
}

/// Due to an implementation bug, this is effectively a fixed number
pub const BAOCHIP_SIG_VERSION: u32 = 0x1_00;

/// This logic allows us to have a meaningful versioning
pub const BAOCHIP_SIG_MAJ_VER: u8 = 0x01;
pub const BAOCHIP_SIG_MIN_VER: u8 = 0x00;
pub const BAOCHIP_SIG_REV_VER: u16 = 0x0000;
pub const fn baochip_sig_version() -> u32 {
    BAOCHIP_SIG_REV_VER as u32 | (BAOCHIP_SIG_MIN_VER as u32) << 16 | (BAOCHIP_SIG_MAJ_VER as u32) << 24
}
pub const fn baochip_sig_compat(incoming: u32) -> bool {
    if (incoming >> 24) as u8 == BAOCHIP_SIG_MAJ_VER {
        BAOCHIP_SIG_MIN_VER > (incoming >> 16) as u8
            || BAOCHIP_SIG_MIN_VER == (incoming >> 16) as u8 && (BAOCHIP_SIG_REV_VER >= incoming as u16)
    } else {
        false
    }
}

pub const PADDING_LEN: usize = SIGBLOCK_LEN - size_of::<SignatureInFlash>();
pub const MAGIC_NUMBER: [u32; 2] = [u32::from_be_bytes(*b"yumy"), u32::from_be_bytes(*b"Bao3")];
/// Representation of the signature block in memory.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SignatureInFlash {
    /// This is a field that contains a jump instruction such that the CPU boot passes over
    /// this structure on the way to the boot code.
    pub _jal_instruction: u32,
    /// The actual signature.
    pub signature: [u8; SIGNATURE_LENGTH],
    /// `aad_len` also specifies the signing protocol. If it is `0`, then a pure `ed25519ph`
    /// signature is assumed. If it is greater than `0`, then it's assumed to be a
    /// FIDO2/WebAuthn signature format using ed25519, where the signature is computed as:
    /// `signature = Ed25519.sign(authenticatorData || SHA-256(clientData))`
    /// `authenticatorData` is a field that is at least 37 bytes in size. The size is influenced
    /// by the type of signing and operation, but for our implementation I think it stays
    /// fixed at 37 bytes.
    pub aad_len: u32,
    pub aad: [u8; AAD_LENGTH],
    /// All data from this point onward are included in the signature computation.
    pub sealed_data: SealedFields,
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
            aad_len: 0,
            aad: [0u8; AAD_LENGTH],
            sealed_data: SealedFields::default(),
        }
    }
}
impl SignatureInFlash {
    /// The offset is after the jal instruction + signature.
    pub const fn sealed_data_offset() -> usize {
        size_of::<u32>() + SIGNATURE_LENGTH + size_of::<u32>() + AAD_LENGTH
    }

    /// The end of the signed code
    pub fn sealed_data_end(&self) -> usize { self.sealed_data.signed_len as usize + UNSIGNED_LEN }

    /// Returns the address of this structure in memory.
    pub fn base_addr(&self) -> usize { self as *const Self as usize }

    /// Returns the PQ signature field
    /// Safety:
    ///   - Only safe on XIP signature records. Records in off-chip memory have to copy the field out.
    ///   - `base` must point to the actual XIP base in question
    pub unsafe fn pq_signature(&self, base: usize) -> &SignaturePqInFlash {
        let addr = base + self.sealed_data_end();

        // SAFETY: `addr` points to at least SIGNATURE_PQ_LENGTH readable, initialized
        // bytes for the lifetime of this reference (backed by RRAM, not concurrently
        // mutated). Alignment is satisfied since SignaturePqInFlash has an alignment of 1.
        let sig_struct: &SignaturePqInFlash = unsafe { &*(addr as *const SignaturePqInFlash) };
        sig_struct
    }

    /// Offset for retrieving signature from off-chip memory implementations
    pub fn pq_signature_offset(&self) -> usize { self.sealed_data_end() }

    /// Returns `true` if this record is compatible with the current code
    pub fn is_compatible(&self) -> bool {
        // corrected_version == 0x0 is the legacy code. We can't break compatibility
        // with these records under any circumstances, unfortunately. However, if corrected_version
        // is not 0x0, we can increment the maj/min/rev fields and have a meaningful version in the
        // signature records.
        self.sealed_data.version == BAOCHIP_SIG_VERSION
            && (self.sealed_data.corrected_version == 0x0
                || baochip_sig_compat(self.sealed_data.corrected_version))
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SignaturePqInFlash {
    pub signature: [u8; SIGNATURE_PQ_LENGTH],
}
unsafe impl Zeroable for SignaturePqInFlash {}
unsafe impl Pod for SignaturePqInFlash {}
impl AsRef<[u8]> for SignaturePqInFlash {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
impl AsMut<[u8]> for SignaturePqInFlash {
    fn as_mut(&mut self) -> &mut [u8] { bytemuck::bytes_of_mut(self) }
}
impl Default for SignaturePqInFlash {
    fn default() -> Self { Self { signature: [0u8; SIGNATURE_PQ_LENGTH] } }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Pubkey {
    pub pk: [u8; PUBLIC_KEY_LENGTH],
    pub tag: [u8; 4],
}
unsafe impl Zeroable for Pubkey {}
unsafe impl Pod for Pubkey {}
impl Default for Pubkey {
    fn default() -> Self { Self { pk: [0u8; PUBLIC_KEY_LENGTH], tag: [0u8; 4] } }
}
impl Pubkey {
    pub fn populate_from(&mut self, record: &Pubkey) {
        self.pk.copy_from_slice(&record.pk);
        self.tag.copy_from_slice(&record.tag);
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PubkeyPq {
    pub pk: [u8; PUBLIC_KEY_PQ_LENGTH],
    pub tag: [u8; 4],
}
unsafe impl Zeroable for PubkeyPq {}
unsafe impl Pod for PubkeyPq {}
impl Default for PubkeyPq {
    fn default() -> Self { Self { pk: [0u8; PUBLIC_KEY_PQ_LENGTH], tag: [0u8; 4] } }
}
impl PubkeyPq {
    pub fn populate_from(&mut self, record: &PubkeyPq) {
        self.pk.copy_from_slice(&record.pk);
        self.tag.copy_from_slice(&record.tag);
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SealedFields {
    /// Version number of this signature record
    pub version: u32,
    /// Magic number, to quickly differentiate uninitialized memory from a valid signature record
    pub magic: [u32; 2],
    /// Length of the signed code.
    pub signed_len: u32,
    /// Function code of the signed block. Used to provide metadata about what is inside the signed
    /// block. See `FunctionCode`.
    pub function_code: u32,
    /// Anti-rollback version number. This is matched against a one-way counter stored in the
    /// one-way counter memory bank.
    pub anti_rollback: u32,
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
    /// Placeholder for baobit commit: this is the hash of the toolchain used to generate the image.
    /// In many images, this is just 0's; but for signed release images, this is injected into the file
    /// prior to applying the final signature. This injection has to happen after everything is done
    /// because the rules of the reproducible build system disallow it from knowing its final commit
    /// state: you have to commit its final state, which changes its reported commit state, and so
    /// forth, so at build time you can't know what toolchain was used to build you - only after
    /// the build is complete is the information revealed to the user.
    ///
    /// This field is long enough to hold a SHA-1 git hash.
    pub toolchain: [u8; 20],
    // ==== this is the end of non-pq signatures verion 01_00 ====
    /// The version field is repeated here, because the original version code check was buggy in that
    /// any version other than the one exactly specified would cause an incompatibility problem. We can't
    /// fix that my changing its version - it's now effectively fixed or we brick devices. This
    /// `corrected_version` field now specifies the actual version of the signature here
    pub corrected_version: u32,
    /// This is *guaranteed* to be 0 in non-PQ versions because this is zero-padded by the image generator.
    /// Thus a value of `0` here means there is no PQ signature.
    pub pq_enabled: u32,
    /// Similar in structure to the `pubkeys` field. These are similarly immutable but there isn't
    /// the backup copy kept in the IFR, as we're running out of space there.
    pub pubkeys_pq: [PubkeyPq; 4],
}

impl AsRef<[u8]> for SealedFields {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}
impl Default for SealedFields {
    fn default() -> Self {
        Self {
            version: BAOCHIP_SIG_VERSION,
            magic: MAGIC_NUMBER,
            signed_len: 0,
            function_code: 0,
            anti_rollback: 0,
            min_semver: [0u8; 16],
            semver: [0u8; 16],
            pubkeys: [Pubkey::default(); 4],
            toolchain: [0u8; 20],
            corrected_version: baochip_sig_version(),
            pq_enabled: 0,
            pubkeys_pq: [PubkeyPq::default(); 4],
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
    /// This field is deprecated. It is ignored but retained so that the record shape
    /// remains backward compatible
    pub key: [u8; 32],
    pub flash_offset: u32,
}
impl AsRef<[u8]> for SwapDescriptor {
    fn as_ref(&self) -> &[u8] { bytemuck::bytes_of(self) }
}

pub const SWAP_VERSION: u32 = 0x01_01_0000;
