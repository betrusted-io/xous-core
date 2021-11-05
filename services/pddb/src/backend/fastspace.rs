use core::num::NonZeroU32;

use super::PAGE_SIZE;

#[repr(u8)]
pub(crate) enum SpaceState {
    /// pages that are completely un-spoken for
    Free,
    /// pages that are in the process of being used, but the journal has yet to be committed
    MaybeUsed,
    /// pages that are now fully used
    Used,
    /// pages that are no longer used and need to be erased
    Dirty,
}

/// FastSpace tracks a limited set of physical pages
/// This is designed to fill exactly one erase sector of 4096 bytes
#[repr(C, packed)]
pub (crate) struct FastSpace {
    p_nonce: [u8; 12],
    journal_rev: [u8; 4],
    free_pool: [Option<PhysAddr>; (PAGE_SIZE - 16 - 4 -12) / 4],
    p_tag: [u8; 16],
}

/// a 128-bit record that stores an encrypted update to the FastSpace pool
#[repr(C, packed)]
pub (crate) struct SpaceUpdate {
    nonce: u64,
    page_number: u32,
    reserved: [u8; 3],
    state: SpaceState,
}
