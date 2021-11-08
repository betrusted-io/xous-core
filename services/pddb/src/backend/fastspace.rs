use core::num::NonZeroU32;
use core::ops::Deref;

use super::PAGE_SIZE;
use crate::*;

/// Each free_pool entry takes about 4 bytes, so give-or-take we have about 1000 free_pool
/// entries per page of storage for the free_pool, or 4k * 1000 ~ 4MiB per page, when PhysAddr is a u32
pub(crate) const FASTSPACE_PAGES: usize = 2;

#[derive(Debug)]
#[repr(u8)]
pub enum SpaceState {
    /// pages that are completely un-spoken for
    Free = 0,
    /// pages that are in the process of being used, but the journal has yet to be committed
    /// in other words, these are pages that might be in the RAM cache.
    MaybeUsed = 1,
    /// pages that are confirm plus chop fully used
    Used = 2,
    /// pages that are no longer used and need to be erased
    Dirty = 3,
}
impl From<u8> for SpaceState {
    fn from(arg: u8) -> Self {
        match arg & 0x3 {
            0 => SpaceState::Free,
            1 => SpaceState::MaybeUsed,
            2 => SpaceState::Used,
            _ => SpaceState::Dirty,
        }
    }
}
impl From<SpaceState> for u8 {
    fn from(arg: SpaceState) -> Self {
        arg as u8
    }
}

pub(crate) const FASTSPACE_FREE_POOL_LEN: usize = ((PAGE_SIZE * FASTSPACE_PAGES) - (12 + 16)) / core::mem::size_of::<PhysPage>();
/// FastSpace tracks a limited set of physical pages
/// An optimal implementation of this would fill whole pages with the free_pool array.
/// This record is meant to be updated rarely, and atomically, in a make-before-break fashion:
/// Make-before-break: (maybe it's COW? idk, I'm a hardware guy, so I'm switched on to make-before-break)
///   Write a new version of this with a higher journal number, verify its
///   integrity, and then erase the previous version.
/// Atomically:
///   The entire contents of this structure is written in the update operation; there are no partial updates
///   possible to the structure
/// Rarely:
///   Instead of partial updates to this structure, individual "SpaceUpdate" records are encrypted to the
///   system basis and stored in a non-cryptographically free region. These are decrypted and merged
///   into an in-RAM cache that facilitates the "rare" update of this large-ish structure. This creates
///   a side-channel, where an attacker would be able to observe the rate at which pages are modified...
///   and hopefully nothing else?
///   The "rare" property is important especially as the disk size scales up; if we wanted to keep
///   100MiB of "fast space" on hand, this structure would span 25 pages.
#[repr(C, packed)]
pub (crate) struct FastSpace {
    /// Not sure if there is a "better" way to compute things, but we want the number of entries in the
    /// free_pool array to "round out" the FastSpace record to be equal to exactly one page
    pub(crate) free_pool: [PhysPage; FASTSPACE_FREE_POOL_LEN],
}
impl Deref for FastSpace {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const FastSpace as *const u8, core::mem::size_of::<FastSpace>())
                as &[u8]
        }
    }
}

/// this structure is created just to confirm that the overall size is exactly one page, and should only
/// be used by the test included in this file.
#[repr(C, packed)]
struct FastSpaceInFlash {
    p_nonce: [u8; 12],
    ram_rep: FastSpace,
    p_tag: [u8; 16],
}

/// a 128-bit record that stores an encrypted update to the FastSpace pool, facilitating the "rarely" update property of the structure.
#[repr(C, packed)]
#[cfg(not(feature = "u64_pa"))]
pub (crate) struct SpaceUpdate {
    nonce: u64,
    page_number: PhysAddr,
    // this checksum is "weak" but we are protecting against two scenarios:
    // 1. partially written SpaceUpdate record (so the last bytes or so are FF)
    // 2. a malicious attacker
    // In the case of (1), the occurence should be diminishingly small (expected to never happen, maybe
    // a very unstable system that's "blinking" power constantly would have it occure a few times)
    // In the case of (2), an attacker has a chance of generating a collision, but the result is
    // also unlikely to generate a valid PhysAddr, and if it does, the consequence is some valid data
    // being treated as free space and getting erased (data loss, not disclosure).
    checksum: [u8; 4],
}
#[cfg(feature = "u64_pa")]
pub (crate) struct SpaceUpdate {
    nonce: u64,
    page_number: PhysAddr,
    // consider: using the top 12 bits of the PhysAddr as a checksum
}
impl SpaceUpdate {
    // add accessors, decryptors, and constructors so we don't shoot ourselves in the foot so much.
}

mod tests {
    use super::*;
    #[test]
    fn test_fast_space_size() {
        assert!(core::mem::size_of::<FastSpaceInFlash>() & !(PAGE_SIZE - 1) == 0, "FastSpace is not exactly a multiple of one page in size");
    }
}
