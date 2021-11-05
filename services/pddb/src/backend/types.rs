use core::num::{NonZeroU32, NonZeroU64};

/// PAGE_SIZE is required to be a power of two. 0x1000 -> 0x1000 - 1 = 0xFFF, which forms the bitmasks below.
struct PageAlignedU64(u64);
impl From<u64> for PageAlignedU64 {
    fn from(arg: u64) -> Self {
        if arg & (PAGE_SIZE - 1) == 0 {
            PageAlignedU64(arg & !(PAGE_SIZE - 1))
        } else {
            PageAlignedU64((arg & !(PAGE_SIZE - 1)) + PAGE_SIZE)
        }
    }
}
impl From<u32> for PageAlignedU64 {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE - 1) == 0 {
            PageAlignedU64(arg as u64 & !(PAGE_SIZE - 1))
        } else {
            PageAlignedU64((arg as u64 & !(PAGE_SIZE - 1)) + PAGE_SIZE)
        }
    }
}
struct PageAlignedU32(u32);
impl From<u32> for PageAlignedU32 {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE - 1) == 0 {
            PageAlignedU32(arg & !(PAGE_SIZE - 1))
        } else {
            PageAlignedU32((arg & !(PAGE_SIZE - 1)) + PAGE_SIZE)
        }
    }
}
impl From<PageAlignedU32> for u32 {
    fn from(arg: PageAlignedU32) -> Self {
        arg.0
    }
}


pub struct PhysAddr(NonZeroU32);
pub struct VirtAddr(NonZeroU64);


mod tests {
    use super::*;
    #[test]
    fn test_page_size() {
        assert!(PAGE_SIZE & !(PAGE_SIZE - 1) == 0, "PAGE_SIZE is not a power of two!");
    }
}
