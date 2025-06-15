
#![cfg_attr(rustfmt, rustfmt_skip)] // don't format generated files
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
    pub const fn register(&self) -> Register { self.register }
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
    pub unsafe fn base(&self) -> *mut T {
        self.base.load(core::sync::atomic::Ordering::SeqCst) as *mut T
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
pub const HW_VEXRISCV_DEBUG_MEM:     usize = 0xf00f0000;
pub const HW_VEXRISCV_DEBUG_MEM_LEN: usize = 256;
pub const HW_SRAM_MEM:     usize = 0x10000000;
pub const HW_SRAM_MEM_LEN: usize = 8192;
pub const HW_MAIN_RAM_MEM:     usize = 0x40000000;
pub const HW_MAIN_RAM_MEM_LEN: usize = 409600;
pub const HW_BIO_MEM:     usize = 0x90000000;
pub const HW_BIO_MEM_LEN: usize = 65536;
pub const HW_CSR_MEM:     usize = 0xf0000000;
pub const HW_CSR_MEM_LEN: usize = 65536;
pub const HW_BIO_BDMA_MEM:     usize = 0x90000000;
pub const HW_BIO_BDMA_MEM_LEN: usize = 4096;
pub const HW_BIO_IMEM0_MEM:     usize = 0x90001000;
pub const HW_BIO_IMEM0_MEM_LEN: usize = 4096;
pub const HW_BIO_IMEM1_MEM:     usize = 0x90002000;
pub const HW_BIO_IMEM1_MEM_LEN: usize = 4096;
pub const HW_BIO_IMEM2_MEM:     usize = 0x90003000;
pub const HW_BIO_IMEM2_MEM_LEN: usize = 4096;
pub const HW_BIO_IMEM3_MEM:     usize = 0x90004000;
pub const HW_BIO_IMEM3_MEM_LEN: usize = 4096;
pub const HW_BIO_FIFO0_MEM:     usize = 0x90005000;
pub const HW_BIO_FIFO0_MEM_LEN: usize = 4096;
pub const HW_BIO_FIFO1_MEM:     usize = 0x90006000;
pub const HW_BIO_FIFO1_MEM_LEN: usize = 4096;
pub const HW_BIO_FIFO2_MEM:     usize = 0x90007000;
pub const HW_BIO_FIFO2_MEM_LEN: usize = 4096;
pub const HW_BIO_FIFO3_MEM:     usize = 0x90008000;
pub const HW_BIO_FIFO3_MEM_LEN: usize = 4096;

// Physical base addresses of registers
pub const HW_RGB_BASE :   usize = 0xf0000000;
pub const HW_CTRL_BASE :   usize = 0xf0000800;
pub const HW_IDENTIFIER_MEM_BASE :   usize = 0xf0001000;
pub const HW_LEDS_BASE :   usize = 0xf0001800;
pub const HW_TIMER0_BASE :   usize = 0xf0002000;
pub const HW_UART_BASE :   usize = 0xf0002800;
pub const HW_BIO_BDMA_BASE :   usize = 0x90000000;


pub mod utra {

    pub mod rgb {
        pub const RGB_NUMREGS: usize = 1;

        pub const OUT: crate::Register = crate::Register::new(0, 0xfff);
        pub const OUT_OUT: crate::Field = crate::Field::new(12, 0, OUT);

        pub const HW_RGB_BASE: usize = 0xf0000000;
    }

    pub mod ctrl {
        pub const CTRL_NUMREGS: usize = 3;

        pub const RESET: crate::Register = crate::Register::new(0, 0x3);
        pub const RESET_SOC_RST: crate::Field = crate::Field::new(1, 0, RESET);
        pub const RESET_CPU_RST: crate::Field = crate::Field::new(1, 1, RESET);

        pub const SCRATCH: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const SCRATCH_SCRATCH: crate::Field = crate::Field::new(32, 0, SCRATCH);

        pub const BUS_ERRORS: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const BUS_ERRORS_BUS_ERRORS: crate::Field = crate::Field::new(32, 0, BUS_ERRORS);

        pub const HW_CTRL_BASE: usize = 0xf0000800;
    }

    pub mod identifier_mem {
        pub const IDENTIFIER_MEM_NUMREGS: usize = 1;

        pub const IDENTIFIER_MEM: crate::Register = crate::Register::new(0, 0xff);
        pub const IDENTIFIER_MEM_IDENTIFIER_MEM: crate::Field = crate::Field::new(8, 0, IDENTIFIER_MEM);

        pub const HW_IDENTIFIER_MEM_BASE: usize = 0xf0001000;
    }

    pub mod leds {
        pub const LEDS_NUMREGS: usize = 1;

        pub const OUT: crate::Register = crate::Register::new(0, 0xf);
        pub const OUT_OUT: crate::Field = crate::Field::new(4, 0, OUT);

        pub const HW_LEDS_BASE: usize = 0xf0001800;
    }

    pub mod timer0 {
        pub const TIMER0_NUMREGS: usize = 8;

        pub const LOAD: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const LOAD_LOAD: crate::Field = crate::Field::new(32, 0, LOAD);

        pub const RELOAD: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const RELOAD_RELOAD: crate::Field = crate::Field::new(32, 0, RELOAD);

        pub const EN: crate::Register = crate::Register::new(2, 0x1);
        pub const EN_EN: crate::Field = crate::Field::new(1, 0, EN);

        pub const UPDATE_VALUE: crate::Register = crate::Register::new(3, 0x1);
        pub const UPDATE_VALUE_UPDATE_VALUE: crate::Field = crate::Field::new(1, 0, UPDATE_VALUE);

        pub const VALUE: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const VALUE_VALUE: crate::Field = crate::Field::new(32, 0, VALUE);

        pub const EV_STATUS: crate::Register = crate::Register::new(5, 0x1);
        pub const EV_STATUS_ZERO: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(6, 0x1);
        pub const EV_PENDING_ZERO: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(7, 0x1);
        pub const EV_ENABLE_ZERO: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const TIMER0_IRQ: usize = 0;
        pub const HW_TIMER0_BASE: usize = 0xf0002000;
    }

    pub mod uart {
        pub const UART_NUMREGS: usize = 8;

        pub const RXTX: crate::Register = crate::Register::new(0, 0xff);
        pub const RXTX_RXTX: crate::Field = crate::Field::new(8, 0, RXTX);

        pub const TXFULL: crate::Register = crate::Register::new(1, 0x1);
        pub const TXFULL_TXFULL: crate::Field = crate::Field::new(1, 0, TXFULL);

        pub const RXEMPTY: crate::Register = crate::Register::new(2, 0x1);
        pub const RXEMPTY_RXEMPTY: crate::Field = crate::Field::new(1, 0, RXEMPTY);

        pub const EV_STATUS: crate::Register = crate::Register::new(3, 0x3);
        pub const EV_STATUS_TX: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_RX: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(4, 0x3);
        pub const EV_PENDING_TX: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_RX: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(5, 0x3);
        pub const EV_ENABLE_TX: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_RX: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const TXEMPTY: crate::Register = crate::Register::new(6, 0x1);
        pub const TXEMPTY_TXEMPTY: crate::Field = crate::Field::new(1, 0, TXEMPTY);

        pub const RXFULL: crate::Register = crate::Register::new(7, 0x1);
        pub const RXFULL_RXFULL: crate::Field = crate::Field::new(1, 0, RXFULL);

        pub const UART_IRQ: usize = 1;
        pub const HW_UART_BASE: usize = 0xf0002800;
    }

    pub mod bio_bdma {
        pub const BIO_BDMA_NUMREGS: usize = 60;

        pub const SFR_CTRL: crate::Register = crate::Register::new(0, 0xfff);
        pub const SFR_CTRL_EN: crate::Field = crate::Field::new(4, 0, SFR_CTRL);
        pub const SFR_CTRL_RESTART: crate::Field = crate::Field::new(4, 4, SFR_CTRL);
        pub const SFR_CTRL_CLKDIV_RESTART: crate::Field = crate::Field::new(4, 8, SFR_CTRL);

        pub const SFR_CFGINFO: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const SFR_CFGINFO_CONSTANT0: crate::Field = crate::Field::new(8, 0, SFR_CFGINFO);
        pub const SFR_CFGINFO_CONSTANT1: crate::Field = crate::Field::new(8, 8, SFR_CFGINFO);
        pub const SFR_CFGINFO_CONSTANT2: crate::Field = crate::Field::new(16, 16, SFR_CFGINFO);

        pub const SFR_CONFIG: crate::Register = crate::Register::new(2, 0x3ff);
        pub const SFR_CONFIG_SNAP_OUTPUT_TO_WHICH: crate::Field = crate::Field::new(2, 0, SFR_CONFIG);
        pub const SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM: crate::Field = crate::Field::new(1, 2, SFR_CONFIG);
        pub const SFR_CONFIG_SNAP_INPUT_TO_WHICH: crate::Field = crate::Field::new(2, 3, SFR_CONFIG);
        pub const SFR_CONFIG_SNAP_INPUT_TO_QUANTUM: crate::Field = crate::Field::new(1, 5, SFR_CONFIG);
        pub const SFR_CONFIG_DISABLE_FILTER_PERI: crate::Field = crate::Field::new(1, 6, SFR_CONFIG);
        pub const SFR_CONFIG_DISABLE_FILTER_MEM: crate::Field = crate::Field::new(1, 7, SFR_CONFIG);
        pub const SFR_CONFIG_CLOCKING_MODE: crate::Field = crate::Field::new(2, 8, SFR_CONFIG);

        pub const SFR_FLEVEL: crate::Register = crate::Register::new(3, 0xffff);
        pub const SFR_FLEVEL_PCLK_REGFIFO_LEVEL0: crate::Field = crate::Field::new(4, 0, SFR_FLEVEL);
        pub const SFR_FLEVEL_PCLK_REGFIFO_LEVEL1: crate::Field = crate::Field::new(4, 4, SFR_FLEVEL);
        pub const SFR_FLEVEL_PCLK_REGFIFO_LEVEL2: crate::Field = crate::Field::new(4, 8, SFR_FLEVEL);
        pub const SFR_FLEVEL_PCLK_REGFIFO_LEVEL3: crate::Field = crate::Field::new(4, 12, SFR_FLEVEL);

        pub const SFR_TXF0: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const SFR_TXF0_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF0);

        pub const SFR_TXF1: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const SFR_TXF1_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF1);

        pub const SFR_TXF2: crate::Register = crate::Register::new(6, 0xffffffff);
        pub const SFR_TXF2_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF2);

        pub const SFR_TXF3: crate::Register = crate::Register::new(7, 0xffffffff);
        pub const SFR_TXF3_FDIN: crate::Field = crate::Field::new(32, 0, SFR_TXF3);

        pub const SFR_RXF0: crate::Register = crate::Register::new(8, 0xffffffff);
        pub const SFR_RXF0_FDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF0);

        pub const SFR_RXF1: crate::Register = crate::Register::new(9, 0xffffffff);
        pub const SFR_RXF1_FDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF1);

        pub const SFR_RXF2: crate::Register = crate::Register::new(10, 0xffffffff);
        pub const SFR_RXF2_FDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF2);

        pub const SFR_RXF3: crate::Register = crate::Register::new(11, 0xffffffff);
        pub const SFR_RXF3_FDOUT: crate::Field = crate::Field::new(32, 0, SFR_RXF3);

        pub const SFR_ELEVEL: crate::Register = crate::Register::new(12, 0xffffffff);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL0: crate::Field = crate::Field::new(4, 0, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL1: crate::Field = crate::Field::new(4, 4, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL2: crate::Field = crate::Field::new(4, 8, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL3: crate::Field = crate::Field::new(4, 12, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL4: crate::Field = crate::Field::new(4, 16, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL5: crate::Field = crate::Field::new(4, 20, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL6: crate::Field = crate::Field::new(4, 24, SFR_ELEVEL);
        pub const SFR_ELEVEL_FIFO_EVENT_LEVEL7: crate::Field = crate::Field::new(4, 28, SFR_ELEVEL);

        pub const SFR_ETYPE: crate::Register = crate::Register::new(13, 0xffffff);
        pub const SFR_ETYPE_FIFO_EVENT_LT_MASK: crate::Field = crate::Field::new(8, 0, SFR_ETYPE);
        pub const SFR_ETYPE_FIFO_EVENT_EQ_MASK: crate::Field = crate::Field::new(8, 8, SFR_ETYPE);
        pub const SFR_ETYPE_FIFO_EVENT_GT_MASK: crate::Field = crate::Field::new(8, 16, SFR_ETYPE);

        pub const SFR_EVENT_SET: crate::Register = crate::Register::new(14, 0xffffff);
        pub const SFR_EVENT_SET_SFR_EVENT_SET: crate::Field = crate::Field::new(24, 0, SFR_EVENT_SET);

        pub const SFR_EVENT_CLR: crate::Register = crate::Register::new(15, 0xffffff);
        pub const SFR_EVENT_CLR_SFR_EVENT_CLR: crate::Field = crate::Field::new(24, 0, SFR_EVENT_CLR);

        pub const SFR_EVENT_STATUS: crate::Register = crate::Register::new(16, 0xffffffff);
        pub const SFR_EVENT_STATUS_SFR_EVENT_STATUS: crate::Field = crate::Field::new(32, 0, SFR_EVENT_STATUS);

        pub const SFR_EXTCLOCK: crate::Register = crate::Register::new(17, 0xffffff);
        pub const SFR_EXTCLOCK_USE_EXTCLK: crate::Field = crate::Field::new(4, 0, SFR_EXTCLOCK);
        pub const SFR_EXTCLOCK_EXTCLK_GPIO_0: crate::Field = crate::Field::new(5, 4, SFR_EXTCLOCK);
        pub const SFR_EXTCLOCK_EXTCLK_GPIO_1: crate::Field = crate::Field::new(5, 9, SFR_EXTCLOCK);
        pub const SFR_EXTCLOCK_EXTCLK_GPIO_2: crate::Field = crate::Field::new(5, 14, SFR_EXTCLOCK);
        pub const SFR_EXTCLOCK_EXTCLK_GPIO_3: crate::Field = crate::Field::new(5, 19, SFR_EXTCLOCK);

        pub const SFR_FIFO_CLR: crate::Register = crate::Register::new(18, 0xf);
        pub const SFR_FIFO_CLR_SFR_FIFO_CLR: crate::Field = crate::Field::new(4, 0, SFR_FIFO_CLR);

        pub const SFR_QDIV0: crate::Register = crate::Register::new(20, 0x7);
        pub const SFR_QDIV0_UNUSED_DIV: crate::Field = crate::Field::new(1, 0, SFR_QDIV0);
        pub const SFR_QDIV0_DIV_FRAC: crate::Field = crate::Field::new(1, 1, SFR_QDIV0);
        pub const SFR_QDIV0_DIV_INT: crate::Field = crate::Field::new(1, 2, SFR_QDIV0);

        pub const SFR_QDIV1: crate::Register = crate::Register::new(21, 0x7);
        pub const SFR_QDIV1_UNUSED_DIV: crate::Field = crate::Field::new(1, 0, SFR_QDIV1);
        pub const SFR_QDIV1_DIV_FRAC: crate::Field = crate::Field::new(1, 1, SFR_QDIV1);
        pub const SFR_QDIV1_DIV_INT: crate::Field = crate::Field::new(1, 2, SFR_QDIV1);

        pub const SFR_QDIV2: crate::Register = crate::Register::new(22, 0x7);
        pub const SFR_QDIV2_UNUSED_DIV: crate::Field = crate::Field::new(1, 0, SFR_QDIV2);
        pub const SFR_QDIV2_DIV_FRAC: crate::Field = crate::Field::new(1, 1, SFR_QDIV2);
        pub const SFR_QDIV2_DIV_INT: crate::Field = crate::Field::new(1, 2, SFR_QDIV2);

        pub const SFR_QDIV3: crate::Register = crate::Register::new(23, 0x7);
        pub const SFR_QDIV3_UNUSED_DIV: crate::Field = crate::Field::new(1, 0, SFR_QDIV3);
        pub const SFR_QDIV3_DIV_FRAC: crate::Field = crate::Field::new(1, 1, SFR_QDIV3);
        pub const SFR_QDIV3_DIV_INT: crate::Field = crate::Field::new(1, 2, SFR_QDIV3);

        pub const SFR_SYNC_BYPASS: crate::Register = crate::Register::new(24, 0xffffffff);
        pub const SFR_SYNC_BYPASS_SFR_SYNC_BYPASS: crate::Field = crate::Field::new(32, 0, SFR_SYNC_BYPASS);

        pub const SFR_IO_OE_INV: crate::Register = crate::Register::new(25, 0xffffffff);
        pub const SFR_IO_OE_INV_SFR_IO_OE_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_OE_INV);

        pub const SFR_IO_O_INV: crate::Register = crate::Register::new(26, 0xffffffff);
        pub const SFR_IO_O_INV_SFR_IO_O_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_O_INV);

        pub const SFR_IO_I_INV: crate::Register = crate::Register::new(27, 0xffffffff);
        pub const SFR_IO_I_INV_SFR_IO_I_INV: crate::Field = crate::Field::new(32, 0, SFR_IO_I_INV);

        pub const SFR_IRQMASK_0: crate::Register = crate::Register::new(28, 0xffffffff);
        pub const SFR_IRQMASK_0_SFR_IRQMASK_0: crate::Field = crate::Field::new(32, 0, SFR_IRQMASK_0);

        pub const SFR_IRQMASK_1: crate::Register = crate::Register::new(29, 0xffffffff);
        pub const SFR_IRQMASK_1_SFR_IRQMASK_1: crate::Field = crate::Field::new(32, 0, SFR_IRQMASK_1);

        pub const SFR_IRQMASK_2: crate::Register = crate::Register::new(30, 0xffffffff);
        pub const SFR_IRQMASK_2_SFR_IRQMASK_2: crate::Field = crate::Field::new(32, 0, SFR_IRQMASK_2);

        pub const SFR_IRQMASK_3: crate::Register = crate::Register::new(31, 0xffffffff);
        pub const SFR_IRQMASK_3_SFR_IRQMASK_3: crate::Field = crate::Field::new(32, 0, SFR_IRQMASK_3);

        pub const SFR_IRQ_EDGE: crate::Register = crate::Register::new(32, 0xf);
        pub const SFR_IRQ_EDGE_SFR_IRQ_EDGE: crate::Field = crate::Field::new(4, 0, SFR_IRQ_EDGE);

        pub const SFR_DBG_PADOUT: crate::Register = crate::Register::new(33, 0xffffffff);
        pub const SFR_DBG_PADOUT_SFR_DBG_PADOUT: crate::Field = crate::Field::new(32, 0, SFR_DBG_PADOUT);

        pub const SFR_DBG_PADOE: crate::Register = crate::Register::new(34, 0xffffffff);
        pub const SFR_DBG_PADOE_SFR_DBG_PADOE: crate::Field = crate::Field::new(32, 0, SFR_DBG_PADOE);

        pub const SFR_DBG0: crate::Register = crate::Register::new(36, 0x3);
        pub const SFR_DBG0_DBG_PC: crate::Field = crate::Field::new(1, 0, SFR_DBG0);
        pub const SFR_DBG0_TRAP: crate::Field = crate::Field::new(1, 1, SFR_DBG0);

        pub const SFR_DBG1: crate::Register = crate::Register::new(37, 0x3);
        pub const SFR_DBG1_DBG_PC: crate::Field = crate::Field::new(1, 0, SFR_DBG1);
        pub const SFR_DBG1_TRAP: crate::Field = crate::Field::new(1, 1, SFR_DBG1);

        pub const SFR_DBG2: crate::Register = crate::Register::new(38, 0x3);
        pub const SFR_DBG2_DBG_PC: crate::Field = crate::Field::new(1, 0, SFR_DBG2);
        pub const SFR_DBG2_TRAP: crate::Field = crate::Field::new(1, 1, SFR_DBG2);

        pub const SFR_DBG3: crate::Register = crate::Register::new(39, 0x3);
        pub const SFR_DBG3_DBG_PC: crate::Field = crate::Field::new(1, 0, SFR_DBG3);
        pub const SFR_DBG3_TRAP: crate::Field = crate::Field::new(1, 1, SFR_DBG3);

        pub const SFR_MEM_GUTTER: crate::Register = crate::Register::new(40, 0xffffffff);
        pub const SFR_MEM_GUTTER_SFR_MEM_GUTTER: crate::Field = crate::Field::new(32, 0, SFR_MEM_GUTTER);

        pub const SFR_PERI_GUTTER: crate::Register = crate::Register::new(41, 0xffffffff);
        pub const SFR_PERI_GUTTER_SFR_PERI_GUTTER: crate::Field = crate::Field::new(32, 0, SFR_PERI_GUTTER);

        pub const SFR_DMAREQ_MAP_CR_EVMAP0: crate::Register = crate::Register::new(44, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP0);

        pub const SFR_DMAREQ_MAP_CR_EVMAP1: crate::Register = crate::Register::new(45, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP1);

        pub const SFR_DMAREQ_MAP_CR_EVMAP2: crate::Register = crate::Register::new(46, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP2);

        pub const SFR_DMAREQ_MAP_CR_EVMAP3: crate::Register = crate::Register::new(47, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP3);

        pub const SFR_DMAREQ_MAP_CR_EVMAP4: crate::Register = crate::Register::new(48, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP4);

        pub const SFR_DMAREQ_MAP_CR_EVMAP5: crate::Register = crate::Register::new(49, 0xffffffff);
        pub const SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_MAP_CR_EVMAP5);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT0: crate::Register = crate::Register::new(50, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT0);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT1: crate::Register = crate::Register::new(51, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT1);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT2: crate::Register = crate::Register::new(52, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT2);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT3: crate::Register = crate::Register::new(53, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT3);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT4: crate::Register = crate::Register::new(54, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT4);

        pub const SFR_DMAREQ_STAT_SR_EVSTAT5: crate::Register = crate::Register::new(55, 0xffffffff);
        pub const SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5: crate::Field = crate::Field::new(32, 0, SFR_DMAREQ_STAT_SR_EVSTAT5);

        pub const SFR_FILTER_BASE_0: crate::Register = crate::Register::new(56, 0xfffff);
        pub const SFR_FILTER_BASE_0_FILTER_BASE: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BASE_0);

        pub const SFR_FILTER_BOUNDS_0: crate::Register = crate::Register::new(57, 0xfffff);
        pub const SFR_FILTER_BOUNDS_0_FILTER_BOUNDS: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BOUNDS_0);

        pub const SFR_FILTER_BASE_1: crate::Register = crate::Register::new(58, 0xfffff);
        pub const SFR_FILTER_BASE_1_FILTER_BASE: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BASE_1);

        pub const SFR_FILTER_BOUNDS_1: crate::Register = crate::Register::new(59, 0xfffff);
        pub const SFR_FILTER_BOUNDS_1_FILTER_BOUNDS: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BOUNDS_1);

        pub const SFR_FILTER_BASE_2: crate::Register = crate::Register::new(60, 0xfffff);
        pub const SFR_FILTER_BASE_2_FILTER_BASE: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BASE_2);

        pub const SFR_FILTER_BOUNDS_2: crate::Register = crate::Register::new(61, 0xfffff);
        pub const SFR_FILTER_BOUNDS_2_FILTER_BOUNDS: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BOUNDS_2);

        pub const SFR_FILTER_BASE_3: crate::Register = crate::Register::new(62, 0xfffff);
        pub const SFR_FILTER_BASE_3_FILTER_BASE: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BASE_3);

        pub const SFR_FILTER_BOUNDS_3: crate::Register = crate::Register::new(63, 0xfffff);
        pub const SFR_FILTER_BOUNDS_3_FILTER_BOUNDS: crate::Field = crate::Field::new(20, 0, SFR_FILTER_BOUNDS_3);

        pub const HW_BIO_BDMA_BASE: usize = 0x90000000;
    }
}

// Litex auto-generated constants
pub const LITEX_CONFIG_CLOCK_FREQUENCY: usize = 40000000;
pub const LITEX_CONFIG_CPU_HAS_INTERRUPT: &str = "None";
pub const LITEX_CONFIG_CPU_RESET_ADDR: usize = 268435456;
pub const LITEX_CONFIG_CPU_HAS_DCACHE: &str = "None";
pub const LITEX_CONFIG_CPU_HAS_ICACHE: &str = "None";
pub const LITEX_CONFIG_CPU_TYPE_VEXRISCV: &str = "None";
pub const LITEX_CONFIG_CPU_VARIANT_IMAC: &str = "None";
pub const LITEX_CONFIG_CPU_HUMAN_NAME: &str = "VexRiscv_IMACDebug";
pub const LITEX_CONFIG_CPU_NOP: &str = "nop";
pub const LITEX_CONFIG_CSR_DATA_WIDTH: usize = 32;
pub const LITEX_CONFIG_CSR_ALIGNMENT: usize = 32;
pub const LITEX_CONFIG_BUS_STANDARD: &str = "WISHBONE";
pub const LITEX_CONFIG_BUS_DATA_WIDTH: usize = 32;
pub const LITEX_CONFIG_BUS_ADDRESS_WIDTH: usize = 32;
pub const LITEX_CONFIG_BUS_BURSTING: usize = 0;
pub const LITEX_TIMER0_INTERRUPT: usize = 0;
pub const LITEX_UART_INTERRUPT: usize = 1;


#[cfg(test)]
mod tests {

    #[test]
    #[ignore]
    fn compile_check_rgb_csr() {
        use super::*;
        let mut rgb_csr = CSR::new(HW_RGB_BASE as *mut u32);

        let foo = rgb_csr.r(utra::rgb::OUT);
        rgb_csr.wo(utra::rgb::OUT, foo);
        let bar = rgb_csr.rf(utra::rgb::OUT_OUT);
        rgb_csr.rmwf(utra::rgb::OUT_OUT, bar);
        let mut baz = rgb_csr.zf(utra::rgb::OUT_OUT, bar);
        baz |= rgb_csr.ms(utra::rgb::OUT_OUT, 1);
        rgb_csr.wfo(utra::rgb::OUT_OUT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_ctrl_csr() {
        use super::*;
        let mut ctrl_csr = CSR::new(HW_CTRL_BASE as *mut u32);

        let foo = ctrl_csr.r(utra::ctrl::RESET);
        ctrl_csr.wo(utra::ctrl::RESET, foo);
        let bar = ctrl_csr.rf(utra::ctrl::RESET_SOC_RST);
        ctrl_csr.rmwf(utra::ctrl::RESET_SOC_RST, bar);
        let mut baz = ctrl_csr.zf(utra::ctrl::RESET_SOC_RST, bar);
        baz |= ctrl_csr.ms(utra::ctrl::RESET_SOC_RST, 1);
        ctrl_csr.wfo(utra::ctrl::RESET_SOC_RST, baz);
        let bar = ctrl_csr.rf(utra::ctrl::RESET_CPU_RST);
        ctrl_csr.rmwf(utra::ctrl::RESET_CPU_RST, bar);
        let mut baz = ctrl_csr.zf(utra::ctrl::RESET_CPU_RST, bar);
        baz |= ctrl_csr.ms(utra::ctrl::RESET_CPU_RST, 1);
        ctrl_csr.wfo(utra::ctrl::RESET_CPU_RST, baz);

        let foo = ctrl_csr.r(utra::ctrl::SCRATCH);
        ctrl_csr.wo(utra::ctrl::SCRATCH, foo);
        let bar = ctrl_csr.rf(utra::ctrl::SCRATCH_SCRATCH);
        ctrl_csr.rmwf(utra::ctrl::SCRATCH_SCRATCH, bar);
        let mut baz = ctrl_csr.zf(utra::ctrl::SCRATCH_SCRATCH, bar);
        baz |= ctrl_csr.ms(utra::ctrl::SCRATCH_SCRATCH, 1);
        ctrl_csr.wfo(utra::ctrl::SCRATCH_SCRATCH, baz);

        let foo = ctrl_csr.r(utra::ctrl::BUS_ERRORS);
        ctrl_csr.wo(utra::ctrl::BUS_ERRORS, foo);
        let bar = ctrl_csr.rf(utra::ctrl::BUS_ERRORS_BUS_ERRORS);
        ctrl_csr.rmwf(utra::ctrl::BUS_ERRORS_BUS_ERRORS, bar);
        let mut baz = ctrl_csr.zf(utra::ctrl::BUS_ERRORS_BUS_ERRORS, bar);
        baz |= ctrl_csr.ms(utra::ctrl::BUS_ERRORS_BUS_ERRORS, 1);
        ctrl_csr.wfo(utra::ctrl::BUS_ERRORS_BUS_ERRORS, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_identifier_mem_csr() {
        use super::*;
        let mut identifier_mem_csr = CSR::new(HW_IDENTIFIER_MEM_BASE as *mut u32);

        let foo = identifier_mem_csr.r(utra::identifier_mem::IDENTIFIER_MEM);
        identifier_mem_csr.wo(utra::identifier_mem::IDENTIFIER_MEM, foo);
        let bar = identifier_mem_csr.rf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM);
        identifier_mem_csr.rmwf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, bar);
        let mut baz = identifier_mem_csr.zf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, bar);
        baz |= identifier_mem_csr.ms(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, 1);
        identifier_mem_csr.wfo(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_leds_csr() {
        use super::*;
        let mut leds_csr = CSR::new(HW_LEDS_BASE as *mut u32);

        let foo = leds_csr.r(utra::leds::OUT);
        leds_csr.wo(utra::leds::OUT, foo);
        let bar = leds_csr.rf(utra::leds::OUT_OUT);
        leds_csr.rmwf(utra::leds::OUT_OUT, bar);
        let mut baz = leds_csr.zf(utra::leds::OUT_OUT, bar);
        baz |= leds_csr.ms(utra::leds::OUT_OUT, 1);
        leds_csr.wfo(utra::leds::OUT_OUT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_timer0_csr() {
        use super::*;
        let mut timer0_csr = CSR::new(HW_TIMER0_BASE as *mut u32);

        let foo = timer0_csr.r(utra::timer0::LOAD);
        timer0_csr.wo(utra::timer0::LOAD, foo);
        let bar = timer0_csr.rf(utra::timer0::LOAD_LOAD);
        timer0_csr.rmwf(utra::timer0::LOAD_LOAD, bar);
        let mut baz = timer0_csr.zf(utra::timer0::LOAD_LOAD, bar);
        baz |= timer0_csr.ms(utra::timer0::LOAD_LOAD, 1);
        timer0_csr.wfo(utra::timer0::LOAD_LOAD, baz);

        let foo = timer0_csr.r(utra::timer0::RELOAD);
        timer0_csr.wo(utra::timer0::RELOAD, foo);
        let bar = timer0_csr.rf(utra::timer0::RELOAD_RELOAD);
        timer0_csr.rmwf(utra::timer0::RELOAD_RELOAD, bar);
        let mut baz = timer0_csr.zf(utra::timer0::RELOAD_RELOAD, bar);
        baz |= timer0_csr.ms(utra::timer0::RELOAD_RELOAD, 1);
        timer0_csr.wfo(utra::timer0::RELOAD_RELOAD, baz);

        let foo = timer0_csr.r(utra::timer0::EN);
        timer0_csr.wo(utra::timer0::EN, foo);
        let bar = timer0_csr.rf(utra::timer0::EN_EN);
        timer0_csr.rmwf(utra::timer0::EN_EN, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EN_EN, bar);
        baz |= timer0_csr.ms(utra::timer0::EN_EN, 1);
        timer0_csr.wfo(utra::timer0::EN_EN, baz);

        let foo = timer0_csr.r(utra::timer0::UPDATE_VALUE);
        timer0_csr.wo(utra::timer0::UPDATE_VALUE, foo);
        let bar = timer0_csr.rf(utra::timer0::UPDATE_VALUE_UPDATE_VALUE);
        timer0_csr.rmwf(utra::timer0::UPDATE_VALUE_UPDATE_VALUE, bar);
        let mut baz = timer0_csr.zf(utra::timer0::UPDATE_VALUE_UPDATE_VALUE, bar);
        baz |= timer0_csr.ms(utra::timer0::UPDATE_VALUE_UPDATE_VALUE, 1);
        timer0_csr.wfo(utra::timer0::UPDATE_VALUE_UPDATE_VALUE, baz);

        let foo = timer0_csr.r(utra::timer0::VALUE);
        timer0_csr.wo(utra::timer0::VALUE, foo);
        let bar = timer0_csr.rf(utra::timer0::VALUE_VALUE);
        timer0_csr.rmwf(utra::timer0::VALUE_VALUE, bar);
        let mut baz = timer0_csr.zf(utra::timer0::VALUE_VALUE, bar);
        baz |= timer0_csr.ms(utra::timer0::VALUE_VALUE, 1);
        timer0_csr.wfo(utra::timer0::VALUE_VALUE, baz);

        let foo = timer0_csr.r(utra::timer0::EV_STATUS);
        timer0_csr.wo(utra::timer0::EV_STATUS, foo);
        let bar = timer0_csr.rf(utra::timer0::EV_STATUS_ZERO);
        timer0_csr.rmwf(utra::timer0::EV_STATUS_ZERO, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_STATUS_ZERO, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_STATUS_ZERO, 1);
        timer0_csr.wfo(utra::timer0::EV_STATUS_ZERO, baz);

        let foo = timer0_csr.r(utra::timer0::EV_PENDING);
        timer0_csr.wo(utra::timer0::EV_PENDING, foo);
        let bar = timer0_csr.rf(utra::timer0::EV_PENDING_ZERO);
        timer0_csr.rmwf(utra::timer0::EV_PENDING_ZERO, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_PENDING_ZERO, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_PENDING_ZERO, 1);
        timer0_csr.wfo(utra::timer0::EV_PENDING_ZERO, baz);

        let foo = timer0_csr.r(utra::timer0::EV_ENABLE);
        timer0_csr.wo(utra::timer0::EV_ENABLE, foo);
        let bar = timer0_csr.rf(utra::timer0::EV_ENABLE_ZERO);
        timer0_csr.rmwf(utra::timer0::EV_ENABLE_ZERO, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_ENABLE_ZERO, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_ENABLE_ZERO, 1);
        timer0_csr.wfo(utra::timer0::EV_ENABLE_ZERO, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_uart_csr() {
        use super::*;
        let mut uart_csr = CSR::new(HW_UART_BASE as *mut u32);

        let foo = uart_csr.r(utra::uart::RXTX);
        uart_csr.wo(utra::uart::RXTX, foo);
        let bar = uart_csr.rf(utra::uart::RXTX_RXTX);
        uart_csr.rmwf(utra::uart::RXTX_RXTX, bar);
        let mut baz = uart_csr.zf(utra::uart::RXTX_RXTX, bar);
        baz |= uart_csr.ms(utra::uart::RXTX_RXTX, 1);
        uart_csr.wfo(utra::uart::RXTX_RXTX, baz);

        let foo = uart_csr.r(utra::uart::TXFULL);
        uart_csr.wo(utra::uart::TXFULL, foo);
        let bar = uart_csr.rf(utra::uart::TXFULL_TXFULL);
        uart_csr.rmwf(utra::uart::TXFULL_TXFULL, bar);
        let mut baz = uart_csr.zf(utra::uart::TXFULL_TXFULL, bar);
        baz |= uart_csr.ms(utra::uart::TXFULL_TXFULL, 1);
        uart_csr.wfo(utra::uart::TXFULL_TXFULL, baz);

        let foo = uart_csr.r(utra::uart::RXEMPTY);
        uart_csr.wo(utra::uart::RXEMPTY, foo);
        let bar = uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY);
        uart_csr.rmwf(utra::uart::RXEMPTY_RXEMPTY, bar);
        let mut baz = uart_csr.zf(utra::uart::RXEMPTY_RXEMPTY, bar);
        baz |= uart_csr.ms(utra::uart::RXEMPTY_RXEMPTY, 1);
        uart_csr.wfo(utra::uart::RXEMPTY_RXEMPTY, baz);

        let foo = uart_csr.r(utra::uart::EV_STATUS);
        uart_csr.wo(utra::uart::EV_STATUS, foo);
        let bar = uart_csr.rf(utra::uart::EV_STATUS_TX);
        uart_csr.rmwf(utra::uart::EV_STATUS_TX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_STATUS_TX, bar);
        baz |= uart_csr.ms(utra::uart::EV_STATUS_TX, 1);
        uart_csr.wfo(utra::uart::EV_STATUS_TX, baz);
        let bar = uart_csr.rf(utra::uart::EV_STATUS_RX);
        uart_csr.rmwf(utra::uart::EV_STATUS_RX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_STATUS_RX, bar);
        baz |= uart_csr.ms(utra::uart::EV_STATUS_RX, 1);
        uart_csr.wfo(utra::uart::EV_STATUS_RX, baz);

        let foo = uart_csr.r(utra::uart::EV_PENDING);
        uart_csr.wo(utra::uart::EV_PENDING, foo);
        let bar = uart_csr.rf(utra::uart::EV_PENDING_TX);
        uart_csr.rmwf(utra::uart::EV_PENDING_TX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_PENDING_TX, bar);
        baz |= uart_csr.ms(utra::uart::EV_PENDING_TX, 1);
        uart_csr.wfo(utra::uart::EV_PENDING_TX, baz);
        let bar = uart_csr.rf(utra::uart::EV_PENDING_RX);
        uart_csr.rmwf(utra::uart::EV_PENDING_RX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_PENDING_RX, bar);
        baz |= uart_csr.ms(utra::uart::EV_PENDING_RX, 1);
        uart_csr.wfo(utra::uart::EV_PENDING_RX, baz);

        let foo = uart_csr.r(utra::uart::EV_ENABLE);
        uart_csr.wo(utra::uart::EV_ENABLE, foo);
        let bar = uart_csr.rf(utra::uart::EV_ENABLE_TX);
        uart_csr.rmwf(utra::uart::EV_ENABLE_TX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_ENABLE_TX, bar);
        baz |= uart_csr.ms(utra::uart::EV_ENABLE_TX, 1);
        uart_csr.wfo(utra::uart::EV_ENABLE_TX, baz);
        let bar = uart_csr.rf(utra::uart::EV_ENABLE_RX);
        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_ENABLE_RX, bar);
        baz |= uart_csr.ms(utra::uart::EV_ENABLE_RX, 1);
        uart_csr.wfo(utra::uart::EV_ENABLE_RX, baz);

        let foo = uart_csr.r(utra::uart::TXEMPTY);
        uart_csr.wo(utra::uart::TXEMPTY, foo);
        let bar = uart_csr.rf(utra::uart::TXEMPTY_TXEMPTY);
        uart_csr.rmwf(utra::uart::TXEMPTY_TXEMPTY, bar);
        let mut baz = uart_csr.zf(utra::uart::TXEMPTY_TXEMPTY, bar);
        baz |= uart_csr.ms(utra::uart::TXEMPTY_TXEMPTY, 1);
        uart_csr.wfo(utra::uart::TXEMPTY_TXEMPTY, baz);

        let foo = uart_csr.r(utra::uart::RXFULL);
        uart_csr.wo(utra::uart::RXFULL, foo);
        let bar = uart_csr.rf(utra::uart::RXFULL_RXFULL);
        uart_csr.rmwf(utra::uart::RXFULL_RXFULL, bar);
        let mut baz = uart_csr.zf(utra::uart::RXFULL_RXFULL, bar);
        baz |= uart_csr.ms(utra::uart::RXFULL_RXFULL, 1);
        uart_csr.wfo(utra::uart::RXFULL_RXFULL, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_bio_bdma_csr() {
        use super::*;
        let mut bio_bdma_csr = CSR::new(HW_BIO_BDMA_BASE as *mut u32);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_CTRL);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_CTRL, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CTRL_EN);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CTRL_EN, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CTRL_EN, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CTRL_EN, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CTRL_EN, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CTRL_RESTART);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CTRL_RESTART, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CTRL_RESTART, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CTRL_RESTART, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CTRL_RESTART, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_CFGINFO);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_CFGINFO, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CFGINFO_CONSTANT0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CFGINFO_CONSTANT0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CFGINFO_CONSTANT0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CFGINFO_CONSTANT0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CFGINFO_CONSTANT0, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CFGINFO_CONSTANT1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CFGINFO_CONSTANT1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CFGINFO_CONSTANT1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CFGINFO_CONSTANT1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CFGINFO_CONSTANT1, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CFGINFO_CONSTANT2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CFGINFO_CONSTANT2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CFGINFO_CONSTANT2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CFGINFO_CONSTANT2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CFGINFO_CONSTANT2, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_CONFIG);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_CONFIG, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FLEVEL);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FLEVEL, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_TXF0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_TXF0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_TXF0_FDIN);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_TXF0_FDIN, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_TXF0_FDIN, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_TXF0_FDIN, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_TXF0_FDIN, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_TXF1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_TXF1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_TXF1_FDIN);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_TXF1_FDIN, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_TXF1_FDIN, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_TXF1_FDIN, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_TXF1_FDIN, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_TXF2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_TXF2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_TXF2_FDIN);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_TXF2_FDIN, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_TXF2_FDIN, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_TXF2_FDIN, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_TXF2_FDIN, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_TXF3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_TXF3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_TXF3_FDIN);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_TXF3_FDIN, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_TXF3_FDIN, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_TXF3_FDIN, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_TXF3_FDIN, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_RXF0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_RXF0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_RXF0_FDOUT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_RXF0_FDOUT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_RXF0_FDOUT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_RXF0_FDOUT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_RXF0_FDOUT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_RXF1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_RXF1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_RXF1_FDOUT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_RXF1_FDOUT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_RXF1_FDOUT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_RXF1_FDOUT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_RXF1_FDOUT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_RXF2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_RXF2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_RXF2_FDOUT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_RXF2_FDOUT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_RXF2_FDOUT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_RXF2_FDOUT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_RXF2_FDOUT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_RXF3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_RXF3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_RXF3_FDOUT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_RXF3_FDOUT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_RXF3_FDOUT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_RXF3_FDOUT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_RXF3_FDOUT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_ELEVEL);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_ELEVEL, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL0, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL1, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL2, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL3, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL5);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL5, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL5, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL5, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL5, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL6);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL6, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL6, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL6, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL6, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL7);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL7, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL7, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL7, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL7, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_ETYPE);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_ETYPE, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_LT_MASK);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_LT_MASK, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_LT_MASK, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_LT_MASK, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_LT_MASK, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_EVENT_SET);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_EVENT_SET, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EVENT_SET_SFR_EVENT_SET);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EVENT_SET_SFR_EVENT_SET, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EVENT_SET_SFR_EVENT_SET, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EVENT_SET_SFR_EVENT_SET, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EVENT_SET_SFR_EVENT_SET, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_EVENT_CLR);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_EVENT_CLR, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_EVENT_STATUS);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_EVENT_STATUS, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_EXTCLOCK);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_EXTCLOCK, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EXTCLOCK_USE_EXTCLK, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_0, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_1, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_2, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_EXTCLOCK_EXTCLK_GPIO_3, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FIFO_CLR);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FIFO_CLR, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_QDIV0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_QDIV0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV0_UNUSED_DIV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV0_UNUSED_DIV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV0_UNUSED_DIV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV0_UNUSED_DIV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV0_UNUSED_DIV, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV0_DIV_FRAC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV0_DIV_FRAC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV0_DIV_FRAC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV0_DIV_FRAC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV0_DIV_FRAC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV0_DIV_INT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV0_DIV_INT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV0_DIV_INT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV0_DIV_INT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV0_DIV_INT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_QDIV1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_QDIV1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV1_UNUSED_DIV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV1_UNUSED_DIV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV1_UNUSED_DIV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV1_UNUSED_DIV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV1_UNUSED_DIV, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV1_DIV_FRAC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV1_DIV_FRAC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV1_DIV_FRAC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV1_DIV_FRAC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV1_DIV_FRAC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV1_DIV_INT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV1_DIV_INT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV1_DIV_INT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV1_DIV_INT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV1_DIV_INT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_QDIV2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_QDIV2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV2_UNUSED_DIV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV2_UNUSED_DIV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV2_UNUSED_DIV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV2_UNUSED_DIV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV2_UNUSED_DIV, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV2_DIV_FRAC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV2_DIV_FRAC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV2_DIV_FRAC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV2_DIV_FRAC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV2_DIV_FRAC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV2_DIV_INT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV2_DIV_INT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV2_DIV_INT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV2_DIV_INT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV2_DIV_INT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_QDIV3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_QDIV3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV3_UNUSED_DIV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV3_UNUSED_DIV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV3_UNUSED_DIV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV3_UNUSED_DIV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV3_UNUSED_DIV, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV3_DIV_FRAC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV3_DIV_FRAC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV3_DIV_FRAC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV3_DIV_FRAC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV3_DIV_FRAC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_QDIV3_DIV_INT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_QDIV3_DIV_INT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_QDIV3_DIV_INT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_QDIV3_DIV_INT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_QDIV3_DIV_INT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_SYNC_BYPASS);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_SYNC_BYPASS, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_SYNC_BYPASS_SFR_SYNC_BYPASS, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IO_OE_INV);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IO_OE_INV, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IO_OE_INV_SFR_IO_OE_INV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IO_OE_INV_SFR_IO_OE_INV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IO_OE_INV_SFR_IO_OE_INV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IO_OE_INV_SFR_IO_OE_INV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IO_OE_INV_SFR_IO_OE_INV, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IO_O_INV);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IO_O_INV, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IO_O_INV_SFR_IO_O_INV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IO_O_INV_SFR_IO_O_INV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IO_O_INV_SFR_IO_O_INV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IO_O_INV_SFR_IO_O_INV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IO_O_INV_SFR_IO_O_INV, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IO_I_INV);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IO_I_INV, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IO_I_INV_SFR_IO_I_INV);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IO_I_INV_SFR_IO_I_INV, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IO_I_INV_SFR_IO_I_INV, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IO_I_INV_SFR_IO_I_INV, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IO_I_INV_SFR_IO_I_INV, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IRQMASK_0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IRQMASK_0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IRQMASK_0_SFR_IRQMASK_0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IRQMASK_0_SFR_IRQMASK_0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IRQMASK_0_SFR_IRQMASK_0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IRQMASK_0_SFR_IRQMASK_0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IRQMASK_0_SFR_IRQMASK_0, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IRQMASK_1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IRQMASK_1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IRQMASK_1_SFR_IRQMASK_1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IRQMASK_1_SFR_IRQMASK_1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IRQMASK_1_SFR_IRQMASK_1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IRQMASK_1_SFR_IRQMASK_1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IRQMASK_1_SFR_IRQMASK_1, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IRQMASK_2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IRQMASK_2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IRQMASK_2_SFR_IRQMASK_2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IRQMASK_2_SFR_IRQMASK_2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IRQMASK_2_SFR_IRQMASK_2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IRQMASK_2_SFR_IRQMASK_2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IRQMASK_2_SFR_IRQMASK_2, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IRQMASK_3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IRQMASK_3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IRQMASK_3_SFR_IRQMASK_3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IRQMASK_3_SFR_IRQMASK_3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IRQMASK_3_SFR_IRQMASK_3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IRQMASK_3_SFR_IRQMASK_3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IRQMASK_3_SFR_IRQMASK_3, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_IRQ_EDGE);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_IRQ_EDGE, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_IRQ_EDGE_SFR_IRQ_EDGE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_IRQ_EDGE_SFR_IRQ_EDGE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_IRQ_EDGE_SFR_IRQ_EDGE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_IRQ_EDGE_SFR_IRQ_EDGE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_IRQ_EDGE_SFR_IRQ_EDGE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG_PADOUT);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG_PADOUT, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG_PADOUT_SFR_DBG_PADOUT);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG_PADOUT_SFR_DBG_PADOUT, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG_PADOUT_SFR_DBG_PADOUT, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG_PADOUT_SFR_DBG_PADOUT, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG_PADOUT_SFR_DBG_PADOUT, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG_PADOE);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG_PADOE, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG_PADOE_SFR_DBG_PADOE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG_PADOE_SFR_DBG_PADOE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG_PADOE_SFR_DBG_PADOE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG_PADOE_SFR_DBG_PADOE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG_PADOE_SFR_DBG_PADOE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG0_DBG_PC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG0_DBG_PC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG0_DBG_PC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG0_DBG_PC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG0_DBG_PC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG0_TRAP);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG0_TRAP, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG0_TRAP, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG0_TRAP, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG0_TRAP, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG1_DBG_PC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG1_DBG_PC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG1_DBG_PC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG1_DBG_PC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG1_DBG_PC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG1_TRAP);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG1_TRAP, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG1_TRAP, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG1_TRAP, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG1_TRAP, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG2_DBG_PC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG2_DBG_PC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG2_DBG_PC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG2_DBG_PC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG2_DBG_PC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG2_TRAP);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG2_TRAP, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG2_TRAP, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG2_TRAP, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG2_TRAP, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DBG3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DBG3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG3_DBG_PC);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG3_DBG_PC, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG3_DBG_PC, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG3_DBG_PC, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG3_DBG_PC, baz);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DBG3_TRAP);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DBG3_TRAP, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DBG3_TRAP, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DBG3_TRAP, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DBG3_TRAP, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_MEM_GUTTER);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_MEM_GUTTER, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_MEM_GUTTER_SFR_MEM_GUTTER);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_MEM_GUTTER_SFR_MEM_GUTTER, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_MEM_GUTTER_SFR_MEM_GUTTER, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_MEM_GUTTER_SFR_MEM_GUTTER, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_MEM_GUTTER_SFR_MEM_GUTTER, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_PERI_GUTTER);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_PERI_GUTTER, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_PERI_GUTTER_SFR_PERI_GUTTER);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_PERI_GUTTER_SFR_PERI_GUTTER, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_PERI_GUTTER_SFR_PERI_GUTTER, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_PERI_GUTTER_SFR_PERI_GUTTER, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_PERI_GUTTER_SFR_PERI_GUTTER, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP0_CR_EVMAP0, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP1_CR_EVMAP1, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP2_CR_EVMAP2, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP3_CR_EVMAP3, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP4_CR_EVMAP4, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5_CR_EVMAP5, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT0_SR_EVSTAT0, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT1_SR_EVSTAT1, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT2_SR_EVSTAT2, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT3_SR_EVSTAT3, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT4_SR_EVSTAT4, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5_SR_EVSTAT5, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BASE_0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BASE_0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BASE_0_FILTER_BASE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BASE_0_FILTER_BASE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BASE_0_FILTER_BASE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BASE_0_FILTER_BASE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BASE_0_FILTER_BASE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BOUNDS_0);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BOUNDS_0, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BOUNDS_0_FILTER_BOUNDS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BOUNDS_0_FILTER_BOUNDS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BOUNDS_0_FILTER_BOUNDS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BOUNDS_0_FILTER_BOUNDS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BOUNDS_0_FILTER_BOUNDS, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BASE_1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BASE_1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BASE_1_FILTER_BASE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BASE_1_FILTER_BASE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BASE_1_FILTER_BASE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BASE_1_FILTER_BASE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BASE_1_FILTER_BASE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BOUNDS_1);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BOUNDS_1, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BOUNDS_1_FILTER_BOUNDS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BOUNDS_1_FILTER_BOUNDS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BOUNDS_1_FILTER_BOUNDS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BOUNDS_1_FILTER_BOUNDS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BOUNDS_1_FILTER_BOUNDS, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BASE_2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BASE_2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BASE_2_FILTER_BASE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BASE_2_FILTER_BASE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BASE_2_FILTER_BASE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BASE_2_FILTER_BASE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BASE_2_FILTER_BASE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BOUNDS_2);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BOUNDS_2, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BOUNDS_2_FILTER_BOUNDS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BOUNDS_2_FILTER_BOUNDS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BOUNDS_2_FILTER_BOUNDS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BOUNDS_2_FILTER_BOUNDS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BOUNDS_2_FILTER_BOUNDS, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BASE_3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BASE_3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BASE_3_FILTER_BASE);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BASE_3_FILTER_BASE, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BASE_3_FILTER_BASE, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BASE_3_FILTER_BASE, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BASE_3_FILTER_BASE, baz);

        let foo = bio_bdma_csr.r(utra::bio_bdma::SFR_FILTER_BOUNDS_3);
        bio_bdma_csr.wo(utra::bio_bdma::SFR_FILTER_BOUNDS_3, foo);
        let bar = bio_bdma_csr.rf(utra::bio_bdma::SFR_FILTER_BOUNDS_3_FILTER_BOUNDS);
        bio_bdma_csr.rmwf(utra::bio_bdma::SFR_FILTER_BOUNDS_3_FILTER_BOUNDS, bar);
        let mut baz = bio_bdma_csr.zf(utra::bio_bdma::SFR_FILTER_BOUNDS_3_FILTER_BOUNDS, bar);
        baz |= bio_bdma_csr.ms(utra::bio_bdma::SFR_FILTER_BOUNDS_3_FILTER_BOUNDS, 1);
        bio_bdma_csr.wfo(utra::bio_bdma::SFR_FILTER_BOUNDS_3_FILTER_BOUNDS, baz);
  }
}
