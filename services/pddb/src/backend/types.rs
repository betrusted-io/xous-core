use core::num::{NonZeroU32, NonZeroU64};
use core::convert::TryFrom;
use core::ops::Add;
use super::PAGE_SIZE;
use crate::SpaceState;
use bitfield::bitfield;
use std::hash::{Hash, Hasher};

/// for the life of me, I can't figure out how to query the AES crate to give me the length of a 256-bit key.
/// I mean, we know what it is, it's well-defined and never changes. But it'd just be nice to you know,
/// derive it from a const or something with symbolic meaning, but the KeySize Trait is buried in some sort
/// of a NewBlock trait and I can't figure out how to access it. Looking at the example code on the AES crate
/// on docs.rs, they just pull the number 16 out of their ass instead of referring to a trait.
/// So, maybe that's just what you're supposed to do. ¯\_(ツ)_/¯ Oddly enough, a BLOCK_SIZE constant /is/ defined,
/// but maybe that's because it's constant regardless of the key size so it's easy to do.
pub(crate) const AES_KEYSIZE: usize = 32;

/// This has to be manually synchronized with the bit range of the `journal` field below. It doesn't look like
/// there is a good way to automatically derive this.
pub(crate) const PHYS_PAGE_JOURNAL_MAX: u8 = 15;
/// We should be able to change this to a u64 and everything should "just work", but
/// we'd end up using 2x the amount of data for overhead and bookkeeping.
#[cfg(not(feature = "u64_pa"))]
pub type PhysAddr = u32;
#[cfg(feature = "u64_pa")]
pub type PhysAddr = u64;
const BITFIELD_PAGE_WIDTH: usize = (core::mem::size_of::<PhysAddr>() * 8 - 12);
// Physical page information, coded as a bitfield, because space is a premium!
bitfield! {
    #[derive(Copy, Clone, Eq)]
    pub struct PhysPage(PhysAddr);
    impl Debug;
    pub page_number, set_page_number: BITFIELD_PAGE_WIDTH - 1, 0;
    // this is only used by the page table mechanism
    pub clean, set_clean: BITFIELD_PAGE_WIDTH + 0;
    // when set, indicates that the record contents are valid and should be used
    // when cleared, the record contents are invalid and should be ignored.
    // valid is used by both FastSpace and the page table mechanism. Note that we rely upon the mapping of 0->not valid.
    pub valid, set_valid: BITFIELD_PAGE_WIDTH + 1;
    // these are only used by the FastSpace mechanism; they have no meaning in other contexts
    pub u8, from into SpaceState, space_state, set_space_state: BITFIELD_PAGE_WIDTH + 3, BITFIELD_PAGE_WIDTH + 2;
    // 4 bits for a journal revision. Intended for the FastSpace mechanism
    pub u8, journal, set_journal: BITFIELD_PAGE_WIDTH + 8, BITFIELD_PAGE_WIDTH + 4;
}
// hashes should only key off of the page number, not the metadata
impl Hash for PhysPage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.page_number().hash(state);
    }
}
impl PartialEq for PhysPage {
    fn eq(&self, other: &Self) -> bool {
        self.page_number() == other.page_number()
    }
}


pub type VirtAddr = u64;

#[derive(Copy, Clone)]
pub(crate) struct PageAlignedVa(VirtAddr);
impl PageAlignedVa {
    pub(crate) fn as_u32(&self) -> u32 {
        if self.0 <= u32::MAX as VirtAddr {
            self.0 as u32
        } else {
            panic!("This PageAlignedVa would not fit into a PageAlignedPa");
        }
    }
    pub(crate) fn as_u64(&self) -> u64 {self.0 as u64}
    pub(crate) fn as_usize(&self) -> usize {self.0 as usize}
}
impl From<u64> for PageAlignedVa {
    fn from(arg: u64) -> Self {
        if arg & (PAGE_SIZE as u64 - 1) == 0 {
            PageAlignedVa(arg & !(PAGE_SIZE as VirtAddr - 1))
        } else {
            PageAlignedVa((arg & !(PAGE_SIZE as VirtAddr - 1)) + PAGE_SIZE as VirtAddr)
        }
    }
}
impl From<PageAlignedPa> for PageAlignedVa {
    fn from(arg: PageAlignedPa) -> Self { PageAlignedVa(arg.0 as VirtAddr) } // already aligned, and it fits.
}
impl From<u32> for PageAlignedVa {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE as u32 - 1) == 0 {
            PageAlignedVa(arg as VirtAddr & !(PAGE_SIZE as VirtAddr - 1))
        } else {
            PageAlignedVa((arg as VirtAddr & !(PAGE_SIZE as VirtAddr - 1)) + PAGE_SIZE as VirtAddr)
        }
    }
}
impl From<usize> for PageAlignedVa {
    fn from(arg: usize) -> Self {
        if arg & (PAGE_SIZE - 1) == 0 {
            PageAlignedVa(arg as VirtAddr & !(PAGE_SIZE as VirtAddr - 1))
        } else {
            PageAlignedVa((arg as VirtAddr & !(PAGE_SIZE as VirtAddr - 1)) + PAGE_SIZE as VirtAddr)
        }
    }
}
impl Add for PageAlignedVa {
    type Output = PageAlignedVa;
    fn add(self, other: PageAlignedVa) -> PageAlignedVa {
        PageAlignedVa(self.0 + other.0)
    }
}

#[derive(Copy, Clone)]
pub(crate) struct PageAlignedPa(PhysAddr);
impl PageAlignedPa {
    pub(crate) fn as_u32(&self) -> u32 {self.0 as u32}
    pub(crate) fn as_u64(&self) -> u64 {self.0 as u64}
    pub(crate) fn as_usize(&self) -> usize {self.0 as usize}
    pub(crate) fn as_phys_addr(&self) -> PhysAddr {self.0}
}
impl From<u32> for PageAlignedPa {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE as u32 - 1) == 0 {
            PageAlignedPa( (arg & !(PAGE_SIZE as u32 - 1)) as PhysAddr )
        } else {
            PageAlignedPa( ((arg & !(PAGE_SIZE as u32 - 1)) + PAGE_SIZE as u32) as PhysAddr )
        }
    }
}
impl From<usize> for PageAlignedPa {
    fn from(arg: usize) -> Self {
        if arg & (PAGE_SIZE as usize - 1) == 0 {
            PageAlignedPa( (arg as u32 & !(PAGE_SIZE as u32 - 1)) as PhysAddr )
        } else {
            PageAlignedPa( ((arg as u32 & !(PAGE_SIZE as u32 - 1)) + PAGE_SIZE as u32) as PhysAddr )
        }
    }
}
impl From<PageAlignedPa> for u32 {
    fn from(arg: PageAlignedPa) -> Self {
        arg.0 as u32
    }
}
impl Add for PageAlignedPa {
    type Output = PageAlignedPa;
    fn add(self, other: PageAlignedPa) -> PageAlignedPa {
        PageAlignedPa(self.0 + other.0 as PhysAddr)
    }
}


mod tests {
    use super::*;
    #[test]
    /// PAGE_SIZE is required to be a power of two. 0x1000 -> 0x1000 - 1 = 0xFFF, which forms the bitmasks.
    fn test_page_size() {
        assert!(PAGE_SIZE & !(PAGE_SIZE - 1) == 0, "PAGE_SIZE is not a power of two!");
    }
    /// This test exists because nothing in the bitfield spec explicitly requires that a true maps to a 1.
    /// In fact a lot of code would work just fine if you mapped true to 0 and false to 1: if you're just using
    /// the generated getter and setter, it woudln't matter.
    /// However, in our application, we fully expect a true to be a 1. This test exists to ensure this seemingly
    /// obvious but not explicitly stated fact always remains true.
    fn test_bitfield_bool() {
        bitfield! {
            pub struct Test(u8);
            impl Debug;
            pub test, set_test: 1;
        }
        let mut t = Test(0);
        t.set_test(true);
        assert!(t.0 == 0x2, "polarity of boolean bit is not as expected");
        assert!(t.test() == true, "bool getter did not work as expected");
    }
    fn test_journal_range() {
        let pp = PhysPage(u32::MAX);
        assert!(pp.journal() == PHYS_PAGE_JOURNAL_MAX, "PHYS_PAGE_JOURNAL_MAX is incorrect");
    }
}
