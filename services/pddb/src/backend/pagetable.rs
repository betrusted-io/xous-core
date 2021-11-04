
/// A Page Table Entry. Contains the address map of the corresponding entry,
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
pub(crate) struct Pte {
    /// the virtual address is 48 bits long
    pddb_addr: [u8; 6],
    /// the flags are largely TBD at this moment
    flags: [u8; 2],
    /// 32-bit strength of a nonce, but can be varied
    nonce: [u8; 4],
    /// 32-bit "weak" checksum, used only for quick scans of the PTE to determine a coarse "in" or "out" classifier
    /// checksum is computed on all of the bits prior, so checksum(pddb_addr, flags, nonce)
    checksum: [u8; 4],
}

/// This structure is mapped into the top of FLASH memory, starting at
/// xous::PDDB_LOC
pub const PDDB_SIZE_PAGES: usize = xous::PDDB_LEN as usize / 4096;
#[repr(C, packed)]
pub(crate) struct PageTableInFLash {
    table: [Pte; PDDB_SIZE_PAGES],
}

/// This is the representation of a page of data on disk. Keys that span multiple
/// pages have to decrypt individual pages, subtracting the nonce, journalrev, and tag, to find
/// the actual data being retrieved.
pub(crate) struct EncryptedPage {
    /// the nonce is not encrypted
    p_nonce: [u8; 12],
    /// journal_rev is encrypted and indicates the current journal revision for the block
    journal_rev: [u8; 4],
    /// data is encrypted and holds the good stuff
    data: [u8; (4096 - 12 - 16 - 4)],
    /// tag is the authentication tag. If the page decrypts & authenticates, we know it's a valid data block for us.
    p_tag: [u8; 16],
}