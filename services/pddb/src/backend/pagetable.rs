use super::PAGE_SIZE;
use core::mem::size_of;
use aes_gcm_siv::{Nonce, Tag};

use bitflags::bitflags;

bitflags! {
    /// flags used by the page table
    pub struct PtFlags: u8 {
        /// Pages that don't decrypt properly are marked as INVALID in the cache.
        const  INVALID            = 0b0000_0000;
        /// set for records that are synced to the copy in Flash. Every valid record
        /// from Flash should have this set; it should only be cleared for blocks in Cache.
        const  CLEAN              = 0b0000_0001;

    }
}
impl Default for PtFlags {
    fn default() -> PtFlags {PtFlags::INVALID}
}

/// A Page Table Entry. Must be equal in length to one AES block size (128 bits).
/// This is stored in the FLASH itself, so size is not as much of a constraint.
///
/// Contains the address map of the corresponding entry,
/// plus a nonce, and a checksum. Due to the Page Table being deliberately
/// srtuctured to have invalid entries that don't decrypt correctly, you
/// can't use a chaining approach. Thus these entries are encrypted closer to
/// an ECB-style, thus an embedded nonce is necessary to keep identical entries
/// from appearing the same in the ciphertext domain.
///
/// It's not clear at all if the nonce is large enough to prevent random collisions;
/// however, the sheer bulk of the page table demands a compact representation. Thus,
/// any routines downstream of the Pte shall be coded to handle potentially a much larger
/// nonce and checksum structure.
#[repr(C, packed)]
#[derive(Default)]
pub(crate) struct Pte {
    /// the virtual address is 48 bits long
    pddb_addr: [u8; 6],
    /// this maps to a u8
    flags: PtFlags,
    reserved: u8,
    /// 32-bit strength of a nonce, but can be varied
    nonce: [u8; 4],
    /// 32-bit "weak" checksum, used only for quick scans of the PTE to determine a coarse "in" or "out" classifier
    /// checksum is computed on all of the bits prior, so checksum(pddb_addr, flags, nonce)
    checksum: [u8; 4],
}

pub const PDDB_SIZE_PAGES: usize = crate::PDDB_A_LEN as usize / PAGE_SIZE;
/// This structure is mapped into the top of FLASH memory, starting at
/// xous::PDDB_LOC. This actually slightly over-sizes the page table,
/// because the page table does not map the locations for the page table
/// itself, the MBBB, or the FSCB. However, the 0th entry of the page table
/// always corresponds to the base of data in FLASH, which means the excess
/// pages are going to be toward the high end of the page table range.
#[repr(C, packed)]
pub(crate) struct PageTableInFlash {
    table: [Pte; PDDB_SIZE_PAGES],
}

/// This is the representation of a page of data on disk. Keys that span multiple
/// pages have to decrypt individual pages, subtracting the nonce, journalrev, and tag, to find
/// the actual data being retrieved.
pub(crate) struct EncryptedPage {
    /// the nonce is not encrypted
    p_nonce: [u8; size_of::<Nonce>()],
    /// journal_rev is encrypted and indicates the current journal revision for the block (u32 le)
    journal_rev: [u8; 4],
    /// data is encrypted and holds the good stuff
    data: [u8; (PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<u32>())],
    /// tag is the authentication tag. If the page decrypts & authenticates, we know it's a valid data block for us.
    p_tag: [u8; size_of::<Tag>()],
}