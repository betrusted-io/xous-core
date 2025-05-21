use core::ops::{Deref, DerefMut};

/// AES operation definitions
use zeroize::Zeroize;

use crate::TOTAL_CHECKSUMS;
pub use crate::rkyv_enum::*;

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
pub enum AesOpType {
    Encrypt = 0,
    Decrypt = 1,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct AesOp {
    /// the caller can try to request "any" index, but it's checked inside the oracle first.
    #[cfg(feature = "gen1")]
    pub key_index: u8,
    #[cfg(feature = "gen2")]
    pub domain: String,
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

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq)]
pub enum KeyWrapOp {
    Wrap = 0,
    Unwrap = 1,
}

use std::error::Error;
impl Error for KeywrapError {}

use std::{fmt, u64};

impl fmt::Display for KeywrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            KeywrapError::InvalidDataSize => f.write_str("Invalid data size"),
            KeywrapError::InvalidKekSize => f.write_str("Invalid key size"),
            KeywrapError::InvalidOutputSize => f.write_str("Invalid output size"),
            KeywrapError::IntegrityCheckFailed => f.write_str("Authentication failed"),
            KeywrapError::NoDomainSeperator => f.write_str("Missing domain separator"),
            KeywrapError::UpgradeToNew((_k, _wk)) => {
                f.write_str("Legacy migration detected! New wrapped key transmitted to caller")
            }
        }
    }
}

pub const MAX_WRAP_DATA: usize = 2048;
/// Note regression in v0.9.9: we had to return an array type in the KeywrapError enum that
/// has a signature for an array that is 40 bytes long, which is bigger than Rust's devire
/// can deal with. So, unfortunately, the result of this does *not* get zeroized on drop :(
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
// #[zeroize(drop)]
pub struct KeyWrapper {
    pub data: [u8; MAX_WRAP_DATA + 8],
    // used to specify the length of the data used in the fixed-length array above
    pub len: u32,
    #[cfg(feature = "gen1")]
    pub key_index: u8,
    #[cfg(feature = "gen2")]
    pub domain: String,
    pub op: KeyWrapOp,
    pub result: Option<KeywrapError>,
    // used by the unwrap side
    pub expected_len: u32,
}

/// The Checksums structure is an array of 16-byte (128-bit) checksums that
/// are applied to backed up data. There is one checksum per block region
/// (currently set to 1MiB).
///
/// The actual checksum is a SHA512 of the region, but with only the first
/// 128 bits stored. The goal of the checksum isn't a cryptographic tamper
/// proofing -- this is already handled by the underlying AEAD's that are
/// applied to the PDDB. The utility of the checksum is to detect media
/// or transmission errors in storage, without having to fully decrypt
/// the entire PDDB to look for corruption. We use SHA512 and truncate it to
/// 128 bits not because it's the optimal hash, but because it's what we
/// have a hardware accelerator for. We don't truncate to 256 bits because
/// we're trying to pack as many checksums into a 4k header, and going to
/// 128 bits is still "strong" for a checksum and allows us to get the region
/// size down to 1MiB, which is a reasonably sized region for a retry-download
/// in case a checksum error is found.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[repr(C, align(8))]
pub struct Checksums {
    pub checksums: [[u8; 16]; TOTAL_CHECKSUMS as usize],
}
impl Default for Checksums {
    fn default() -> Self { Checksums { checksums: [[0u8; 16]; TOTAL_CHECKSUMS as usize] } }
}
impl Deref for Checksums {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Checksums as *const u8, size_of::<Checksums>())
                as &[u8]
        }
    }
}
impl DerefMut for Checksums {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Checksums as *mut u8, size_of::<Checksums>())
                as &mut [u8]
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordState {
    /// Mounted successfully.
    Correct,
    /// User-initiated aborted. The main purpose for this path is to facilitate
    /// developers who want shellchat access but don't want to mount the PDDB.
    Incorrect(u64),
    /// Abort initiated by system policy due to too many failed attempts
    ForcedAbort(u64),
    /// Failure because the PDDB hasn't been initialized yet (can't mount because nothing to mount)
    Uninit,
}

impl From<(usize, usize, usize)> for PasswordState {
    fn from(value: (usize, usize, usize)) -> Self {
        let (arg1, arg2, arg3) = value;
        match arg1 {
            0 => PasswordState::Correct,
            1 => PasswordState::Incorrect(arg2 as u64 | (arg3 as u64) << 32),
            2 => PasswordState::ForcedAbort(arg2 as u64 | (arg3 as u64) << 32),
            3 => PasswordState::Uninit,
            _ => PasswordState::ForcedAbort(u64::MAX),
        }
    }
}

impl Into<(usize, usize, usize)> for PasswordState {
    fn into(self) -> (usize, usize, usize) {
        match self {
            PasswordState::Correct => (0, 0, 0),
            PasswordState::Incorrect(arg) => {
                (1, (arg as usize) & 0xFFFF_FFFF, (arg >> 32) as usize & 0xFFFF_FFFF)
            }
            PasswordState::ForcedAbort(arg) => {
                (2, (arg as usize) & 0xFFFF_FFFF, (arg >> 32) as usize & 0xFFFF_FFFF)
            }
            PasswordState::Uninit => (3, 0, 0),
        }
    }
}
