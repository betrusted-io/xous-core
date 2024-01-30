use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

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
    Ok(range)
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> { Ok(()) }

pub fn unmap_memory_post(_range: MemoryRange) -> core::result::Result<(), Error> { Ok(()) }
