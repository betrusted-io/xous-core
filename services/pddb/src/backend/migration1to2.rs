use core::mem::size_of;
use core::ops::Deref;

use crate::*;

pub(crate) const SCD_VERSION_MIGRATION1: u32 = 1;

#[repr(C)] // this can map directly into Flash
pub(crate) struct StaticCryptoDataV1 {
    /// a version number for the block
    pub(crate) version: u32,
    /// aes-256 key of the system basis, encrypted with the User0 root key, and wrapped using NIST SP800-38F
    pub(crate) system_key: [u8; WRAPPED_AES_KEYSIZE],
    /// a pool of fixed data used as a salt
    pub(crate) salt_base: [u8; 4096 - WRAPPED_AES_KEYSIZE - size_of::<u32>()],
}
impl StaticCryptoDataV1 {
    pub fn default() -> StaticCryptoDataV1 {
        StaticCryptoDataV1 {
            version: SCD_VERSION_MIGRATION1,
            system_key: [0u8; WRAPPED_AES_KEYSIZE],
            salt_base: [0u8; 4096 - WRAPPED_AES_KEYSIZE - size_of::<u32>()],
        }
    }
}
impl Deref for StaticCryptoDataV1 {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const StaticCryptoDataV1 as *const u8,
                size_of::<StaticCryptoDataV1>(),
            ) as &[u8]
        }
    }
}

pub(crate) fn data_aad_v1(pddb_os: &PddbOs, name: &str) -> Vec<u8> {
    let mut aad = Vec::<u8>::new();
    aad.extend_from_slice(&name.as_bytes());
    let (old_version, _new_version) = PDDB_MIGRATE_1;
    aad.extend_from_slice(&old_version.to_le_bytes());
    aad.extend_from_slice(&pddb_os.dna().to_le_bytes());
    aad
}
