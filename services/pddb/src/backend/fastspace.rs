use core::num::NonZeroU32;

use super::PAGE_SIZE;
use crate::*;

/// Each free_pool entry takes about 4 bytes, so give-or-take we have about 1000 free_pool
/// entries per page of storage for the free_pool, or 4k * 1000 ~ 4MiB per page, when PhysAddr is a u32
pub(crate) const FASTSPACE_PAGES: usize = 2;

#[repr(u8)]
pub(crate) enum SpaceState {
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
    p_nonce: [u8; 12],
    /// u32 in lsb byte order
    journal_rev: [u8; 4],
    /// Not sure if there is a "better" way to compute things, but we want the number of entries in the
    /// free_pool array to "round out" the FastSpace record to be equal to exactly one page
    free_pool: [PhysPage; ((PAGE_SIZE * FASTSPACE_PAGES) - (12 + 4 + 16)) / core::mem::size_of::<PhysPage>()],
    p_tag: [u8; 16],
}

/// a 128-bit record that stores an encrypted update to the FastSpace pool, facilitating the "rarely" trait of the structure.
#[repr(C, packed)]
#[cfg(not(feature = "u64_pa"))]
pub (crate) struct SpaceUpdate {
    nonce: u64,
    page_number: PhysAddr,
    reserved: [u8; 4],
}
#[cfg(feature = "u64_pa")]
pub (crate) struct SpaceUpdate {
    nonce: u64,
    page_number: PhysAddr,
}
impl SpaceUpdate {
    // add accessors, decryptors, and constructors so we don't shoot ourselves in the foot so much.
}

mod tests {
    use super::*;
    #[test]
    fn test_fast_space_size() {
        assert!(core::mem::size_of::<FastSpace>() & !(PAGE_SIZE - 1) == 0, "FastSpace is not exactly a multiple of one page in size");
    }
}
