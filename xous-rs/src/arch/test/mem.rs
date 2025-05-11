use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

extern crate alloc;
use alloc::alloc::{Layout, alloc, dealloc};

pub const PAGE_SIZE: usize = 4096;
pub const MMAP_VIRT_BASE: usize = 0xb000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;
pub const USER_AREA_END: usize = 0xff00_0000;

pub fn map_memory_pre(
    _phys: &Option<MemoryAddress>,
    _virt: &Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
) -> core::result::Result<(), Error> {
    Ok(())
}

pub fn map_memory_post(
    _phys: Option<MemoryAddress>,
    _virt: Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
    range: MemoryRange,
) -> core::result::Result<MemoryRange, Error> {
    let layout = Layout::from_size_align(range.len(), 4096).unwrap();
    let new_mem = MemoryAddress::new(unsafe { alloc(layout) } as usize).ok_or(Error::BadAddress)?;
    Ok(unsafe { MemoryRange::new(new_mem.get(), range.len()).unwrap() })
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> { Ok(()) }

pub fn unmap_memory_post(range: MemoryRange) -> core::result::Result<(), Error> {
    let layout = Layout::from_size_align(range.len(), 4096).unwrap();
    let ptr = range.as_mut_ptr();
    unsafe { dealloc(ptr, layout) };
    Ok(())
}
