
#![allow(dead_code)]
use core::convert::TryInto;
#[cfg(feature="std")]
use core::sync::atomic::AtomicPtr;
#[cfg(feature="std")]
use std::sync::Arc;

#[derive(Debug, Copy, Clone)]
pub struct Register {
    /// Offset of this register within this CSR
    offset: usize,
    /// Mask of SVD-specified bits for the register
    mask: usize,
}
impl Register {
    pub const fn new(offset: usize, mask: usize) -> Register {
        Register { offset, mask }
    }
    pub const fn offset(&self) -> usize { self.offset }
    pub const fn mask(&self) -> usize { self.mask }
}
#[derive(Debug, Copy, Clone)]
pub struct Field {
    /// A bitmask we use to AND to the value, unshifted.
    /// E.g. for a width of `3` bits, this mask would be 0b111.
    mask: usize,
    /// Offset of the first bit in this field
    offset: usize,
    /// A copy of the register address that this field
    /// is a member of. Ideally this is optimized out by the
    /// compiler.
    register: Register,
}
impl Field {
    /// Define a new CSR field with the given width at a specified
    /// offset from the start of the register.
    pub const fn new(width: usize, offset: usize, register: Register) -> Field {
        let mask = if width < 32 { (1 << width) - 1 } else {0xFFFF_FFFF};
        Field {
            mask,
            offset,
            register,
        }
    }
    pub const fn offset(&self) -> usize { self.offset }
    pub const fn mask(&self) -> usize { self.mask }
}
#[derive(Debug, Copy, Clone)]
pub struct CSR<T> {
    base: *mut T,
}
impl<T> CSR<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *mut T) -> Self {
        CSR { base }
    }
    /// Retrieve the raw pointer used as the base of the CSR. This is unsafe because the copied
    /// value can be used to do all kinds of awful shared mutable operations (like creating
    /// another CSR accessor owned by another thread). However, sometimes this is unavoidable
    /// because hardware is in fact shared mutable state.
    pub unsafe fn base(&self) -> *mut T {
        self.base
    }
    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        unsafe { usize_base.add(reg.offset).read_volatile() }
            .try_into()
            .unwrap_or_default()
    }
    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        ((unsafe { usize_base.add(field.register.offset).read_volatile() } >> field.offset)
            & field.mask)
            .try_into()
            .unwrap_or_default()
    }
    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&mut self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset;
        let previous =
            unsafe { usize_base.add(field.register.offset).read_volatile() } & !(field.mask << field.offset);
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(previous | value_as_usize)
        };
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write a given field without reading it first
    pub fn wfo(&mut self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask) << field.offset;
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(value_as_usize)
        };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write the entire contents of a register without reading it first
    pub fn wo(&mut self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask << field.offset))
            .try_into()
            .unwrap_or_default()
    }
    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask) << field.offset)
            .try_into()
            .unwrap_or_default()
    }
}

#[derive(Debug)]
#[cfg(feature="std")]
pub struct AtomicCsr<T> {
    base: Arc::<AtomicPtr<T>>,
}
#[cfg(feature="std")]
impl<T> AtomicCsr<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    /// AtomicCsr wraps the CSR in an Arc + AtomicPtr, so that write operations don't require
    /// a mutable reference. This allows us to stick CSR accesses into APIs that require
    /// non-mutable references to hardware state (such as certain "standardized" USB APIs).
    /// Hiding the fact that you're tweaking hardware registers behind Arc/AtomicPtr seems a little
    /// scary, but, it does make for nicer Rust semantics.
    pub fn new(base: *mut T) -> Self {
        AtomicCsr {
            base: Arc::new(AtomicPtr::new(base))
        }
    }
    pub fn clone(&self) -> Self {
        AtomicCsr {
            base: self.base.clone()
        }
    }
    /// Read the contents of this register
    pub fn r(&self, reg: Register) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        unsafe { usize_base.add(reg.offset).read_volatile() }
            .try_into()
            .unwrap_or_default()
    }
    /// Read a field from this CSR
    pub fn rf(&self, field: Field) -> T {
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        ((unsafe { usize_base.add(field.register.offset).read_volatile() } >> field.offset)
            & field.mask)
            .try_into()
            .unwrap_or_default()
    }
    /// Read-modify-write a given field in this CSR
    pub fn rmwf(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset;
        let previous =
            unsafe { usize_base.add(field.register.offset).read_volatile() } & !(field.mask << field.offset);
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(previous | value_as_usize)
        };
        // prevent re-ordering
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write a given field without reading it first
    pub fn wfo(&self, field: Field, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask) << field.offset;
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(value_as_usize)
        };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Write the entire contents of a register without reading it first
    pub fn wo(&self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base.load(core::sync::atomic::Ordering::SeqCst)) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
        // Ensure the compiler doesn't re-order the write.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    /// Zero a field from a provided value
    pub fn zf(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask << field.offset))
            .try_into()
            .unwrap_or_default()
    }
    /// Shift & mask a value to its final field position
    pub fn ms(&self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask) << field.offset)
            .try_into()
            .unwrap_or_default()
    }
}
// Physical base addresses of memory regions
pub const HW_RP_PIO_MEM:     usize = 0x50123000;
pub const HW_RP_PIO_MEM_LEN: usize = 4096;

// Physical base addresses of registers
pub const HW_RP_PIO_BASE :   usize = 0x50123000;


pub mod utra {

    pub mod rp_pio {
        pub const RP_PIO_NUMREGS: usize = 104;

        pub const SFR_CTRL: crate::Register = crate::Register::new(0, 0xfff);
        pub const SFR_CTRL_EN: crate::Field = crate::Field::new(4, 0, SFR_CTRL);
        pub const SFR_CTRL_RESTART: crate::Field = crate::Field::new(4, 4, SFR_CTRL);
        pub const SFR_CTRL_CLKDIV_RESTART: crate::Field = crate::Field::new(4, 8, SFR_CTRL);

        pub const SFR_FSTAT: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const SFR_FSTAT_RX_FULL: crate::Field = crate::Field::new(4, 0, SFR_FSTAT);
        pub const SFR_FSTAT_CONSTANT0: crate::Field = crate::Field::new(4, 4, SFR_FSTAT);
        pub const SFR_FSTAT_RX_EMPTY: crate::Field = crate::Field::new(4, 8, SFR_FSTAT);
        pub const SFR_FSTAT_CONSTANT1: crate::Field = crate::Field::new(4, 12, SFR_FSTAT);
        pub const SFR_FSTAT_TX_FULL: crate::Field = crate::Field::new(4, 16, SFR_FSTAT);
        pub const SFR_FSTAT_CONSTANT2: crate::Field = crate::Field::new(4, 20, SFR_FSTAT);
        pub const SFR_FSTAT_TX_EMPTY: crate::Field = crate::Field::new(4, 24, SFR_FSTAT);
        pub const SFR_FSTAT_CONSTANT3: crate::Field = crate::Field::new(4, 28, SFR_FSTAT);

        pub const SFR_FDEBUG: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const SFR_FDEBUG_RXSTALL: crate::Field = crate::Field::new(4, 0, SFR_FDEBUG);
        pub const SFR_FDEBUG_NC_DBG3: crate::Field = crate::Field::new(4, 4, SFR_FDEBUG);
        pub const SFR_FDEBUG_RXUNDER: crate::Field = crate::Field::new(4, 8, SFR_FDEBUG);
        pub const SFR_FDEBUG_NC_DBG2: crate::Field = crate::Field::new(4, 12, SFR_FDEBUG);
        pub const SFR_FDEBUG_TXOVER: crate::Field = crate::Field::new(4, 16, SFR_FDEBUG);
        pub const SFR_FDEBUG_NC_DBG1: crate::Field = crate::Field::new(4, 20, SFR_FDEBUG);
        pub const SFR_FDEBUG_TXSTALL: crate::Field = crate::Field::new(4, 24, SFR_FDEBUG);
        pub const SFR_FDEBUG_NC_DBG0: crate::Field = crate::Field::new(4, 28, SFR_FDEBUG);

        pub const SFR_FLEVEL: crate::Register = crate::Register::new(3, 0xffffffff);
        pub const SFR_FLEVEL_TX_LEVEL0: crate::Field = crate::Field::new(3, 0, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT0: crate::Field = crate::Field::new(1, 3, SFR_FLEVEL);
        pub const SFR_FLEVEL_RX_LEVEL0: crate::Field = crate::Field::new(3, 4, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT1: crate::Field = crate::Field::new(1, 7, SFR_FLEVEL);
        pub const SFR_FLEVEL_TX_LEVEL1: crate::Field = crate::Field::new(3, 8, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT2: crate::Field = crate::Field::new(1, 11, SFR_FLEVEL);
        pub const SFR_FLEVEL_RX_LEVEL1: crate::Field = crate::Field::new(3, 12, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT3: crate::Field = crate::Field::new(1, 15, SFR_FLEVEL);
        pub const SFR_FLEVEL_TX_LEVEL2: crate::Field = crate::Field::new(3, 16, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT4: crate::Field = crate::Field::new(1, 19, SFR_FLEVEL);
        pub const SFR_FLEVEL_RX_LEVEL2: crate::Field = crate::Field::new(3, 20, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT5: crate::Field = crate::Field::new(1, 23, SFR_FLEVEL);
        pub const SFR_FLEVEL_TX_LEVEL3: crate::Field = crate::Field::new(3, 24, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT6: crate::Field = crate::Field::new(1, 27, SFR_FLEVEL);
        pub const SFR_FLEVEL_RX_LEVEL3: crate::Field = crate::Field::new(3, 28, SFR_FLEVEL);
        pub const SFR_FLEVEL_CONSTANT7: crate::Field = crate::Field::new(1, 31, SFR_FLEVEL);

        pub const SFR_TXF0: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const SFR_TXF0_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF0);

        pub const SFR_TXF1: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const SFR_TXF1_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF1);

        pub const SFR_TXF2: crate::Register = crate::Register::new(6, 0xffffffff);
        pub const SFR_TXF2_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF2);

        pub const SFR_TXF3: crate::Register = crate::Register::new(7, 0xffffffff);
        pub const SFR_TXF3_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF3);

        pub const SFR_RXF0: crate::Register = crate::Register::new(8, 0xffffffff);
        pub const SFR_RXF0_PDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF0);

        pub const SFR_RXF1: crate::Register = crate::Register::new(9, 0xffffffff);
        pub const SFR_RXF1_PDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF1);

        pub const SFR_RXF2: crate::Register = crate::Register::new(10, 0xffffffff);
        pub const SFR_RXF2_PDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF2);

        pub const SFR_RXF3: crate::Register = crate::Register::new(11, 0xffffffff);
        pub const SFR_RXF3_PDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF3);

        pub const SFR_IRQ: crate::Register = crate::Register::new(12, 0xff);
        pub const SFR_IRQ_SFR_IRQ: crate::Field = crate::Field::new(8, 0, SFR_IRQ);

        pub const SFR_IRQ_FORCE: crate::Register = crate::Register::new(13, 0xff);
        pub const SFR_IRQ_FORCE_SFR_IRQ_FORCE: crate::Field = crate::Field::new(8, 0, SFR_IRQ_FORCE);

        pub const SFR_SYNC_BYPASS: crate::Register = crate::Register::new(14, 0xffffffff);
        pub const SFR_SYNC_BYPASS_SFR_SYNC_BYPASS: crate::Field = crate::Field::new(32, 0, SFR_SYNC_BYPASS);

        pub const SFR_DBG_PADOUT: crate::Register = crate::Register::new(15, 0xffffffff);
        pub const SFR_DBG_PADOUT_SFR_DBG_PADOUT: crate::Field = crate::Field::new(32, 0, SFR_DBG_PADOUT);

        pub const SFR_DBG_PADOE: crate::Register = crate::Register::new(16, 0xffffffff);
        pub const SFR_DBG_PADOE_SFR_DBG_PADOE: crate::Field = crate::Field::new(32, 0, SFR_DBG_PADOE);

        pub const SFR_DBG_CFGINFO: crate::Register = crate::Register::new(17, 0xffffffff);
        pub const SFR_DBG_CFGINFO_CONSTANT0: crate::Field = crate::Field::new(8, 0, SFR_DBG_CFGINFO);
        pub const SFR_DBG_CFGINFO_CONSTANT1: crate::Field = crate::Field::new(8, 8, SFR_DBG_CFGINFO);
        pub const SFR_DBG_CFGINFO_CONSTANT2: crate::Field = crate::Field::new(16, 16, SFR_DBG_CFGINFO);

        pub const SFR_INSTR_MEM0: crate::Register = crate::Register::new(18, 0xffff);
        pub const SFR_INSTR_MEM0_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM0);

        pub const SFR_INSTR_MEM1: crate::Register = crate::Register::new(19, 0xffff);
        pub const SFR_INSTR_MEM1_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM1);

        pub const SFR_INSTR_MEM2: crate::Register = crate::Register::new(20, 0xffff);
        pub const SFR_INSTR_MEM2_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM2);

        pub const SFR_INSTR_MEM3: crate::Register = crate::Register::new(21, 0xffff);
        pub const SFR_INSTR_MEM3_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM3);

        pub const SFR_INSTR_MEM4: crate::Register = crate::Register::new(22, 0xffff);
        pub const SFR_INSTR_MEM4_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM4);

        pub const SFR_INSTR_MEM5: crate::Register = crate::Register::new(23, 0xffff);
        pub const SFR_INSTR_MEM5_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM5);

        pub const SFR_INSTR_MEM6: crate::Register = crate::Register::new(24, 0xffff);
        pub const SFR_INSTR_MEM6_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM6);

        pub const SFR_INSTR_MEM7: crate::Register = crate::Register::new(25, 0xffff);
        pub const SFR_INSTR_MEM7_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM7);

        pub const SFR_INSTR_MEM8: crate::Register = crate::Register::new(26, 0xffff);
        pub const SFR_INSTR_MEM8_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM8);

        pub const SFR_INSTR_MEM9: crate::Register = crate::Register::new(27, 0xffff);
        pub const SFR_INSTR_MEM9_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM9);

        pub const SFR_INSTR_MEM10: crate::Register = crate::Register::new(28, 0xffff);
        pub const SFR_INSTR_MEM10_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM10);

        pub const SFR_INSTR_MEM11: crate::Register = crate::Register::new(29, 0xffff);
        pub const SFR_INSTR_MEM11_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM11);

        pub const SFR_INSTR_MEM12: crate::Register = crate::Register::new(30, 0xffff);
        pub const SFR_INSTR_MEM12_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM12);

        pub const SFR_INSTR_MEM13: crate::Register = crate::Register::new(31, 0xffff);
        pub const SFR_INSTR_MEM13_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM13);

        pub const SFR_INSTR_MEM14: crate::Register = crate::Register::new(32, 0xffff);
        pub const SFR_INSTR_MEM14_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM14);

        pub const SFR_INSTR_MEM15: crate::Register = crate::Register::new(33, 0xffff);
        pub const SFR_INSTR_MEM15_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM15);

        pub const SFR_INSTR_MEM16: crate::Register = crate::Register::new(34, 0xffff);
        pub const SFR_INSTR_MEM16_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM16);

        pub const SFR_INSTR_MEM17: crate::Register = crate::Register::new(35, 0xffff);
        pub const SFR_INSTR_MEM17_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM17);

        pub const SFR_INSTR_MEM18: crate::Register = crate::Register::new(36, 0xffff);
        pub const SFR_INSTR_MEM18_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM18);

        pub const SFR_INSTR_MEM19: crate::Register = crate::Register::new(37, 0xffff);
        pub const SFR_INSTR_MEM19_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM19);

        pub const SFR_INSTR_MEM20: crate::Register = crate::Register::new(38, 0xffff);
        pub const SFR_INSTR_MEM20_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM20);

        pub const SFR_INSTR_MEM21: crate::Register = crate::Register::new(39, 0xffff);
        pub const SFR_INSTR_MEM21_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM21);

        pub const SFR_INSTR_MEM22: crate::Register = crate::Register::new(40, 0xffff);
        pub const SFR_INSTR_MEM22_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM22);

        pub const SFR_INSTR_MEM23: crate::Register = crate::Register::new(41, 0xffff);
        pub const SFR_INSTR_MEM23_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM23);

        pub const SFR_INSTR_MEM24: crate::Register = crate::Register::new(42, 0xffff);
        pub const SFR_INSTR_MEM24_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM24);

        pub const SFR_INSTR_MEM25: crate::Register = crate::Register::new(43, 0xffff);
        pub const SFR_INSTR_MEM25_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM25);

        pub const SFR_INSTR_MEM26: crate::Register = crate::Register::new(44, 0xffff);
        pub const SFR_INSTR_MEM26_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM26);

        pub const SFR_INSTR_MEM27: crate::Register = crate::Register::new(45, 0xffff);
        pub const SFR_INSTR_MEM27_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM27);

        pub const SFR_INSTR_MEM28: crate::Register = crate::Register::new(46, 0xffff);
        pub const SFR_INSTR_MEM28_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM28);

        pub const SFR_INSTR_MEM29: crate::Register = crate::Register::new(47, 0xffff);
        pub const SFR_INSTR_MEM29_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM29);

        pub const SFR_INSTR_MEM30: crate::Register = crate::Register::new(48, 0xffff);
        pub const SFR_INSTR_MEM30_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM30);

        pub const SFR_INSTR_MEM31: crate::Register = crate::Register::new(49, 0xffff);
        pub const SFR_INSTR_MEM31_INSTR: crate::Field = crate::Field::new(16, 0, SFR_INSTR_MEM31);

        pub const SFR_SM0_CLKDIV: crate::Register = crate::Register::new(50, 0xffffffff);
        pub const SFR_SM0_CLKDIV_UNUSED_DIV: crate::Field = crate::Field::new(8, 0, SFR_SM0_CLKDIV);
        pub const SFR_SM0_CLKDIV_DIV_FRAC: crate::Field = crate::Field::new(8, 8, SFR_SM0_CLKDIV);
        pub const SFR_SM0_CLKDIV_DIV_INT: crate::Field = crate::Field::new(16, 16, SFR_SM0_CLKDIV);

        pub const SFR_SM0_EXECCTRL: crate::Register = crate::Register::new(51, 0xffffffff);
        pub const SFR_SM0_EXECCTRL_STATUS_N: crate::Field = crate::Field::new(4, 0, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_STATUS_SEL: crate::Field = crate::Field::new(1, 4, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_RESVD_EXEC: crate::Field = crate::Field::new(2, 5, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_WRAP_TARGET: crate::Field = crate::Field::new(5, 7, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_PEND: crate::Field = crate::Field::new(5, 12, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_OUT_STICKY: crate::Field = crate::Field::new(1, 17, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_INLINE_OUT_EN: crate::Field = crate::Field::new(1, 18, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_OUT_EN_SEL: crate::Field = crate::Field::new(5, 19, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_JMP_PIN: crate::Field = crate::Field::new(5, 24, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_SIDE_PINDIR: crate::Field = crate::Field::new(1, 29, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT: crate::Field = crate::Field::new(1, 30, SFR_SM0_EXECCTRL);
        pub const SFR_SM0_EXECCTRL_EXEC_STALLED_RO0: crate::Field = crate::Field::new(1, 31, SFR_SM0_EXECCTRL);

        pub const SFR_SM0_SHIFTCTRL: crate::Register = crate::Register::new(52, 0xffffffff);
        pub const SFR_SM0_SHIFTCTRL_RESVD_SHIFT: crate::Field = crate::Field::new(16, 0, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_AUTO_PUSH: crate::Field = crate::Field::new(1, 16, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_AUTO_PULL: crate::Field = crate::Field::new(1, 17, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR: crate::Field = crate::Field::new(1, 18, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR: crate::Field = crate::Field::new(1, 19, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_ISR_THRESHOLD: crate::Field = crate::Field::new(5, 20, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_OSR_THRESHOLD: crate::Field = crate::Field::new(5, 25, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_JOIN_TX: crate::Field = crate::Field::new(1, 30, SFR_SM0_SHIFTCTRL);
        pub const SFR_SM0_SHIFTCTRL_JOIN_RX: crate::Field = crate::Field::new(1, 31, SFR_SM0_SHIFTCTRL);

        pub const SFR_SM0_ADDR: crate::Register = crate::Register::new(53, 0x1f);
        pub const SFR_SM0_ADDR_PC: crate::Field = crate::Field::new(5, 0, SFR_SM0_ADDR);

        pub const SFR_SM0_INSTR: crate::Register = crate::Register::new(54, 0xffff);
        pub const SFR_SM0_INSTR_IMM_INSTR: crate::Field = crate::Field::new(16, 0, SFR_SM0_INSTR);

        pub const SFR_SM0_PINCTRL: crate::Register = crate::Register::new(55, 0xffffffff);
        pub const SFR_SM0_PINCTRL_PINS_OUT_BASE: crate::Field = crate::Field::new(5, 0, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_SET_BASE: crate::Field = crate::Field::new(5, 5, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_SIDE_BASE: crate::Field = crate::Field::new(5, 10, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_IN_BASE: crate::Field = crate::Field::new(5, 15, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_OUT_COUNT: crate::Field = crate::Field::new(6, 20, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_SET_COUNT: crate::Field = crate::Field::new(3, 26, SFR_SM0_PINCTRL);
        pub const SFR_SM0_PINCTRL_PINS_SIDE_COUNT: crate::Field = crate::Field::new(3, 29, SFR_SM0_PINCTRL);

        pub const SFR_SM1_CLKDIV: crate::Register = crate::Register::new(56, 0xffffffff);
        pub const SFR_SM1_CLKDIV_UNUSED_DIV: crate::Field = crate::Field::new(8, 0, SFR_SM1_CLKDIV);
        pub const SFR_SM1_CLKDIV_DIV_FRAC: crate::Field = crate::Field::new(8, 8, SFR_SM1_CLKDIV);
        pub const SFR_SM1_CLKDIV_DIV_INT: crate::Field = crate::Field::new(16, 16, SFR_SM1_CLKDIV);

        pub const SFR_SM1_EXECCTRL: crate::Register = crate::Register::new(57, 0xffffffff);
        pub const SFR_SM1_EXECCTRL_STATUS_N: crate::Field = crate::Field::new(4, 0, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_STATUS_SEL: crate::Field = crate::Field::new(1, 4, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_RESVD_EXEC: crate::Field = crate::Field::new(2, 5, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_WRAP_TARGET: crate::Field = crate::Field::new(5, 7, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_PEND: crate::Field = crate::Field::new(5, 12, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_OUT_STICKY: crate::Field = crate::Field::new(1, 17, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_INLINE_OUT_EN: crate::Field = crate::Field::new(1, 18, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_OUT_EN_SEL: crate::Field = crate::Field::new(5, 19, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_JMP_PIN: crate::Field = crate::Field::new(5, 24, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_SIDE_PINDIR: crate::Field = crate::Field::new(1, 29, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT: crate::Field = crate::Field::new(1, 30, SFR_SM1_EXECCTRL);
        pub const SFR_SM1_EXECCTRL_EXEC_STALLED_RO1: crate::Field = crate::Field::new(1, 31, SFR_SM1_EXECCTRL);

        pub const SFR_SM1_SHIFTCTRL: crate::Register = crate::Register::new(58, 0xffffffff);
        pub const SFR_SM1_SHIFTCTRL_RESVD_SHIFT: crate::Field = crate::Field::new(16, 0, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_AUTO_PUSH: crate::Field = crate::Field::new(1, 16, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_AUTO_PULL: crate::Field = crate::Field::new(1, 17, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR: crate::Field = crate::Field::new(1, 18, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR: crate::Field = crate::Field::new(1, 19, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_ISR_THRESHOLD: crate::Field = crate::Field::new(5, 20, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_OSR_THRESHOLD: crate::Field = crate::Field::new(5, 25, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_JOIN_TX: crate::Field = crate::Field::new(1, 30, SFR_SM1_SHIFTCTRL);
        pub const SFR_SM1_SHIFTCTRL_JOIN_RX: crate::Field = crate::Field::new(1, 31, SFR_SM1_SHIFTCTRL);

        pub const SFR_SM1_ADDR: crate::Register = crate::Register::new(59, 0x1f);
        pub const SFR_SM1_ADDR_PC: crate::Field = crate::Field::new(5, 0, SFR_SM1_ADDR);

        pub const SFR_SM1_INSTR: crate::Register = crate::Register::new(60, 0xffff);
        pub const SFR_SM1_INSTR_IMM_INSTR: crate::Field = crate::Field::new(16, 0, SFR_SM1_INSTR);

        pub const SFR_SM1_PINCTRL: crate::Register = crate::Register::new(61, 0xffffffff);
        pub const SFR_SM1_PINCTRL_PINS_OUT_BASE: crate::Field = crate::Field::new(5, 0, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_SET_BASE: crate::Field = crate::Field::new(5, 5, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_SIDE_BASE: crate::Field = crate::Field::new(5, 10, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_IN_BASE: crate::Field = crate::Field::new(5, 15, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_OUT_COUNT: crate::Field = crate::Field::new(6, 20, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_SET_COUNT: crate::Field = crate::Field::new(3, 26, SFR_SM1_PINCTRL);
        pub const SFR_SM1_PINCTRL_PINS_SIDE_COUNT: crate::Field = crate::Field::new(3, 29, SFR_SM1_PINCTRL);

        pub const SFR_SM2_CLKDIV: crate::Register = crate::Register::new(62, 0xffffffff);
        pub const SFR_SM2_CLKDIV_UNUSED_DIV: crate::Field = crate::Field::new(8, 0, SFR_SM2_CLKDIV);
        pub const SFR_SM2_CLKDIV_DIV_FRAC: crate::Field = crate::Field::new(8, 8, SFR_SM2_CLKDIV);
        pub const SFR_SM2_CLKDIV_DIV_INT: crate::Field = crate::Field::new(16, 16, SFR_SM2_CLKDIV);

        pub const SFR_SM2_EXECCTRL: crate::Register = crate::Register::new(63, 0xffffffff);
        pub const SFR_SM2_EXECCTRL_STATUS_N: crate::Field = crate::Field::new(4, 0, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_STATUS_SEL: crate::Field = crate::Field::new(1, 4, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_RESVD_EXEC: crate::Field = crate::Field::new(2, 5, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_WRAP_TARGET: crate::Field = crate::Field::new(5, 7, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_PEND: crate::Field = crate::Field::new(5, 12, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_OUT_STICKY: crate::Field = crate::Field::new(1, 17, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_INLINE_OUT_EN: crate::Field = crate::Field::new(1, 18, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_OUT_EN_SEL: crate::Field = crate::Field::new(5, 19, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_JMP_PIN: crate::Field = crate::Field::new(5, 24, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_SIDE_PINDIR: crate::Field = crate::Field::new(1, 29, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT: crate::Field = crate::Field::new(1, 30, SFR_SM2_EXECCTRL);
        pub const SFR_SM2_EXECCTRL_EXEC_STALLED_RO2: crate::Field = crate::Field::new(1, 31, SFR_SM2_EXECCTRL);

        pub const SFR_SM2_SHIFTCTRL: crate::Register = crate::Register::new(64, 0xffffffff);
        pub const SFR_SM2_SHIFTCTRL_RESVD_SHIFT: crate::Field = crate::Field::new(16, 0, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_AUTO_PUSH: crate::Field = crate::Field::new(1, 16, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_AUTO_PULL: crate::Field = crate::Field::new(1, 17, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR: crate::Field = crate::Field::new(1, 18, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR: crate::Field = crate::Field::new(1, 19, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_ISR_THRESHOLD: crate::Field = crate::Field::new(5, 20, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_OSR_THRESHOLD: crate::Field = crate::Field::new(5, 25, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_JOIN_TX: crate::Field = crate::Field::new(1, 30, SFR_SM2_SHIFTCTRL);
        pub const SFR_SM2_SHIFTCTRL_JOIN_RX: crate::Field = crate::Field::new(1, 31, SFR_SM2_SHIFTCTRL);

        pub const SFR_SM2_ADDR: crate::Register = crate::Register::new(65, 0x1f);
        pub const SFR_SM2_ADDR_PC: crate::Field = crate::Field::new(5, 0, SFR_SM2_ADDR);

        pub const SFR_SM2_INSTR: crate::Register = crate::Register::new(66, 0xffff);
        pub const SFR_SM2_INSTR_IMM_INSTR: crate::Field = crate::Field::new(16, 0, SFR_SM2_INSTR);

        pub const SFR_SM2_PINCTRL: crate::Register = crate::Register::new(67, 0xffffffff);
        pub const SFR_SM2_PINCTRL_PINS_OUT_BASE: crate::Field = crate::Field::new(5, 0, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_SET_BASE: crate::Field = crate::Field::new(5, 5, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_SIDE_BASE: crate::Field = crate::Field::new(5, 10, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_IN_BASE: crate::Field = crate::Field::new(5, 15, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_OUT_COUNT: crate::Field = crate::Field::new(6, 20, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_SET_COUNT: crate::Field = crate::Field::new(3, 26, SFR_SM2_PINCTRL);
        pub const SFR_SM2_PINCTRL_PINS_SIDE_COUNT: crate::Field = crate::Field::new(3, 29, SFR_SM2_PINCTRL);

        pub const SFR_SM3_CLKDIV: crate::Register = crate::Register::new(68, 0xffffffff);
        pub const SFR_SM3_CLKDIV_UNUSED_DIV: crate::Field = crate::Field::new(8, 0, SFR_SM3_CLKDIV);
        pub const SFR_SM3_CLKDIV_DIV_FRAC: crate::Field = crate::Field::new(8, 8, SFR_SM3_CLKDIV);
        pub const SFR_SM3_CLKDIV_DIV_INT: crate::Field = crate::Field::new(16, 16, SFR_SM3_CLKDIV);

        pub const SFR_SM3_EXECCTRL: crate::Register = crate::Register::new(69, 0xffffffff);
        pub const SFR_SM3_EXECCTRL_STATUS_N: crate::Field = crate::Field::new(4, 0, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_STATUS_SEL: crate::Field = crate::Field::new(1, 4, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_RESVD_EXEC: crate::Field = crate::Field::new(2, 5, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_WRAP_TARGET: crate::Field = crate::Field::new(5, 7, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_PEND: crate::Field = crate::Field::new(5, 12, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_OUT_STICKY: crate::Field = crate::Field::new(1, 17, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_INLINE_OUT_EN: crate::Field = crate::Field::new(1, 18, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_OUT_EN_SEL: crate::Field = crate::Field::new(5, 19, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_JMP_PIN: crate::Field = crate::Field::new(5, 24, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_SIDE_PINDIR: crate::Field = crate::Field::new(1, 29, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT: crate::Field = crate::Field::new(1, 30, SFR_SM3_EXECCTRL);
        pub const SFR_SM3_EXECCTRL_EXEC_STALLED_RO3: crate::Field = crate::Field::new(1, 31, SFR_SM3_EXECCTRL);

        pub const SFR_SM3_SHIFTCTRL: crate::Register = crate::Register::new(70, 0xffffffff);
        pub const SFR_SM3_SHIFTCTRL_RESVD_SHIFT: crate::Field = crate::Field::new(16, 0, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_AUTO_PUSH: crate::Field = crate::Field::new(1, 16, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_AUTO_PULL: crate::Field = crate::Field::new(1, 17, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR: crate::Field = crate::Field::new(1, 18, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR: crate::Field = crate::Field::new(1, 19, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_ISR_THRESHOLD: crate::Field = crate::Field::new(5, 20, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_OSR_THRESHOLD: crate::Field = crate::Field::new(5, 25, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_JOIN_TX: crate::Field = crate::Field::new(1, 30, SFR_SM3_SHIFTCTRL);
        pub const SFR_SM3_SHIFTCTRL_JOIN_RX: crate::Field = crate::Field::new(1, 31, SFR_SM3_SHIFTCTRL);

        pub const SFR_SM3_ADDR: crate::Register = crate::Register::new(71, 0x1f);
        pub const SFR_SM3_ADDR_PC: crate::Field = crate::Field::new(5, 0, SFR_SM3_ADDR);

        pub const SFR_SM3_INSTR: crate::Register = crate::Register::new(72, 0xffff);
        pub const SFR_SM3_INSTR_IMM_INSTR: crate::Field = crate::Field::new(16, 0, SFR_SM3_INSTR);

        pub const SFR_SM3_PINCTRL: crate::Register = crate::Register::new(73, 0xffffffff);
        pub const SFR_SM3_PINCTRL_PINS_OUT_BASE: crate::Field = crate::Field::new(5, 0, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_SET_BASE: crate::Field = crate::Field::new(5, 5, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_SIDE_BASE: crate::Field = crate::Field::new(5, 10, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_IN_BASE: crate::Field = crate::Field::new(5, 15, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_OUT_COUNT: crate::Field = crate::Field::new(6, 20, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_SET_COUNT: crate::Field = crate::Field::new(3, 26, SFR_SM3_PINCTRL);
        pub const SFR_SM3_PINCTRL_PINS_SIDE_COUNT: crate::Field = crate::Field::new(3, 29, SFR_SM3_PINCTRL);

        pub const SFR_INTR: crate::Register = crate::Register::new(74, 0xfff);
        pub const SFR_INTR_INTR_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_INTR);
        pub const SFR_INTR_INTR_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_INTR);
        pub const SFR_INTR_INTR_SM: crate::Field = crate::Field::new(4, 8, SFR_INTR);

        pub const SFR_IRQ0_INTE: crate::Register = crate::Register::new(75, 0xfff);
        pub const SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ0_INTE);
        pub const SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ0_INTE);
        pub const SFR_IRQ0_INTE_IRQ0_INTE_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ0_INTE);

        pub const SFR_IRQ0_INTF: crate::Register = crate::Register::new(76, 0xfff);
        pub const SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ0_INTF);
        pub const SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ0_INTF);
        pub const SFR_IRQ0_INTF_IRQ0_INTF_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ0_INTF);

        pub const SFR_IRQ0_INTS: crate::Register = crate::Register::new(77, 0xfff);
        pub const SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ0_INTS);
        pub const SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ0_INTS);
        pub const SFR_IRQ0_INTS_IRQ0_INTS_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ0_INTS);

        pub const SFR_IRQ1_INTE: crate::Register = crate::Register::new(78, 0xfff);
        pub const SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ1_INTE);
        pub const SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ1_INTE);
        pub const SFR_IRQ1_INTE_IRQ1_INTE_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ1_INTE);

        pub const SFR_IRQ1_INTF: crate::Register = crate::Register::new(79, 0xfff);
        pub const SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ1_INTF);
        pub const SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ1_INTF);
        pub const SFR_IRQ1_INTF_IRQ1_INTF_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ1_INTF);

        pub const SFR_IRQ1_INTS: crate::Register = crate::Register::new(80, 0xfff);
        pub const SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY: crate::Field = crate::Field::new(4, 0, SFR_IRQ1_INTS);
        pub const SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL: crate::Field = crate::Field::new(4, 4, SFR_IRQ1_INTS);
        pub const SFR_IRQ1_INTS_IRQ1_INTS_SM: crate::Field = crate::Field::new(4, 8, SFR_IRQ1_INTS);

        pub const RESERVED81: crate::Register = crate::Register::new(81, 0x1);
        pub const RESERVED81_RESERVED81: crate::Field = crate::Field::new(1, 0, RESERVED81);

        pub const RESERVED82: crate::Register = crate::Register::new(82, 0x1);
        pub const RESERVED82_RESERVED82: crate::Field = crate::Field::new(1, 0, RESERVED82);

        pub const RESERVED83: crate::Register = crate::Register::new(83, 0x1);
        pub const RESERVED83_RESERVED83: crate::Field = crate::Field::new(1, 0, RESERVED83);

        pub const RESERVED84: crate::Register = crate::Register::new(84, 0x1);
        pub const RESERVED84_RESERVED84: crate::Field = crate::Field::new(1, 0, RESERVED84);

        pub const RESERVED85: crate::Register = crate::Register::new(85, 0x1);
        pub const RESERVED85_RESERVED85: crate::Field = crate::Field::new(1, 0, RESERVED85);

        pub const RESERVED86: crate::Register = crate::Register::new(86, 0x1);
        pub const RESERVED86_RESERVED86: crate::Field = crate::Field::new(1, 0, RESERVED86);

        pub const RESERVED87: crate::Register = crate::Register::new(87, 0x1);
        pub const RESERVED87_RESERVED87: crate::Field = crate::Field::new(1, 0, RESERVED87);

        pub const RESERVED88: crate::Register = crate::Register::new(88, 0x1);
        pub const RESERVED88_RESERVED88: crate::Field = crate::Field::new(1, 0, RESERVED88);

        pub const RESERVED89: crate::Register = crate::Register::new(89, 0x1);
        pub const RESERVED89_RESERVED89: crate::Field = crate::Field::new(1, 0, RESERVED89);

        pub const RESERVED90: crate::Register = crate::Register::new(90, 0x1);
        pub const RESERVED90_RESERVED90: crate::Field = crate::Field::new(1, 0, RESERVED90);

        pub const RESERVED91: crate::Register = crate::Register::new(91, 0x1);
        pub const RESERVED91_RESERVED91: crate::Field = crate::Field::new(1, 0, RESERVED91);

        pub const RESERVED92: crate::Register = crate::Register::new(92, 0x1);
        pub const RESERVED92_RESERVED92: crate::Field = crate::Field::new(1, 0, RESERVED92);

        pub const RESERVED93: crate::Register = crate::Register::new(93, 0x1);
        pub const RESERVED93_RESERVED93: crate::Field = crate::Field::new(1, 0, RESERVED93);

        pub const RESERVED94: crate::Register = crate::Register::new(94, 0x1);
        pub const RESERVED94_RESERVED94: crate::Field = crate::Field::new(1, 0, RESERVED94);

        pub const RESERVED95: crate::Register = crate::Register::new(95, 0x1);
        pub const RESERVED95_RESERVED95: crate::Field = crate::Field::new(1, 0, RESERVED95);

        pub const SFR_IO_OE_INV: crate::Register = crate::Register::new(96, 0xffffffff);
        pub const SFR_IO_OE_INV_SFR_IO_OE_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_OE_INV);

        pub const SFR_IO_O_INV: crate::Register = crate::Register::new(97, 0xffffffff);
        pub const SFR_IO_O_INV_SFR_IO_O_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_O_INV);

        pub const SFR_IO_I_INV: crate::Register = crate::Register::new(98, 0xffffffff);
        pub const SFR_IO_I_INV_SFR_IO_I_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_I_INV);

        pub const SFR_FIFO_MARGIN: crate::Register = crate::Register::new(99, 0xffff);
        pub const SFR_FIFO_MARGIN_FIFO_TX_MARGIN0: crate::Field = crate::Field::new(2, 0, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_RX_MARGIN0: crate::Field = crate::Field::new(2, 2, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_TX_MARGIN1: crate::Field = crate::Field::new(2, 4, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_RX_MARGIN1: crate::Field = crate::Field::new(2, 6, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_TX_MARGIN2: crate::Field = crate::Field::new(2, 8, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_RX_MARGIN2: crate::Field = crate::Field::new(2, 10, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_TX_MARGIN3: crate::Field = crate::Field::new(2, 12, SFR_FIFO_MARGIN);
        pub const SFR_FIFO_MARGIN_FIFO_RX_MARGIN3: crate::Field = crate::Field::new(2, 14, SFR_FIFO_MARGIN);

        pub const SFR_ZERO0: crate::Register = crate::Register::new(100, 0xffffffff);
        pub const SFR_ZERO0_SFR_ZERO0: crate::Field = crate::Field::new(32, 0, SFR_ZERO0);

        pub const SFR_ZERO1: crate::Register = crate::Register::new(101, 0xffffffff);
        pub const SFR_ZERO1_SFR_ZERO1: crate::Field = crate::Field::new(32, 0, SFR_ZERO1);

        pub const SFR_ZERO2: crate::Register = crate::Register::new(102, 0xffffffff);
        pub const SFR_ZERO2_SFR_ZERO2: crate::Field = crate::Field::new(32, 0, SFR_ZERO2);

        pub const SFR_ZERO3: crate::Register = crate::Register::new(103, 0xffffffff);
        pub const SFR_ZERO3_SFR_ZERO3: crate::Field = crate::Field::new(32, 0, SFR_ZERO3);

        pub const HW_RP_PIO_BASE: usize = 0x50123000;
    }
}

// Litex auto-generated constants


#[cfg(test)]
mod tests {

    #[test]
    #[ignore]
    fn compile_check_rp_pio_csr() {
        use super::*;
        let mut rp_pio_csr = CSR::new(HW_RP_PIO_BASE as *mut u32);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_CTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_CTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_CTRL_EN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_CTRL_EN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_CTRL_EN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_CTRL_EN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_CTRL_EN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_CTRL_RESTART);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_CTRL_RESTART, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_CTRL_RESTART, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_CTRL_RESTART, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_CTRL_RESTART, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_CTRL_CLKDIV_RESTART);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_CTRL_CLKDIV_RESTART, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_CTRL_CLKDIV_RESTART, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_CTRL_CLKDIV_RESTART, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_CTRL_CLKDIV_RESTART, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_FSTAT);
        rp_pio_csr.wo(utra::rp_pio::SFR_FSTAT, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_RX_FULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_RX_FULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_RX_FULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_RX_FULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_RX_FULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_CONSTANT0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_CONSTANT0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_CONSTANT0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_CONSTANT0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_CONSTANT0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_RX_EMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_RX_EMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_RX_EMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_RX_EMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_RX_EMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_CONSTANT1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_CONSTANT1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_CONSTANT1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_CONSTANT1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_CONSTANT1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_TX_FULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_TX_FULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_TX_FULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_TX_FULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_TX_FULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_CONSTANT2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_CONSTANT2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_CONSTANT2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_CONSTANT2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_CONSTANT2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_TX_EMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_TX_EMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_TX_EMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_TX_EMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_TX_EMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FSTAT_CONSTANT3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FSTAT_CONSTANT3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FSTAT_CONSTANT3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FSTAT_CONSTANT3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FSTAT_CONSTANT3, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_FDEBUG);
        rp_pio_csr.wo(utra::rp_pio::SFR_FDEBUG, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_RXSTALL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_RXSTALL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_RXSTALL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_RXSTALL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_RXSTALL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_NC_DBG3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_NC_DBG3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_NC_DBG3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_NC_DBG3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_NC_DBG3, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_RXUNDER);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_RXUNDER, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_RXUNDER, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_RXUNDER, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_RXUNDER, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_NC_DBG2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_NC_DBG2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_NC_DBG2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_NC_DBG2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_NC_DBG2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_TXOVER);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_TXOVER, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_TXOVER, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_TXOVER, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_TXOVER, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_NC_DBG1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_NC_DBG1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_NC_DBG1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_NC_DBG1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_NC_DBG1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_TXSTALL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_TXSTALL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_TXSTALL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_TXSTALL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_TXSTALL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FDEBUG_NC_DBG0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FDEBUG_NC_DBG0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FDEBUG_NC_DBG0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FDEBUG_NC_DBG0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FDEBUG_NC_DBG0, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_FLEVEL);
        rp_pio_csr.wo(utra::rp_pio::SFR_FLEVEL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_TX_LEVEL0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_TX_LEVEL0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_RX_LEVEL0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_RX_LEVEL0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_TX_LEVEL1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_TX_LEVEL1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_RX_LEVEL1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_RX_LEVEL1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT3, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_TX_LEVEL2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_TX_LEVEL2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT4);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT4, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT4, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT4, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT4, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_RX_LEVEL2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_RX_LEVEL2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT5);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT5, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT5, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT5, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT5, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_TX_LEVEL3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_TX_LEVEL3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_TX_LEVEL3, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT6);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT6, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT6, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT6, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT6, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_RX_LEVEL3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_RX_LEVEL3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_RX_LEVEL3, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FLEVEL_CONSTANT7);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FLEVEL_CONSTANT7, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FLEVEL_CONSTANT7, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FLEVEL_CONSTANT7, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FLEVEL_CONSTANT7, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_TXF0);
        rp_pio_csr.wo(utra::rp_pio::SFR_TXF0, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_TXF0_FDIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_TXF0_FDIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_TXF0_FDIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_TXF0_FDIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_TXF0_FDIN, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_TXF1);
        rp_pio_csr.wo(utra::rp_pio::SFR_TXF1, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_TXF1_FDIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_TXF1_FDIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_TXF1_FDIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_TXF1_FDIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_TXF1_FDIN, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_TXF2);
        rp_pio_csr.wo(utra::rp_pio::SFR_TXF2, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_TXF2_FDIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_TXF2_FDIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_TXF2_FDIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_TXF2_FDIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_TXF2_FDIN, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_TXF3);
        rp_pio_csr.wo(utra::rp_pio::SFR_TXF3, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_TXF3_FDIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_TXF3_FDIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_TXF3_FDIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_TXF3_FDIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_TXF3_FDIN, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_RXF0);
        rp_pio_csr.wo(utra::rp_pio::SFR_RXF0, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_RXF0_PDOUT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_RXF0_PDOUT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_RXF0_PDOUT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_RXF0_PDOUT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_RXF0_PDOUT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_RXF1);
        rp_pio_csr.wo(utra::rp_pio::SFR_RXF1, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_RXF1_PDOUT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_RXF1_PDOUT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_RXF1_PDOUT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_RXF1_PDOUT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_RXF1_PDOUT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_RXF2);
        rp_pio_csr.wo(utra::rp_pio::SFR_RXF2, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_RXF2_PDOUT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_RXF2_PDOUT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_RXF2_PDOUT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_RXF2_PDOUT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_RXF2_PDOUT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_RXF3);
        rp_pio_csr.wo(utra::rp_pio::SFR_RXF3, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_RXF3_PDOUT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_RXF3_PDOUT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_RXF3_PDOUT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_RXF3_PDOUT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_RXF3_PDOUT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ_SFR_IRQ);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ_SFR_IRQ, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ_SFR_IRQ, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ_SFR_IRQ, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ_SFR_IRQ, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ_FORCE);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ_FORCE, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SYNC_BYPASS);
        rp_pio_csr.wo(utra::rp_pio::SFR_SYNC_BYPASS, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_DBG_PADOUT);
        rp_pio_csr.wo(utra::rp_pio::SFR_DBG_PADOUT, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_DBG_PADOUT_SFR_DBG_PADOUT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_DBG_PADOUT_SFR_DBG_PADOUT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_DBG_PADOUT_SFR_DBG_PADOUT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_DBG_PADOUT_SFR_DBG_PADOUT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_DBG_PADOUT_SFR_DBG_PADOUT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_DBG_PADOE);
        rp_pio_csr.wo(utra::rp_pio::SFR_DBG_PADOE, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_DBG_PADOE_SFR_DBG_PADOE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_DBG_PADOE_SFR_DBG_PADOE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_DBG_PADOE_SFR_DBG_PADOE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_DBG_PADOE_SFR_DBG_PADOE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_DBG_PADOE_SFR_DBG_PADOE, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_DBG_CFGINFO);
        rp_pio_csr.wo(utra::rp_pio::SFR_DBG_CFGINFO, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_DBG_CFGINFO_CONSTANT2, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM0);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM0, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM0_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM0_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM0_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM0_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM0_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM1);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM1, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM1_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM1_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM1_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM1_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM1_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM2);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM2, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM2_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM2_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM2_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM2_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM2_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM3);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM3, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM3_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM3_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM3_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM3_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM3_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM4);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM4, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM4_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM4_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM4_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM4_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM4_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM5);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM5, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM5_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM5_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM5_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM5_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM5_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM6);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM6, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM6_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM6_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM6_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM6_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM6_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM7);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM7, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM7_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM7_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM7_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM7_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM7_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM8);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM8, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM8_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM8_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM8_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM8_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM8_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM9);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM9, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM9_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM9_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM9_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM9_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM9_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM10);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM10, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM10_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM10_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM10_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM10_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM10_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM11);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM11, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM11_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM11_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM11_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM11_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM11_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM12);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM12, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM12_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM12_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM12_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM12_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM12_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM13);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM13, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM13_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM13_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM13_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM13_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM13_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM14);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM14, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM14_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM14_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM14_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM14_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM14_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM15);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM15, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM15_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM15_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM15_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM15_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM15_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM16);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM16, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM16_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM16_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM16_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM16_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM16_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM17);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM17, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM17_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM17_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM17_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM17_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM17_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM18);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM18, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM18_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM18_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM18_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM18_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM18_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM19);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM19, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM19_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM19_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM19_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM19_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM19_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM20);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM20, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM20_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM20_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM20_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM20_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM20_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM21);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM21, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM21_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM21_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM21_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM21_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM21_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM22);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM22, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM22_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM22_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM22_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM22_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM22_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM23);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM23, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM23_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM23_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM23_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM23_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM23_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM24);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM24, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM24_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM24_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM24_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM24_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM24_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM25);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM25, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM25_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM25_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM25_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM25_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM25_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM26);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM26, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM26_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM26_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM26_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM26_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM26_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM27);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM27, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM27_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM27_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM27_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM27_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM27_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM28);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM28, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM28_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM28_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM28_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM28_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM28_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM29);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM29, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM29_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM29_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM29_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM29_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM29_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM30);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM30, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM30_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM30_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM30_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM30_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM30_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INSTR_MEM31);
        rp_pio_csr.wo(utra::rp_pio::SFR_INSTR_MEM31, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INSTR_MEM31_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INSTR_MEM31_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INSTR_MEM31_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INSTR_MEM31_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INSTR_MEM31_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_CLKDIV);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_CLKDIV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_CLKDIV_UNUSED_DIV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_CLKDIV_UNUSED_DIV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_CLKDIV_UNUSED_DIV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_CLKDIV_UNUSED_DIV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_CLKDIV_UNUSED_DIV, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_FRAC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_FRAC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_FRAC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_CLKDIV_DIV_FRAC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_CLKDIV_DIV_FRAC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_INT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_INT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_CLKDIV_DIV_INT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_CLKDIV_DIV_INT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_CLKDIV_DIV_INT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_EXECCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_EXECCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_N);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_N, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_N, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_N, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_N, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_RESVD_EXEC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_RESVD_EXEC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_RESVD_EXEC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_RESVD_EXEC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_RESVD_EXEC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_PEND);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_PEND, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_PEND, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_PEND, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_PEND, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_JMP_PIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_JMP_PIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_JMP_PIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_JMP_PIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_JMP_PIN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_SHIFTCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_SHIFTCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_RESVD_SHIFT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_RESVD_SHIFT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_RESVD_SHIFT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_RESVD_SHIFT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_RESVD_SHIFT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_ADDR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_ADDR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_ADDR_PC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_ADDR_PC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_ADDR_PC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_ADDR_PC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_ADDR_PC, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_INSTR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_INSTR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_INSTR_IMM_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_INSTR_IMM_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_INSTR_IMM_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_INSTR_IMM_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_INSTR_IMM_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM0_PINCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM0_PINCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_CLKDIV);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_CLKDIV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_CLKDIV_UNUSED_DIV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_CLKDIV_UNUSED_DIV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_CLKDIV_UNUSED_DIV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_CLKDIV_UNUSED_DIV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_CLKDIV_UNUSED_DIV, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_FRAC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_FRAC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_FRAC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_CLKDIV_DIV_FRAC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_CLKDIV_DIV_FRAC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_INT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_INT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_CLKDIV_DIV_INT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_CLKDIV_DIV_INT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_CLKDIV_DIV_INT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_EXECCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_EXECCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_N);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_N, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_N, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_N, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_N, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_STATUS_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_RESVD_EXEC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_RESVD_EXEC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_RESVD_EXEC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_RESVD_EXEC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_RESVD_EXEC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_PEND);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_PEND, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_PEND, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_PEND, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_PEND, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_STICKY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_STICKY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_STICKY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_STICKY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_STICKY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_INLINE_OUT_EN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_INLINE_OUT_EN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_INLINE_OUT_EN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_INLINE_OUT_EN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_INLINE_OUT_EN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_EN_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_EN_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_EN_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_EN_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_OUT_EN_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_JMP_PIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_JMP_PIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_JMP_PIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_JMP_PIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_JMP_PIN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDE_PINDIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDE_PINDIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDE_PINDIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_SIDE_PINDIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_SIDE_PINDIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_SIDESET_ENABLE_BIT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_SHIFTCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_SHIFTCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_RESVD_SHIFT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_RESVD_SHIFT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_RESVD_SHIFT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_RESVD_SHIFT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_RESVD_SHIFT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PUSH);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PUSH, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PUSH, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PUSH, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PUSH, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_AUTO_PULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_IN_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_OUT_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_ISR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_ISR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_ISR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_ISR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_ISR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OSR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OSR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_OSR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_OSR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_OSR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_TX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_TX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_TX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_TX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_TX, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_RX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_RX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_RX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_RX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_SHIFTCTRL_JOIN_RX, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_ADDR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_ADDR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_ADDR_PC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_ADDR_PC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_ADDR_PC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_ADDR_PC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_ADDR_PC, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_INSTR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_INSTR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_INSTR_IMM_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_INSTR_IMM_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_INSTR_IMM_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_INSTR_IMM_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_INSTR_IMM_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM1_PINCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM1_PINCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_IN_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_IN_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_IN_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_IN_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_IN_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_OUT_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SET_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM1_PINCTRL_PINS_SIDE_COUNT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_CLKDIV);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_CLKDIV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_CLKDIV_UNUSED_DIV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_CLKDIV_UNUSED_DIV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_CLKDIV_UNUSED_DIV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_CLKDIV_UNUSED_DIV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_CLKDIV_UNUSED_DIV, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_FRAC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_FRAC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_FRAC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_CLKDIV_DIV_FRAC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_CLKDIV_DIV_FRAC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_INT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_INT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_CLKDIV_DIV_INT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_CLKDIV_DIV_INT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_CLKDIV_DIV_INT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_EXECCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_EXECCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_N);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_N, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_N, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_N, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_N, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_STATUS_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_RESVD_EXEC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_RESVD_EXEC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_RESVD_EXEC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_RESVD_EXEC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_RESVD_EXEC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_PEND);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_PEND, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_PEND, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_PEND, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_PEND, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_STICKY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_STICKY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_STICKY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_STICKY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_STICKY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_INLINE_OUT_EN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_INLINE_OUT_EN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_INLINE_OUT_EN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_INLINE_OUT_EN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_INLINE_OUT_EN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_EN_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_EN_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_EN_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_EN_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_OUT_EN_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_JMP_PIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_JMP_PIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_JMP_PIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_JMP_PIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_JMP_PIN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDE_PINDIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDE_PINDIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDE_PINDIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_SIDE_PINDIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_SIDE_PINDIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_SIDESET_ENABLE_BIT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_SHIFTCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_SHIFTCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_RESVD_SHIFT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_RESVD_SHIFT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_RESVD_SHIFT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_RESVD_SHIFT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_RESVD_SHIFT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PUSH);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PUSH, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PUSH, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PUSH, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PUSH, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_AUTO_PULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_IN_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_OUT_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_ISR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_ISR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_ISR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_ISR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_ISR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OSR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OSR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_OSR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_OSR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_OSR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_TX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_TX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_TX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_TX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_TX, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_RX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_RX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_RX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_RX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_SHIFTCTRL_JOIN_RX, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_ADDR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_ADDR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_ADDR_PC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_ADDR_PC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_ADDR_PC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_ADDR_PC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_ADDR_PC, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_INSTR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_INSTR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_INSTR_IMM_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_INSTR_IMM_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_INSTR_IMM_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_INSTR_IMM_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_INSTR_IMM_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM2_PINCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM2_PINCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_IN_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_IN_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_IN_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_IN_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_IN_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_OUT_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SET_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM2_PINCTRL_PINS_SIDE_COUNT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_CLKDIV);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_CLKDIV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_CLKDIV_UNUSED_DIV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_CLKDIV_UNUSED_DIV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_CLKDIV_UNUSED_DIV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_CLKDIV_UNUSED_DIV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_CLKDIV_UNUSED_DIV, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_FRAC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_FRAC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_FRAC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_CLKDIV_DIV_FRAC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_CLKDIV_DIV_FRAC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_INT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_INT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_CLKDIV_DIV_INT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_CLKDIV_DIV_INT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_CLKDIV_DIV_INT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_EXECCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_EXECCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_N);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_N, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_N, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_N, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_N, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_STATUS_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_RESVD_EXEC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_RESVD_EXEC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_RESVD_EXEC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_RESVD_EXEC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_RESVD_EXEC, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_PEND);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_PEND, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_PEND, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_PEND, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_PEND, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_STICKY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_STICKY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_STICKY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_STICKY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_STICKY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_INLINE_OUT_EN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_INLINE_OUT_EN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_INLINE_OUT_EN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_INLINE_OUT_EN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_INLINE_OUT_EN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_EN_SEL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_EN_SEL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_EN_SEL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_EN_SEL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_OUT_EN_SEL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_JMP_PIN);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_JMP_PIN, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_JMP_PIN, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_JMP_PIN, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_JMP_PIN, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDE_PINDIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDE_PINDIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDE_PINDIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_SIDE_PINDIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_SIDE_PINDIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_SIDESET_ENABLE_BIT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_SHIFTCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_SHIFTCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_RESVD_SHIFT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_RESVD_SHIFT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_RESVD_SHIFT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_RESVD_SHIFT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_RESVD_SHIFT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PUSH);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PUSH, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PUSH, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PUSH, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PUSH, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_AUTO_PULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_IN_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_OUT_SHIFT_DIR, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_ISR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_ISR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_ISR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_ISR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_ISR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OSR_THRESHOLD);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OSR_THRESHOLD, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_OSR_THRESHOLD, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_OSR_THRESHOLD, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_OSR_THRESHOLD, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_TX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_TX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_TX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_TX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_TX, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_RX);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_RX, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_RX, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_RX, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_SHIFTCTRL_JOIN_RX, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_ADDR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_ADDR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_ADDR_PC);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_ADDR_PC, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_ADDR_PC, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_ADDR_PC, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_ADDR_PC, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_INSTR);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_INSTR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_INSTR_IMM_INSTR);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_INSTR_IMM_INSTR, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_INSTR_IMM_INSTR, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_INSTR_IMM_INSTR, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_INSTR_IMM_INSTR, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_SM3_PINCTRL);
        rp_pio_csr.wo(utra::rp_pio::SFR_SM3_PINCTRL, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_IN_BASE);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_IN_BASE, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_IN_BASE, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_IN_BASE, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_IN_BASE, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_OUT_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SET_COUNT, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_COUNT);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_COUNT, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_COUNT, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_COUNT, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_SM3_PINCTRL_PINS_SIDE_COUNT, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_INTR);
        rp_pio_csr.wo(utra::rp_pio::SFR_INTR, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INTR_INTR_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INTR_INTR_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INTR_INTR_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INTR_INTR_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INTR_INTR_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INTR_INTR_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INTR_INTR_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INTR_INTR_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INTR_INTR_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INTR_INTR_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_INTR_INTR_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_INTR_INTR_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_INTR_INTR_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_INTR_INTR_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_INTR_INTR_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ0_INTE);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ0_INTE, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTE_IRQ0_INTE_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ0_INTF);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ0_INTF, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTF_IRQ0_INTF_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ0_INTS);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ0_INTS, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ0_INTS_IRQ0_INTS_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ1_INTE);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ1_INTE, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTE_IRQ1_INTE_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ1_INTF);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ1_INTF, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTF_IRQ1_INTF_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IRQ1_INTS);
        rp_pio_csr.wo(utra::rp_pio::SFR_IRQ1_INTS, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_RXNEMPTY, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_TXNFULL, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_SM);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_SM, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_SM, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_SM, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IRQ1_INTS_IRQ1_INTS_SM, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED81);
        rp_pio_csr.wo(utra::rp_pio::RESERVED81, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED81_RESERVED81);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED81_RESERVED81, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED81_RESERVED81, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED81_RESERVED81, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED81_RESERVED81, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED82);
        rp_pio_csr.wo(utra::rp_pio::RESERVED82, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED82_RESERVED82);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED82_RESERVED82, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED82_RESERVED82, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED82_RESERVED82, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED82_RESERVED82, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED83);
        rp_pio_csr.wo(utra::rp_pio::RESERVED83, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED83_RESERVED83);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED83_RESERVED83, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED83_RESERVED83, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED83_RESERVED83, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED83_RESERVED83, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED84);
        rp_pio_csr.wo(utra::rp_pio::RESERVED84, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED84_RESERVED84);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED84_RESERVED84, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED84_RESERVED84, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED84_RESERVED84, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED84_RESERVED84, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED85);
        rp_pio_csr.wo(utra::rp_pio::RESERVED85, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED85_RESERVED85);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED85_RESERVED85, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED85_RESERVED85, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED85_RESERVED85, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED85_RESERVED85, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED86);
        rp_pio_csr.wo(utra::rp_pio::RESERVED86, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED86_RESERVED86);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED86_RESERVED86, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED86_RESERVED86, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED86_RESERVED86, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED86_RESERVED86, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED87);
        rp_pio_csr.wo(utra::rp_pio::RESERVED87, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED87_RESERVED87);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED87_RESERVED87, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED87_RESERVED87, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED87_RESERVED87, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED87_RESERVED87, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED88);
        rp_pio_csr.wo(utra::rp_pio::RESERVED88, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED88_RESERVED88);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED88_RESERVED88, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED88_RESERVED88, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED88_RESERVED88, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED88_RESERVED88, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED89);
        rp_pio_csr.wo(utra::rp_pio::RESERVED89, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED89_RESERVED89);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED89_RESERVED89, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED89_RESERVED89, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED89_RESERVED89, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED89_RESERVED89, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED90);
        rp_pio_csr.wo(utra::rp_pio::RESERVED90, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED90_RESERVED90);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED90_RESERVED90, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED90_RESERVED90, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED90_RESERVED90, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED90_RESERVED90, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED91);
        rp_pio_csr.wo(utra::rp_pio::RESERVED91, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED91_RESERVED91);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED91_RESERVED91, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED91_RESERVED91, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED91_RESERVED91, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED91_RESERVED91, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED92);
        rp_pio_csr.wo(utra::rp_pio::RESERVED92, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED92_RESERVED92);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED92_RESERVED92, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED92_RESERVED92, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED92_RESERVED92, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED92_RESERVED92, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED93);
        rp_pio_csr.wo(utra::rp_pio::RESERVED93, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED93_RESERVED93);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED93_RESERVED93, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED93_RESERVED93, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED93_RESERVED93, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED93_RESERVED93, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED94);
        rp_pio_csr.wo(utra::rp_pio::RESERVED94, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED94_RESERVED94);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED94_RESERVED94, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED94_RESERVED94, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED94_RESERVED94, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED94_RESERVED94, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::RESERVED95);
        rp_pio_csr.wo(utra::rp_pio::RESERVED95, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::RESERVED95_RESERVED95);
        rp_pio_csr.rmwf(utra::rp_pio::RESERVED95_RESERVED95, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::RESERVED95_RESERVED95, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::RESERVED95_RESERVED95, 1);
        rp_pio_csr.wfo(utra::rp_pio::RESERVED95_RESERVED95, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IO_OE_INV);
        rp_pio_csr.wo(utra::rp_pio::SFR_IO_OE_INV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IO_OE_INV_SFR_IO_OE_INV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IO_OE_INV_SFR_IO_OE_INV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IO_OE_INV_SFR_IO_OE_INV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IO_OE_INV_SFR_IO_OE_INV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IO_OE_INV_SFR_IO_OE_INV, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IO_O_INV);
        rp_pio_csr.wo(utra::rp_pio::SFR_IO_O_INV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IO_O_INV_SFR_IO_O_INV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IO_O_INV_SFR_IO_O_INV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IO_O_INV_SFR_IO_O_INV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IO_O_INV_SFR_IO_O_INV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IO_O_INV_SFR_IO_O_INV, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_IO_I_INV);
        rp_pio_csr.wo(utra::rp_pio::SFR_IO_I_INV, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_IO_I_INV_SFR_IO_I_INV);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_IO_I_INV_SFR_IO_I_INV, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_IO_I_INV_SFR_IO_I_INV, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_IO_I_INV_SFR_IO_I_INV, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_IO_I_INV_SFR_IO_I_INV, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_FIFO_MARGIN);
        rp_pio_csr.wo(utra::rp_pio::SFR_FIFO_MARGIN, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3, baz);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_ZERO0);
        rp_pio_csr.wo(utra::rp_pio::SFR_ZERO0, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_ZERO0_SFR_ZERO0);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_ZERO0_SFR_ZERO0, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_ZERO0_SFR_ZERO0, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_ZERO0_SFR_ZERO0, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_ZERO0_SFR_ZERO0, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_ZERO1);
        rp_pio_csr.wo(utra::rp_pio::SFR_ZERO1, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_ZERO1_SFR_ZERO1);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_ZERO1_SFR_ZERO1, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_ZERO1_SFR_ZERO1, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_ZERO1_SFR_ZERO1, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_ZERO1_SFR_ZERO1, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_ZERO2);
        rp_pio_csr.wo(utra::rp_pio::SFR_ZERO2, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_ZERO2_SFR_ZERO2);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_ZERO2_SFR_ZERO2, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_ZERO2_SFR_ZERO2, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_ZERO2_SFR_ZERO2, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_ZERO2_SFR_ZERO2, baz);

        let foo = rp_pio_csr.r(utra::rp_pio::SFR_ZERO3);
        rp_pio_csr.wo(utra::rp_pio::SFR_ZERO3, foo);
        let bar = rp_pio_csr.rf(utra::rp_pio::SFR_ZERO3_SFR_ZERO3);
        rp_pio_csr.rmwf(utra::rp_pio::SFR_ZERO3_SFR_ZERO3, bar);
        let mut baz = rp_pio_csr.zf(utra::rp_pio::SFR_ZERO3_SFR_ZERO3, bar);
        baz |= rp_pio_csr.ms(utra::rp_pio::SFR_ZERO3_SFR_ZERO3, 1);
        rp_pio_csr.wfo(utra::rp_pio::SFR_ZERO3_SFR_ZERO3, baz);
  }
}
