use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

extern crate alloc;
use alloc::alloc::{Layout, alloc, dealloc};

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
