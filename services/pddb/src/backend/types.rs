use core::num::{NonZeroU32, NonZeroU64};
use core::convert::TryFrom;
use core::ops::Add;
use super::PAGE_SIZE;
/// PAGE_SIZE is required to be a power of two. 0x1000 -> 0x1000 - 1 = 0xFFF, which forms the bitmasks below.
#[derive(Copy, Clone)]
pub(crate) struct PageAlignedU64(u64);
impl PageAlignedU64 {
    pub(crate) fn as_u32(&self) -> u32 {
        if self.0 <= u32::MAX as u64 {
            self.0 as u32
        } else {
            panic!("This PageAlignedU64 would not fit into a PageAlignedU32");
        }
    }
    pub(crate) fn as_u64(&self) -> u64 {self.0}
}
impl From<u64> for PageAlignedU64 {
    fn from(arg: u64) -> Self {
        if arg & (PAGE_SIZE as u64 - 1) == 0 {
            PageAlignedU64(arg & !(PAGE_SIZE as u64 - 1))
        } else {
            PageAlignedU64((arg & !(PAGE_SIZE as u64 - 1)) + PAGE_SIZE as u64)
        }
    }
}
impl From<PageAlignedU32> for PageAlignedU64 {
    fn from(arg: PageAlignedU32) -> Self { PageAlignedU64(arg.0 as u64) } // already aligned, and it fits.
}
impl From<u32> for PageAlignedU64 {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE as u32 - 1) == 0 {
            PageAlignedU64(arg as u64 & !(PAGE_SIZE as u64 - 1))
        } else {
            PageAlignedU64((arg as u64 & !(PAGE_SIZE as u64 - 1)) + PAGE_SIZE as u64)
        }
    }
}
impl From<usize> for PageAlignedU64 {
    fn from(arg: usize) -> Self {
        if arg & (PAGE_SIZE - 1) == 0 {
            PageAlignedU64(arg as u64 & !(PAGE_SIZE as u64 - 1))
        } else {
            PageAlignedU64((arg as u64 & !(PAGE_SIZE as u64 - 1)) + PAGE_SIZE as u64)
        }
    }
}
impl Add for PageAlignedU64 {
    type Output = PageAlignedU64;
    fn add(self, other: PageAlignedU64) -> PageAlignedU64 {
        PageAlignedU64(self.0 + other.0)
    }
}

#[derive(Copy, Clone)]
pub(crate) struct PageAlignedU32(u32);
impl PageAlignedU32 {
    pub(crate) fn as_u32(&self) -> u32 {self.0}
    pub(crate) fn as_u64(&self) -> u64 {self.0 as u64}
}
impl From<u32> for PageAlignedU32 {
    fn from(arg: u32) -> Self {
        if arg & (PAGE_SIZE as u32 - 1) == 0 {
            PageAlignedU32(arg & !(PAGE_SIZE as u32 - 1))
        } else {
            PageAlignedU32((arg & !(PAGE_SIZE as u32 - 1)) + PAGE_SIZE as u32)
        }
    }
}
impl From<usize> for PageAlignedU32 {
    fn from(arg: usize) -> Self {
        if arg & (PAGE_SIZE as usize - 1) == 0 {
            PageAlignedU32(arg as u32 & !(PAGE_SIZE as u32 - 1))
        } else {
            PageAlignedU32((arg as u32 & !(PAGE_SIZE as u32 - 1)) + PAGE_SIZE as u32)
        }
    }
}
/*  // let's get real, this is kind of a waste of cycles to check this...
impl TryFrom<usize> for PageAlignedU32 {
    type Error = &'static str;
    fn try_from(arg: usize) -> Result<Self, Self::Error> {
        if arg > u32::MAX as usize {
            Err("PageAlignedU32 only accepts usize less than u32::MAX")
        } else {
            Ok(
                if arg & (PAGE_SIZE - 1) == 0 {
                    PageAlignedU32(arg as u32 & !(PAGE_SIZE as u32 - 1))
                } else {
                    PageAlignedU32((arg as u32 & !(PAGE_SIZE as u32 - 1)) + PAGE_SIZE as u32)
                }
            )
        }
    }
}
*/
impl From<PageAlignedU32> for u32 {
    fn from(arg: PageAlignedU32) -> Self {
        arg.0
    }
}
impl Add for PageAlignedU32 {
    type Output = PageAlignedU32;
    fn add(self, other: PageAlignedU32) -> PageAlignedU32 {
        PageAlignedU32(self.0 + other.0 as u32)
    }
    /*
    fn add(self, other: usize) -> PageAlignedU32 {
        let page_aligned_other = if other & (PAGE_SIZE - 1) == 0 {
            other & !(PAGE_SIZE - 1)
        } else {
            (other & !(PAGE_SIZE - 1)) + PAGE_SIZE
        };
        // saturate adds
        if self.0 as usize + page_aligned_other > u32::MAX as usize {
            PageAlignedU32(u32::MAX)
        } else {
            PageAlignedU32(self.0 + page_aligned_other as u32)
        }
    }*/
}

/*
#[derive(Hash, Copy, Clone, PartialEq, Eq)]
pub struct PhysAddr(NonZeroU32);
impl From<u32> for PhysAddr {
    fn from(arg: u32) -> Self {
        PhysAddr( NonZeroU32::try_from(arg).expect("Physical addresses must not be zero") )
    }
}
*/

pub type VirtAddr = u64;

mod tests {
    use super::*;
    #[test]
    fn test_page_size() {
        assert!(PAGE_SIZE & !(PAGE_SIZE - 1) == 0, "PAGE_SIZE is not a power of two!");
    }
}
