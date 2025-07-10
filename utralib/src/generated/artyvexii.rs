
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
pub const HW_ROM_MEM:     usize = 0x80000000;
pub const HW_ROM_MEM_LEN: usize = 65536;
pub const HW_SRAM_MEM:     usize = 0x01000000;
pub const HW_SRAM_MEM_LEN: usize = 8192;
pub const HW_MAIN_RAM_MEM:     usize = 0x40000000;
pub const HW_MAIN_RAM_MEM_LEN: usize = 131072;
pub const HW_BIO_MEM:     usize = 0x90000000;
pub const HW_BIO_MEM_LEN: usize = 65536;
pub const HW_CSR_MEM:     usize = 0xe0000000;
pub const HW_CSR_MEM_LEN: usize = 65536;

// Physical base addresses of registers
pub const HW_LEGACY_INT_BASE :   usize = 0xe0000000;
pub const HW_RGB_BASE :   usize = 0xe0000800;
pub const HW_CTRL_BASE :   usize = 0xe0001000;
pub const HW_IDENTIFIER_MEM_BASE :   usize = 0xe0001800;
pub const HW_LEDS_BASE :   usize = 0xe0002000;
pub const HW_TIMER0_BASE :   usize = 0xe0002800;
pub const HW_UART_BASE :   usize = 0xe0003000;


pub mod utra {

    pub mod legacy_int {
        pub const LEGACY_INT_NUMREGS: usize = 4;

        pub const MACH_MASK: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const MACH_MASK_MACH_MASK: crate::Field = crate::Field::new(32, 0, MACH_MASK);

        pub const MACH_PENDING: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const MACH_PENDING_MACH_PENDING: crate::Field = crate::Field::new(32, 0, MACH_PENDING);

        pub const SUPER_MASK: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const SUPER_MASK_SUPER_MASK: crate::Field = crate::Field::new(32, 0, SUPER_MASK);

        pub const SUPER_PENDING: crate::Register = crate::Register::new(3, 0xffffffff);
        pub const SUPER_PENDING_SUPER_PENDING: crate::Field = crate::Field::new(32, 0, SUPER_PENDING);

        pub const HW_LEGACY_INT_BASE: usize = 0xe0000000;
    }

    pub mod rgb {
        pub const RGB_NUMREGS: usize = 1;

        pub const OUT: crate::Register = crate::Register::new(0, 0xfff);
        pub const OUT_OUT: crate::Field = crate::Field::new(12, 0, OUT);

        pub const HW_RGB_BASE: usize = 0xe0000800;
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

        pub const HW_CTRL_BASE: usize = 0xe0001000;
    }

    pub mod identifier_mem {
        pub const IDENTIFIER_MEM_NUMREGS: usize = 1;

        pub const IDENTIFIER_MEM: crate::Register = crate::Register::new(0, 0xff);
        pub const IDENTIFIER_MEM_IDENTIFIER_MEM: crate::Field = crate::Field::new(8, 0, IDENTIFIER_MEM);

        pub const HW_IDENTIFIER_MEM_BASE: usize = 0xe0001800;
    }

    pub mod leds {
        pub const LEDS_NUMREGS: usize = 1;

        pub const OUT: crate::Register = crate::Register::new(0, 0xf);
        pub const OUT_OUT: crate::Field = crate::Field::new(4, 0, OUT);

        pub const HW_LEDS_BASE: usize = 0xe0002000;
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
        pub const HW_TIMER0_BASE: usize = 0xe0002800;
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
        pub const HW_UART_BASE: usize = 0xe0003000;
    }
}

// Litex auto-generated constants
pub const LITEX_CONFIG_CLOCK_FREQUENCY: usize = 40000000;
pub const LITEX_CONFIG_CPU_HAS_INTERRUPT: &str = "None";
pub const LITEX_CONFIG_CPU_RESET_ADDR: usize = 2147483648;
pub const LITEX_CONFIG_CPU_HAS_DCACHE: &str = "None";
pub const LITEX_CONFIG_CPU_HAS_ICACHE: &str = "None";
pub const LITEX_CONFIG_CPU_TYPE_VEXIIRISCV: &str = "None";
pub const LITEX_CONFIG_CPU_VARIANT_STANDARD: &str = "None";
pub const LITEX_CONFIG_CPU_HUMAN_NAME: &str = "VexiiRiscv";
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
    fn compile_check_legacy_int_csr() {
        use super::*;
        let mut legacy_int_csr = CSR::new(HW_LEGACY_INT_BASE as *mut u32);

        let foo = legacy_int_csr.r(utra::legacy_int::MACH_MASK);
        legacy_int_csr.wo(utra::legacy_int::MACH_MASK, foo);
        let bar = legacy_int_csr.rf(utra::legacy_int::MACH_MASK_MACH_MASK);
        legacy_int_csr.rmwf(utra::legacy_int::MACH_MASK_MACH_MASK, bar);
        let mut baz = legacy_int_csr.zf(utra::legacy_int::MACH_MASK_MACH_MASK, bar);
        baz |= legacy_int_csr.ms(utra::legacy_int::MACH_MASK_MACH_MASK, 1);
        legacy_int_csr.wfo(utra::legacy_int::MACH_MASK_MACH_MASK, baz);

        let foo = legacy_int_csr.r(utra::legacy_int::MACH_PENDING);
        legacy_int_csr.wo(utra::legacy_int::MACH_PENDING, foo);
        let bar = legacy_int_csr.rf(utra::legacy_int::MACH_PENDING_MACH_PENDING);
        legacy_int_csr.rmwf(utra::legacy_int::MACH_PENDING_MACH_PENDING, bar);
        let mut baz = legacy_int_csr.zf(utra::legacy_int::MACH_PENDING_MACH_PENDING, bar);
        baz |= legacy_int_csr.ms(utra::legacy_int::MACH_PENDING_MACH_PENDING, 1);
        legacy_int_csr.wfo(utra::legacy_int::MACH_PENDING_MACH_PENDING, baz);

        let foo = legacy_int_csr.r(utra::legacy_int::SUPER_MASK);
        legacy_int_csr.wo(utra::legacy_int::SUPER_MASK, foo);
        let bar = legacy_int_csr.rf(utra::legacy_int::SUPER_MASK_SUPER_MASK);
        legacy_int_csr.rmwf(utra::legacy_int::SUPER_MASK_SUPER_MASK, bar);
        let mut baz = legacy_int_csr.zf(utra::legacy_int::SUPER_MASK_SUPER_MASK, bar);
        baz |= legacy_int_csr.ms(utra::legacy_int::SUPER_MASK_SUPER_MASK, 1);
        legacy_int_csr.wfo(utra::legacy_int::SUPER_MASK_SUPER_MASK, baz);

        let foo = legacy_int_csr.r(utra::legacy_int::SUPER_PENDING);
        legacy_int_csr.wo(utra::legacy_int::SUPER_PENDING, foo);
        let bar = legacy_int_csr.rf(utra::legacy_int::SUPER_PENDING_SUPER_PENDING);
        legacy_int_csr.rmwf(utra::legacy_int::SUPER_PENDING_SUPER_PENDING, bar);
        let mut baz = legacy_int_csr.zf(utra::legacy_int::SUPER_PENDING_SUPER_PENDING, bar);
        baz |= legacy_int_csr.ms(utra::legacy_int::SUPER_PENDING_SUPER_PENDING, 1);
        legacy_int_csr.wfo(utra::legacy_int::SUPER_PENDING_SUPER_PENDING, baz);
  }

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
}
