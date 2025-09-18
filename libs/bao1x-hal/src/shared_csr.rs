use utralib::*;

/// Create an immutable-friendly shared CSR object for the I/O ports. The consequence
/// of this is that we don't get the borrow checker to check the global shared state of
/// the I/O pins status, but the truth is, it's global shared state and there's nothing
/// you can do about it. Might as well make the APIs cleaner so we have less work to
/// do maintaining APIs and can pay attention to sharing/allocating the shared state
/// correctly.
#[derive(Debug)]
pub struct SharedCsr<T> {
    pub base: *const T,
}
impl<T> SharedCsr<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *const T) -> Self { SharedCsr { base: base as *const T } }

    pub unsafe fn base(&self) -> *mut T { self.base as *mut T }

    pub fn clone(&self) -> Self { SharedCsr { base: self.base.clone() } }

    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        unsafe { usize_base.add(reg.offset()).read_volatile() }.try_into().unwrap_or_default()
    }

    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        ((unsafe { usize_base.add(field.register().offset()).read_volatile() } >> field.offset())
            & field.mask())
        .try_into()
        .unwrap_or_default()
    }

    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset();
        let previous = unsafe { usize_base.add(field.register().offset()).read_volatile() }
            & !(field.mask() << field.offset());
        unsafe { usize_base.add(field.register().offset()).write_volatile(previous | value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Write a given field without reading it first
    pub fn wfo(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask()) << field.offset();
        unsafe { usize_base.add(field.register().offset()).write_volatile(value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Write the entire contents of a register without reading it first
    pub fn wo(&self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset()).write_volatile(value_as_usize) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask() << field.offset())).try_into().unwrap_or_default()
    }

    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask()) << field.offset()).try_into().unwrap_or_default()
    }
}
