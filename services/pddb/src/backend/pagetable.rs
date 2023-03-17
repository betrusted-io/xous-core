use super::{PAGE_SIZE, TrngPool, VirtAddr, murmur3_32, VPAGE_SIZE};
use core::mem::size_of;
use aes_gcm_siv::{Nonce, Tag};
use std::rc::Rc;
use core::cell::RefCell;
use core::ops::{Deref, DerefMut};
use core::convert::TryInto;

use bitflags::bitflags;

bitflags! {
    /// flags used by the page table
    pub struct PtFlags: u8 {
        /// Pages that don't decrypt properly are marked as INVALID in the cache.
        const  INVALID            = 0b0000_0000;
        /// set for records that are synced to the copy in Flash. Every valid record
        /// from Flash should have this set; it should only be cleared for blocks in Cache.
        const  CLEAN              = 0b0000_0001;
        /// set for records that are confirmed to be valid through a subsequent decryption op.
        /// This flag exists because there is a chance that the 32-bit checksum used to protect
        /// a page table entry experiences a collision.
        const CHECKED             = 0b0000_0010;
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
#[repr(packed)]
#[derive(Default)]
pub(crate) struct Pte {
    /// the virtual page number is 52 bits long (52 + 12 = 64). 4 bits are wasted in this representation.
    /// The storage format is in *page numbers* but the API accepts *addresses*. Therefore a division and
    /// multiplication by VPAGE_SIZE wraps the getters and setters for this field.
    pddb_addr: [u8; 7],
    /// this maps to a u8
    flags: PtFlags,
    /// 32-bit strength of a nonce, but can be varied
    nonce: [u8; 4],
    /// 32-bit "weak" checksum, used only for quick scans of the PTE to determine a coarse "in" or "out" classifier
    /// checksum is computed on all of the bits prior, so checksum(pddb_addr, flags, nonce)
    checksum: [u8; 4],
}
impl Pte {
    pub fn new(va: VirtAddr, flags: PtFlags, entropy: Rc<RefCell<TrngPool>>) -> Self {
        let nonce_u32 = entropy.borrow_mut().get_u32();
        let mut pte = Pte {
            pddb_addr: (va.get() / VPAGE_SIZE as u64).to_le_bytes()[..7].try_into().unwrap(),
            flags,
            nonce: nonce_u32.to_le_bytes(),
            checksum: [0; 4],
        };
        let pte_data = pte.deref();
        let checksum = murmur3_32(&pte_data[..12], nonce_u32);
        pte.checksum = checksum.to_le_bytes();

        pte
    }
    pub fn vaddr(&self) -> VirtAddr {
        let mut full_addr = [0u8; 8];
        // LSB encoded, so this loop deposits the partial pddb_addr in the LSBs, and the MSBs are correctly 0 from above initializer
        for (&src, dst) in self.pddb_addr.iter().zip(full_addr.iter_mut()) {
            *dst = src;
        }
        VirtAddr::new(u64::from_le_bytes(full_addr) * VPAGE_SIZE as u64).unwrap()
    }
    /// V1 databases stored the virtual address as a full address, instead of as a page number, which means
    /// the overall size of our database was about 4000x smaller than we had thought. This was fixed in v2,
    /// but this getter is required to migrate from v1.
    /// This allows us to retrieve the old address format for the first phase of migration.
    #[cfg(feature="migration1")]
    pub fn vaddr_v1(&self) -> VirtAddr {
        let mut full_addr = [0u8; 8];
        // LSB encoded, so this loop deposits the partial pddb_addr in the LSBs, and the MSBs are correctly 0 from above initializer
        for (&src, dst) in self.pddb_addr.iter().zip(full_addr.iter_mut()) {
            *dst = src;
        }
        VirtAddr::new(u64::from_le_bytes(full_addr)).unwrap()
    }

    #[allow(dead_code)]
    pub fn flags(&self) -> PtFlags {
        self.flags
    }
    pub fn try_from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() == size_of::<Pte>() {
            let mut maybe_pt = Pte::default();
            for (&src, dst) in slice.iter().zip(maybe_pt.deref_mut().iter_mut()) {
                *dst = src;
            }
            let nonce_u32 = u32::from_le_bytes(maybe_pt.nonce);
            if u32::from_le_bytes(maybe_pt.checksum) == murmur3_32(&slice[..12], nonce_u32) {
                Some(maybe_pt)
            } else {
                None
            }
        } else {
            None
        }
    }
    /// Normally you should be using pt_patch_mapping(), which generates a new nonce every
    /// time the entry is patched. However, this function is provided for "bulk" operations
    /// such as migrations where we violate the abstractions to improve performance.
    pub fn re_nonce(&mut self, entropy: Rc<RefCell<TrngPool>>) {
        let nonce_u32 = entropy.borrow_mut().get_u32();
        self.nonce = nonce_u32.to_le_bytes();
        let pte_data = self.deref();
        let checksum = murmur3_32(&pte_data[..12], nonce_u32);
        self.checksum = checksum.to_le_bytes();
    }
}
impl Deref for Pte {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Pte as *const u8, core::mem::size_of::<Pte>())
                as &[u8]
        }
    }
}

impl DerefMut for Pte {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Pte as *mut u8, core::mem::size_of::<Pte>())
                as &mut [u8]
        }
    }
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
#[allow(dead_code)] // this structure is never explicitly created, but it's helpful to have to document our layout on disk.
pub(crate) struct EncryptedPage {
    /// the nonce is not encrypted
    p_nonce: [u8; size_of::<Nonce>()],
    /// journal_rev is encrypted and indicates the current journal revision for the block (u32 le)
    journal_rev: [u8; 4],
    /// data is encrypted and holds the good stuff
    data: [u8; PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<u32>()],
    /// tag is the authentication tag. If the page decrypts & authenticates, we know it's a valid data block for us.
    p_tag: [u8; size_of::<Tag>()],
}