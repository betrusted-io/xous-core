use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};
const PAGE_SIZE: usize = 4096;

extern crate alloc;
use alloc::alloc::{alloc, dealloc, Layout};

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
    size: usize,
    _flags: MemoryFlags,
    _range: MemoryRange,
) -> core::result::Result<MemoryRange, Error> {
    // let rounded_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let layout = Layout::from_size_align(size, PAGE_SIZE).unwrap().pad_to_align();
    let mem = unsafe { alloc(layout) } as usize;

    // println!("Allocated {} bytes (requested {}) @ {:016x}", rounded_size, size, mem);
    unsafe { MemoryRange::new(mem, size) }
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> { Ok(()) }

pub fn unmap_memory_post(range: MemoryRange) -> core::result::Result<(), Error> {
    // println!("Request to free {} bytes @ {:016x}", range.len(), range.as_ptr() as usize);
    // let rounded_size = (range.len() + 4095) / 4096;
    let layout = Layout::from_size_align(range.len(), PAGE_SIZE).unwrap().pad_to_align();
    let ptr = range.as_mut_ptr();
    unsafe { dealloc(ptr, layout) };
    Ok(())
}
