use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

pub fn map_memory_pre(
    _phys: &Option<MemoryAddress>,
    _virt: &Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
) -> core::result::Result<(), Error> {
    Ok(())
}

#[allow(unused_mut)]
pub fn map_memory_post(
    _phys: Option<MemoryAddress>,
    _virt: Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
    mut range: MemoryRange,
) -> core::result::Result<MemoryRange, Error> {
    let data = vec![0u8; range.len()];
    let mut data = std::mem::ManuallyDrop::new(data);
    data.shrink_to_fit();
    assert_eq!(data.len(), data.capacity());
    let len = data.len();
    let addr = data.as_mut_ptr();
    Ok(unsafe { crate::MemoryRange::new(addr as _, len).unwrap() })
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> {
    Ok(())
}

pub fn unmap_memory_post(range: MemoryRange) -> core::result::Result<(), Error> {
    let _v = unsafe { std::vec::Vec::from_raw_parts(range.as_mut_ptr(), range.len(), range.len()) };
    Ok(())
}

// Ideally we'd use the `alloc` API directly, however this doesn't work for unknown reasons.
// The code is left here as an exercise to someone who wants to figure out why.

// use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

// extern crate alloc;
// use alloc::alloc::{alloc, dealloc, Layout};

// pub fn map_memory_pre(
//     _phys: &Option<MemoryAddress>,
//     _virt: &Option<MemoryAddress>,
//     _size: usize,
//     _flags: MemoryFlags,
// ) -> core::result::Result<(), Error> {
//     Ok(())
// }

// pub fn map_memory_post(
//     _phys: Option<MemoryAddress>,
//     _virt: Option<MemoryAddress>,
//     size: usize,
//     _flags: MemoryFlags,
//     _range: MemoryRange,
// ) -> core::result::Result<MemoryRange, Error> {
//     let rounded_size = (size + 4095) / 4096;

//     let layout = Layout::from_size_align(rounded_size, 4096).unwrap();
//     unsafe { MemoryRange::new(alloc(layout) as usize, size) }
// }

// pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> {
//     Ok(())
// }

// pub fn unmap_memory_post(range: MemoryRange) -> core::result::Result<(), Error> {
//     // let rounded_size = (range.len() + 4095) / 4096;
//     // let layout = Layout::from_size_align(rounded_size, 4096).unwrap();
//     // let ptr = range.as_mut_ptr();
//     // unsafe { dealloc(ptr, layout) };
//     Ok(())
// }
