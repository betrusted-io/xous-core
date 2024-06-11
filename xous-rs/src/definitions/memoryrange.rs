use crate::definitions::{Error, MemoryAddress, MemorySize};

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct MemoryRange {
    pub(crate) addr: MemoryAddress,
    pub(crate) size: MemorySize,
}

impl MemoryRange {
    /// # Safety
    ///
    /// This allows for creating a `MemoryRange` from any arbitrary pointer,
    /// so it is imperative that this only be used to point to valid, page-aligned
    /// ranges.
    pub unsafe fn new(addr: usize, size: usize) -> core::result::Result<MemoryRange, Error> {
        Ok(MemoryRange {
            addr: MemoryAddress::new(addr).ok_or(Error::BadAddress)?,
            size: MemorySize::new(size).ok_or(Error::BadAddress)?,
        })
    }

    pub fn len(&self) -> usize { self.size.get() }

    pub fn is_empty(&self) -> bool { self.size.get() > 0 }

    pub fn as_ptr(&self) -> *const u8 { self.addr.get() as *const u8 }

    pub fn as_mut_ptr(&self) -> *mut u8 { self.addr.get() as *mut u8 }

    /// Return this memory as a slice of values. The resulting slice
    /// will cover the maximum number of elements given the size of `T`.
    /// For example, if the allocation is 4096 bytes, then the resulting
    /// `&[u8]` would have 4096 elements, `&[u16]` would have 2048, and
    /// `&[u32]` would have 1024. Values are rounded down.
    ///
    /// # Safety
    ///
    /// This is safe as long as the underlying memory is representable
    /// on the target system. For example, you must ensure that `bool`
    /// slices contain only `0` or `1`.
    pub unsafe fn as_slice<T>(&self) -> &[T] {
        // This is safe because the pointer and length are guaranteed to
        // be valid, as long as the user hasn't already called `as_ptr()`
        // and done something unsound with the resulting pointer.
        unsafe {
            core::slice::from_raw_parts(self.as_ptr() as *const T, self.len() / core::mem::size_of::<T>())
        }
    }

    /// Return this memory as a slice of mutable values. The resulting slice
    /// will cover the maximum number of elements given the size of `T`.
    /// For example, if the allocation is 4096 bytes, then the resulting
    /// `&[u8]` would have 4096 elements, `&[u16]` would have 2048, and
    /// `&[u32]` would have 1024. Values are rounded down.
    ///
    /// # Safety
    ///
    /// This is safe as long as the underlying memory is representable
    /// on the target system. For example, you must ensure that `bool`
    /// slices contain only `0` or `1`.
    pub unsafe fn as_slice_mut<T>(&mut self) -> &mut [T] {
        // This is safe because the pointer and length are guaranteed to
        // be valid, as long as the user hasn't already called `as_ptr()`
        // and done something unsound with the resulting pointer.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.as_mut_ptr() as *mut T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }
}
