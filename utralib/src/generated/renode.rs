
#![allow(dead_code)]
use core::convert::TryInto;
use core::sync::atomic::AtomicPtr;

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
        // Asserts don't work in const fn yet.
        // assert!(width != 0, "field width cannot be 0");
        // assert!((width + offset) < 32, "field with and offset must fit within a 32-bit value");
        // It would be lovely if we could call `usize::pow()` in a const fn.
        let mask = match width {
            0 => 0,
            1 => 1,
            2 => 3,
            3 => 7,
            4 => 15,
            5 => 31,
            6 => 63,
            7 => 127,
            8 => 255,
            9 => 511,
            10 => 1023,
            11 => 2047,
            12 => 4095,
            13 => 8191,
            14 => 16383,
            15 => 32767,
            16 => 65535,
            17 => 131071,
            18 => 262143,
            19 => 524287,
            20 => 1048575,
            21 => 2097151,
            22 => 4194303,
            23 => 8388607,
            24 => 16777215,
            25 => 33554431,
            26 => 67108863,
            27 => 134217727,
            28 => 268435455,
            29 => 536870911,
            30 => 1073741823,
            31 => 2147483647,
            32 => 4294967295,
            _ => 0,
        };
        Field {
            mask,
            offset,
            register,
        }
    }
}
#[derive(Debug, Copy, Clone)]
pub struct CSR<T> {
    pub base: *mut T,
}
impl<T> CSR<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *mut T) -> Self {
        CSR { base }
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
pub struct AtomicCsr<T> {
    pub base: AtomicPtr<T>,
}
impl<T> AtomicCsr<T>
where
    T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
{
    pub fn new(base: *mut T) -> Self {
        AtomicCsr {
            base: AtomicPtr::new(base)
        }
    }
    /// In reality, we should wrap this in an `Arc` so we can be truly safe across a multi-core
    /// implementation, but for our single-core system this is fine. The reason we don't do it
    /// immediately is that UTRA also needs to work in a `no_std` environment, where `Arc`
    /// does not exist, and so additional config flags would need to be introduced to not break
    /// that compability issue. If migrating to multicore, this technical debt would have to be
    /// addressed.
    pub fn clone(&self) -> Self {
        AtomicCsr {
            base: AtomicPtr::new(self.base.load(core::sync::atomic::Ordering::SeqCst))
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
pub const HW_VEXRISCV_DEBUG_MEM:     usize = 0xefff0000;
pub const HW_VEXRISCV_DEBUG_MEM_LEN: usize = 256;
pub const HW_SRAM_EXT_MEM:     usize = 0x40000000;
pub const HW_SRAM_EXT_MEM_LEN: usize = 16777216;
pub const HW_MEMLCD_MEM:     usize = 0xb0000000;
pub const HW_MEMLCD_MEM_LEN: usize = 23584;
pub const HW_SPIFLASH_MEM:     usize = 0x20000000;
pub const HW_SPIFLASH_MEM_LEN: usize = 134217728;
pub const HW_AUDIO_MEM:     usize = 0xe0000000;
pub const HW_AUDIO_MEM_LEN: usize = 4;
pub const HW_SHA512_MEM:     usize = 0xe0002000;
pub const HW_SHA512_MEM_LEN: usize = 4;
pub const HW_ENGINE_MEM:     usize = 0xe0020000;
pub const HW_ENGINE_MEM_LEN: usize = 131072;
pub const HW_USBDEV_MEM:     usize = 0xe0040000;
pub const HW_USBDEV_MEM_LEN: usize = 65536;
pub const HW_CSR_MEM:     usize = 0xf0000000;
pub const HW_CSR_MEM_LEN: usize = 262144;

// Physical base addresses of registers
pub const HW_REBOOT_BASE :   usize = 0xf0000000;
pub const HW_TIMER0_BASE :   usize = 0xf0001000;
pub const HW_CRG_BASE :   usize = 0xf0002000;
pub const HW_GPIO_BASE :   usize = 0xf0003000;
pub const HW_UART_BASE :   usize = 0xf0005000;
pub const HW_CONSOLE_BASE :   usize = 0xf0007000;
pub const HW_APP_UART_BASE :   usize = 0xf0009000;
pub const HW_INFO_BASE :   usize = 0xf000a000;
pub const HW_SRAM_EXT_BASE :   usize = 0xf000b000;
pub const HW_MEMLCD_BASE :   usize = 0xf000c000;
pub const HW_COM_BASE :   usize = 0xf000d000;
pub const HW_I2C_BASE :   usize = 0xf000e000;
pub const HW_BTEVENTS_BASE :   usize = 0xf000f000;
pub const HW_MESSIBLE_BASE :   usize = 0xf0010000;
pub const HW_MESSIBLE2_BASE :   usize = 0xf0011000;
pub const HW_TICKTIMER_BASE :   usize = 0xf0012000;
pub const HW_SUSRES_BASE :   usize = 0xf0013000;
pub const HW_POWER_BASE :   usize = 0xf0014000;
pub const HW_SPINOR_SOFT_INT_BASE :   usize = 0xf0015000;
pub const HW_SPINOR_BASE :   usize = 0xf0016000;
pub const HW_KEYBOARD_BASE :   usize = 0xf0017000;
pub const HW_KEYINJECT_BASE :   usize = 0xf0018000;
pub const HW_SEED_BASE :   usize = 0xf0019000;
pub const HW_KEYROM_BASE :   usize = 0xf001a000;
pub const HW_AUDIO_BASE :   usize = 0xf001b000;
pub const HW_TRNG_KERNEL_BASE :   usize = 0xf001c000;
pub const HW_TRNG_SERVER_BASE :   usize = 0xf001d000;
pub const HW_TRNG_BASE :   usize = 0xf001e000;
pub const HW_SHA512_BASE :   usize = 0xf001f000;
pub const HW_ENGINE_BASE :   usize = 0xf0020000;
pub const HW_JTAG_BASE :   usize = 0xf0021000;
pub const HW_WDT_BASE :   usize = 0xf0022000;
pub const HW_USBDEV_BASE :   usize = 0xf0023000;
pub const HW_D11CTIME_BASE :   usize = 0xf0024000;
pub const HW_WFI_BASE :   usize = 0xf0025000;
pub const HW_IDENTIFIER_MEM_BASE :   usize = 0xf0026000;


pub mod utra {

    pub mod reboot {
        pub const REBOOT_NUMREGS: usize = 3;

        pub const SOC_RESET: crate::Register = crate::Register::new(0, 0xff);
        pub const SOC_RESET_SOC_RESET: crate::Field = crate::Field::new(8, 0, SOC_RESET);

        pub const ADDR: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const ADDR_ADDR: crate::Field = crate::Field::new(32, 0, ADDR);

        pub const CPU_RESET: crate::Register = crate::Register::new(2, 0x1);
        pub const CPU_RESET_CPU_RESET: crate::Field = crate::Field::new(1, 0, CPU_RESET);

        pub const HW_REBOOT_BASE: usize = 0xf0000000;
    }

    pub mod timer0 {
        pub const TIMER0_NUMREGS: usize = 6;

        pub const LOAD: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const LOAD_LOAD: crate::Field = crate::Field::new(32, 0, LOAD);

        pub const RELOAD: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const RELOAD_RELOAD: crate::Field = crate::Field::new(32, 0, RELOAD);

        pub const EN: crate::Register = crate::Register::new(2, 0x1);
        pub const EN_EN: crate::Field = crate::Field::new(1, 0, EN);

        pub const EV_STATUS: crate::Register = crate::Register::new(3, 0x1);
        pub const EV_STATUS_ZERO: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(4, 0x1);
        pub const EV_PENDING_ZERO: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(5, 0x1);
        pub const EV_ENABLE_ZERO: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const TIMER0_IRQ: usize = 0;
        pub const HW_TIMER0_BASE: usize = 0xf0001000;
    }

    pub mod crg {
        pub const CRG_NUMREGS: usize = 8;

        pub const MMCM_DRP_RESET: crate::Register = crate::Register::new(0, 0x1);
        pub const MMCM_DRP_RESET_MMCM_DRP_RESET: crate::Field = crate::Field::new(1, 0, MMCM_DRP_RESET);

        pub const MMCM_DRP_LOCKED: crate::Register = crate::Register::new(1, 0x1);
        pub const MMCM_DRP_LOCKED_MMCM_DRP_LOCKED: crate::Field = crate::Field::new(1, 0, MMCM_DRP_LOCKED);

        pub const MMCM_DRP_READ: crate::Register = crate::Register::new(2, 0x1);
        pub const MMCM_DRP_READ_MMCM_DRP_READ: crate::Field = crate::Field::new(1, 0, MMCM_DRP_READ);

        pub const MMCM_DRP_WRITE: crate::Register = crate::Register::new(3, 0x1);
        pub const MMCM_DRP_WRITE_MMCM_DRP_WRITE: crate::Field = crate::Field::new(1, 0, MMCM_DRP_WRITE);

        pub const MMCM_DRP_DRDY: crate::Register = crate::Register::new(4, 0x1);
        pub const MMCM_DRP_DRDY_MMCM_DRP_DRDY: crate::Field = crate::Field::new(1, 0, MMCM_DRP_DRDY);

        pub const MMCM_DRP_ADR: crate::Register = crate::Register::new(5, 0x7f);
        pub const MMCM_DRP_ADR_MMCM_DRP_ADR: crate::Field = crate::Field::new(7, 0, MMCM_DRP_ADR);

        pub const MMCM_DRP_DAT_W: crate::Register = crate::Register::new(6, 0xffff);
        pub const MMCM_DRP_DAT_W_MMCM_DRP_DAT_W: crate::Field = crate::Field::new(16, 0, MMCM_DRP_DAT_W);

        pub const MMCM_DRP_DAT_R: crate::Register = crate::Register::new(7, 0xffff);
        pub const MMCM_DRP_DAT_R_MMCM_DRP_DAT_R: crate::Field = crate::Field::new(16, 0, MMCM_DRP_DAT_R);

        pub const HW_CRG_BASE: usize = 0xf0002000;
    }

    pub mod gpio {
        pub const GPIO_NUMREGS: usize = 10;

        pub const OUTPUT: crate::Register = crate::Register::new(0, 0xff);
        pub const OUTPUT_OUTPUT: crate::Field = crate::Field::new(8, 0, OUTPUT);

        pub const INPUT: crate::Register = crate::Register::new(1, 0xff);
        pub const INPUT_INPUT: crate::Field = crate::Field::new(8, 0, INPUT);

        pub const DRIVE: crate::Register = crate::Register::new(2, 0xff);
        pub const DRIVE_DRIVE: crate::Field = crate::Field::new(8, 0, DRIVE);

        pub const INTENA: crate::Register = crate::Register::new(3, 0xff);
        pub const INTENA_INTENA: crate::Field = crate::Field::new(8, 0, INTENA);

        pub const INTPOL: crate::Register = crate::Register::new(4, 0xff);
        pub const INTPOL_INTPOL: crate::Field = crate::Field::new(8, 0, INTPOL);

        pub const UARTSEL: crate::Register = crate::Register::new(5, 0x3);
        pub const UARTSEL_UARTSEL: crate::Field = crate::Field::new(2, 0, UARTSEL);

        pub const DEBUG: crate::Register = crate::Register::new(6, 0x3);
        pub const DEBUG_WFI: crate::Field = crate::Field::new(1, 0, DEBUG);
        pub const DEBUG_WAKEUP: crate::Field = crate::Field::new(1, 1, DEBUG);

        pub const EV_STATUS: crate::Register = crate::Register::new(7, 0xff);
        pub const EV_STATUS_EVENT0: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_EVENT1: crate::Field = crate::Field::new(1, 1, EV_STATUS);
        pub const EV_STATUS_EVENT2: crate::Field = crate::Field::new(1, 2, EV_STATUS);
        pub const EV_STATUS_EVENT3: crate::Field = crate::Field::new(1, 3, EV_STATUS);
        pub const EV_STATUS_EVENT4: crate::Field = crate::Field::new(1, 4, EV_STATUS);
        pub const EV_STATUS_EVENT5: crate::Field = crate::Field::new(1, 5, EV_STATUS);
        pub const EV_STATUS_EVENT6: crate::Field = crate::Field::new(1, 6, EV_STATUS);
        pub const EV_STATUS_EVENT7: crate::Field = crate::Field::new(1, 7, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(8, 0xff);
        pub const EV_PENDING_EVENT0: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_EVENT1: crate::Field = crate::Field::new(1, 1, EV_PENDING);
        pub const EV_PENDING_EVENT2: crate::Field = crate::Field::new(1, 2, EV_PENDING);
        pub const EV_PENDING_EVENT3: crate::Field = crate::Field::new(1, 3, EV_PENDING);
        pub const EV_PENDING_EVENT4: crate::Field = crate::Field::new(1, 4, EV_PENDING);
        pub const EV_PENDING_EVENT5: crate::Field = crate::Field::new(1, 5, EV_PENDING);
        pub const EV_PENDING_EVENT6: crate::Field = crate::Field::new(1, 6, EV_PENDING);
        pub const EV_PENDING_EVENT7: crate::Field = crate::Field::new(1, 7, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(9, 0xff);
        pub const EV_ENABLE_EVENT0: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_EVENT1: crate::Field = crate::Field::new(1, 1, EV_ENABLE);
        pub const EV_ENABLE_EVENT2: crate::Field = crate::Field::new(1, 2, EV_ENABLE);
        pub const EV_ENABLE_EVENT3: crate::Field = crate::Field::new(1, 3, EV_ENABLE);
        pub const EV_ENABLE_EVENT4: crate::Field = crate::Field::new(1, 4, EV_ENABLE);
        pub const EV_ENABLE_EVENT5: crate::Field = crate::Field::new(1, 5, EV_ENABLE);
        pub const EV_ENABLE_EVENT6: crate::Field = crate::Field::new(1, 6, EV_ENABLE);
        pub const EV_ENABLE_EVENT7: crate::Field = crate::Field::new(1, 7, EV_ENABLE);

        pub const GPIO_IRQ: usize = 1;
        pub const HW_GPIO_BASE: usize = 0xf0003000;
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

        pub const UART_IRQ: usize = 2;
        pub const HW_UART_BASE: usize = 0xf0005000;
    }

    pub mod console {
        pub const CONSOLE_NUMREGS: usize = 8;

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

        pub const CONSOLE_IRQ: usize = 3;
        pub const HW_CONSOLE_BASE: usize = 0xf0007000;
    }

    pub mod app_uart {
        pub const APP_UART_NUMREGS: usize = 8;

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

        pub const APP_UART_IRQ: usize = 4;
        pub const HW_APP_UART_BASE: usize = 0xf0009000;
    }

    pub mod info {
        pub const INFO_NUMREGS: usize = 12;

        pub const DNA_ID1: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const DNA_ID1_DNA_ID: crate::Field = crate::Field::new(32, 0, DNA_ID1);

        pub const DNA_ID0: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const DNA_ID0_DNA_ID: crate::Field = crate::Field::new(32, 0, DNA_ID0);

        pub const GIT_MAJOR: crate::Register = crate::Register::new(2, 0xff);
        pub const GIT_MAJOR_GIT_MAJOR: crate::Field = crate::Field::new(8, 0, GIT_MAJOR);

        pub const GIT_MINOR: crate::Register = crate::Register::new(3, 0xff);
        pub const GIT_MINOR_GIT_MINOR: crate::Field = crate::Field::new(8, 0, GIT_MINOR);

        pub const GIT_REVISION: crate::Register = crate::Register::new(4, 0xff);
        pub const GIT_REVISION_GIT_REVISION: crate::Field = crate::Field::new(8, 0, GIT_REVISION);

        pub const GIT_GITREV: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const GIT_GITREV_GIT_GITREV: crate::Field = crate::Field::new(32, 0, GIT_GITREV);

        pub const GIT_GITEXTRA: crate::Register = crate::Register::new(6, 0x3ff);
        pub const GIT_GITEXTRA_GIT_GITEXTRA: crate::Field = crate::Field::new(10, 0, GIT_GITEXTRA);

        pub const GIT_DIRTY: crate::Register = crate::Register::new(7, 0x1);
        pub const GIT_DIRTY_DIRTY: crate::Field = crate::Field::new(1, 0, GIT_DIRTY);

        pub const PLATFORM_PLATFORM1: crate::Register = crate::Register::new(8, 0xffffffff);
        pub const PLATFORM_PLATFORM1_PLATFORM_PLATFORM: crate::Field = crate::Field::new(32, 0, PLATFORM_PLATFORM1);

        pub const PLATFORM_PLATFORM0: crate::Register = crate::Register::new(9, 0xffffffff);
        pub const PLATFORM_PLATFORM0_PLATFORM_PLATFORM: crate::Field = crate::Field::new(32, 0, PLATFORM_PLATFORM0);

        pub const PLATFORM_TARGET1: crate::Register = crate::Register::new(10, 0xffffffff);
        pub const PLATFORM_TARGET1_PLATFORM_TARGET: crate::Field = crate::Field::new(32, 0, PLATFORM_TARGET1);

        pub const PLATFORM_TARGET0: crate::Register = crate::Register::new(11, 0xffffffff);
        pub const PLATFORM_TARGET0_PLATFORM_TARGET: crate::Field = crate::Field::new(32, 0, PLATFORM_TARGET0);

        pub const HW_INFO_BASE: usize = 0xf000a000;
    }

    pub mod sram_ext {
        pub const SRAM_EXT_NUMREGS: usize = 2;

        pub const CONFIG_STATUS: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const CONFIG_STATUS_MODE: crate::Field = crate::Field::new(32, 0, CONFIG_STATUS);

        pub const READ_CONFIG: crate::Register = crate::Register::new(1, 0x1);
        pub const READ_CONFIG_TRIGGER: crate::Field = crate::Field::new(1, 0, READ_CONFIG);

        pub const HW_SRAM_EXT_BASE: usize = 0xf000b000;
    }

    pub mod memlcd {
        pub const MEMLCD_NUMREGS: usize = 8;

        pub const COMMAND: crate::Register = crate::Register::new(0, 0x3);
        pub const COMMAND_UPDATEDIRTY: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_UPDATEALL: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const BUSY: crate::Register = crate::Register::new(1, 0x1);
        pub const BUSY_BUSY: crate::Field = crate::Field::new(1, 0, BUSY);

        pub const PRESCALER: crate::Register = crate::Register::new(2, 0xff);
        pub const PRESCALER_PRESCALER: crate::Field = crate::Field::new(8, 0, PRESCALER);

        pub const EV_STATUS: crate::Register = crate::Register::new(3, 0x1);
        pub const EV_STATUS_DONE: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(4, 0x1);
        pub const EV_PENDING_DONE: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(5, 0x1);
        pub const EV_ENABLE_DONE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const DEVBOOT: crate::Register = crate::Register::new(6, 0x1);
        pub const DEVBOOT_DEVBOOT: crate::Field = crate::Field::new(1, 0, DEVBOOT);

        pub const DEVSTATUS: crate::Register = crate::Register::new(7, 0x1);
        pub const DEVSTATUS_DEVSTATUS: crate::Field = crate::Field::new(1, 0, DEVSTATUS);

        pub const HW_MEMLCD_BASE: usize = 0xf000c000;
    }

    pub mod com {
        pub const COM_NUMREGS: usize = 7;

        pub const TX: crate::Register = crate::Register::new(0, 0xffff);
        pub const TX_TX: crate::Field = crate::Field::new(16, 0, TX);

        pub const RX: crate::Register = crate::Register::new(1, 0xffff);
        pub const RX_RX: crate::Field = crate::Field::new(16, 0, RX);

        pub const CONTROL: crate::Register = crate::Register::new(2, 0x3);
        pub const CONTROL_INTENA: crate::Field = crate::Field::new(1, 0, CONTROL);
        pub const CONTROL_AUTOHOLD: crate::Field = crate::Field::new(1, 1, CONTROL);

        pub const STATUS: crate::Register = crate::Register::new(3, 0x3);
        pub const STATUS_TIP: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_HOLD: crate::Field = crate::Field::new(1, 1, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(4, 0x3);
        pub const EV_STATUS_SPI_INT: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_SPI_HOLD: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(5, 0x3);
        pub const EV_PENDING_SPI_INT: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_SPI_HOLD: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(6, 0x3);
        pub const EV_ENABLE_SPI_INT: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_SPI_HOLD: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const COM_IRQ: usize = 5;
        pub const HW_COM_BASE: usize = 0xf000d000;
    }

    pub mod i2c {
        pub const I2C_NUMREGS: usize = 10;

        pub const PRESCALE: crate::Register = crate::Register::new(0, 0xffff);
        pub const PRESCALE_PRESCALE: crate::Field = crate::Field::new(16, 0, PRESCALE);

        pub const CONTROL: crate::Register = crate::Register::new(1, 0xff);
        pub const CONTROL_RESVD: crate::Field = crate::Field::new(6, 0, CONTROL);
        pub const CONTROL_IEN: crate::Field = crate::Field::new(1, 6, CONTROL);
        pub const CONTROL_EN: crate::Field = crate::Field::new(1, 7, CONTROL);

        pub const TXR: crate::Register = crate::Register::new(2, 0xff);
        pub const TXR_TXR: crate::Field = crate::Field::new(8, 0, TXR);

        pub const RXR: crate::Register = crate::Register::new(3, 0xff);
        pub const RXR_RXR: crate::Field = crate::Field::new(8, 0, RXR);

        pub const COMMAND: crate::Register = crate::Register::new(4, 0xff);
        pub const COMMAND_IACK: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_RESVD: crate::Field = crate::Field::new(2, 1, COMMAND);
        pub const COMMAND_ACK: crate::Field = crate::Field::new(1, 3, COMMAND);
        pub const COMMAND_WR: crate::Field = crate::Field::new(1, 4, COMMAND);
        pub const COMMAND_RD: crate::Field = crate::Field::new(1, 5, COMMAND);
        pub const COMMAND_STO: crate::Field = crate::Field::new(1, 6, COMMAND);
        pub const COMMAND_STA: crate::Field = crate::Field::new(1, 7, COMMAND);

        pub const STATUS: crate::Register = crate::Register::new(5, 0xff);
        pub const STATUS_IF: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_TIP: crate::Field = crate::Field::new(1, 1, STATUS);
        pub const STATUS_RESVD: crate::Field = crate::Field::new(3, 2, STATUS);
        pub const STATUS_ARBLOST: crate::Field = crate::Field::new(1, 5, STATUS);
        pub const STATUS_BUSY: crate::Field = crate::Field::new(1, 6, STATUS);
        pub const STATUS_RXACK: crate::Field = crate::Field::new(1, 7, STATUS);

        pub const CORE_RESET: crate::Register = crate::Register::new(6, 0x1);
        pub const CORE_RESET_RESET: crate::Field = crate::Field::new(1, 0, CORE_RESET);

        pub const EV_STATUS: crate::Register = crate::Register::new(7, 0x3);
        pub const EV_STATUS_I2C_INT: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_TXRX_DONE: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(8, 0x3);
        pub const EV_PENDING_I2C_INT: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_TXRX_DONE: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(9, 0x3);
        pub const EV_ENABLE_I2C_INT: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_TXRX_DONE: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const I2C_IRQ: usize = 6;
        pub const HW_I2C_BASE: usize = 0xf000e000;
    }

    pub mod btevents {
        pub const BTEVENTS_NUMREGS: usize = 3;

        pub const EV_STATUS: crate::Register = crate::Register::new(0, 0x3);
        pub const EV_STATUS_COM_INT: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_RTC_INT: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(1, 0x3);
        pub const EV_PENDING_COM_INT: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_RTC_INT: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(2, 0x3);
        pub const EV_ENABLE_COM_INT: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_RTC_INT: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const BTEVENTS_IRQ: usize = 7;
        pub const HW_BTEVENTS_BASE: usize = 0xf000f000;
    }

    pub mod messible {
        pub const MESSIBLE_NUMREGS: usize = 3;

        pub const IN: crate::Register = crate::Register::new(0, 0xff);
        pub const IN_IN: crate::Field = crate::Field::new(8, 0, IN);

        pub const OUT: crate::Register = crate::Register::new(1, 0xff);
        pub const OUT_OUT: crate::Field = crate::Field::new(8, 0, OUT);

        pub const STATUS: crate::Register = crate::Register::new(2, 0x3);
        pub const STATUS_FULL: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_HAVE: crate::Field = crate::Field::new(1, 1, STATUS);

        pub const HW_MESSIBLE_BASE: usize = 0xf0010000;
    }

    pub mod messible2 {
        pub const MESSIBLE2_NUMREGS: usize = 3;

        pub const IN: crate::Register = crate::Register::new(0, 0xff);
        pub const IN_IN: crate::Field = crate::Field::new(8, 0, IN);

        pub const OUT: crate::Register = crate::Register::new(1, 0xff);
        pub const OUT_OUT: crate::Field = crate::Field::new(8, 0, OUT);

        pub const STATUS: crate::Register = crate::Register::new(2, 0x3);
        pub const STATUS_FULL: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_HAVE: crate::Field = crate::Field::new(1, 1, STATUS);

        pub const HW_MESSIBLE2_BASE: usize = 0xf0011000;
    }

    pub mod ticktimer {
        pub const TICKTIMER_NUMREGS: usize = 8;

        pub const CONTROL: crate::Register = crate::Register::new(0, 0x1);
        pub const CONTROL_RESET: crate::Field = crate::Field::new(1, 0, CONTROL);

        pub const TIME1: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const TIME1_TIME: crate::Field = crate::Field::new(32, 0, TIME1);

        pub const TIME0: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const TIME0_TIME: crate::Field = crate::Field::new(32, 0, TIME0);

        pub const MSLEEP_TARGET1: crate::Register = crate::Register::new(3, 0xffffffff);
        pub const MSLEEP_TARGET1_MSLEEP_TARGET: crate::Field = crate::Field::new(32, 0, MSLEEP_TARGET1);

        pub const MSLEEP_TARGET0: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const MSLEEP_TARGET0_MSLEEP_TARGET: crate::Field = crate::Field::new(32, 0, MSLEEP_TARGET0);

        pub const EV_STATUS: crate::Register = crate::Register::new(5, 0x1);
        pub const EV_STATUS_ALARM: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(6, 0x1);
        pub const EV_PENDING_ALARM: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(7, 0x1);
        pub const EV_ENABLE_ALARM: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const TICKTIMER_IRQ: usize = 8;
        pub const HW_TICKTIMER_BASE: usize = 0xf0012000;
    }

    pub mod susres {
        pub const SUSRES_NUMREGS: usize = 13;

        pub const CONTROL: crate::Register = crate::Register::new(0, 0x3);
        pub const CONTROL_PAUSE: crate::Field = crate::Field::new(1, 0, CONTROL);
        pub const CONTROL_LOAD: crate::Field = crate::Field::new(1, 1, CONTROL);

        pub const RESUME_TIME1: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const RESUME_TIME1_RESUME_TIME: crate::Field = crate::Field::new(32, 0, RESUME_TIME1);

        pub const RESUME_TIME0: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const RESUME_TIME0_RESUME_TIME: crate::Field = crate::Field::new(32, 0, RESUME_TIME0);

        pub const TIME1: crate::Register = crate::Register::new(3, 0xffffffff);
        pub const TIME1_TIME: crate::Field = crate::Field::new(32, 0, TIME1);

        pub const TIME0: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const TIME0_TIME: crate::Field = crate::Field::new(32, 0, TIME0);

        pub const STATUS: crate::Register = crate::Register::new(5, 0x1);
        pub const STATUS_PAUSED: crate::Field = crate::Field::new(1, 0, STATUS);

        pub const STATE: crate::Register = crate::Register::new(6, 0x3);
        pub const STATE_RESUME: crate::Field = crate::Field::new(1, 0, STATE);
        pub const STATE_WAS_FORCED: crate::Field = crate::Field::new(1, 1, STATE);

        pub const POWERDOWN: crate::Register = crate::Register::new(7, 0x1);
        pub const POWERDOWN_POWERDOWN: crate::Field = crate::Field::new(1, 0, POWERDOWN);

        pub const WFI: crate::Register = crate::Register::new(8, 0x1);
        pub const WFI_OVERRIDE: crate::Field = crate::Field::new(1, 0, WFI);

        pub const INTERRUPT: crate::Register = crate::Register::new(9, 0x1);
        pub const INTERRUPT_INTERRUPT: crate::Field = crate::Field::new(1, 0, INTERRUPT);

        pub const EV_STATUS: crate::Register = crate::Register::new(10, 0x1);
        pub const EV_STATUS_SOFT_INT: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(11, 0x1);
        pub const EV_PENDING_SOFT_INT: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(12, 0x1);
        pub const EV_ENABLE_SOFT_INT: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const SUSRES_IRQ: usize = 9;
        pub const HW_SUSRES_BASE: usize = 0xf0013000;
    }

    pub mod power {
        pub const POWER_NUMREGS: usize = 9;

        pub const POWER: crate::Register = crate::Register::new(0, 0xfff);
        pub const POWER_AUDIO: crate::Field = crate::Field::new(1, 0, POWER);
        pub const POWER_SELF: crate::Field = crate::Field::new(1, 1, POWER);
        pub const POWER_EC_SNOOP: crate::Field = crate::Field::new(1, 2, POWER);
        pub const POWER_STATE: crate::Field = crate::Field::new(2, 3, POWER);
        pub const POWER_RESET_EC: crate::Field = crate::Field::new(1, 5, POWER);
        pub const POWER_UP5K_ON: crate::Field = crate::Field::new(1, 6, POWER);
        pub const POWER_BOOSTMODE: crate::Field = crate::Field::new(1, 7, POWER);
        pub const POWER_SELFDESTRUCT: crate::Field = crate::Field::new(1, 8, POWER);
        pub const POWER_CRYPTO_ON: crate::Field = crate::Field::new(1, 9, POWER);
        pub const POWER_IGNORE_LOCKED: crate::Field = crate::Field::new(1, 10, POWER);
        pub const POWER_DISABLE_WFI: crate::Field = crate::Field::new(1, 11, POWER);

        pub const CLK_STATUS: crate::Register = crate::Register::new(1, 0xf);
        pub const CLK_STATUS_CRYPTO_ON: crate::Field = crate::Field::new(1, 0, CLK_STATUS);
        pub const CLK_STATUS_SHA_ON: crate::Field = crate::Field::new(1, 1, CLK_STATUS);
        pub const CLK_STATUS_ENGINE_ON: crate::Field = crate::Field::new(1, 2, CLK_STATUS);
        pub const CLK_STATUS_BTPOWER_ON: crate::Field = crate::Field::new(1, 3, CLK_STATUS);

        pub const WAKEUP_SOURCE: crate::Register = crate::Register::new(2, 0xff);
        pub const WAKEUP_SOURCE_KBD: crate::Field = crate::Field::new(1, 0, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_TICKTIMER: crate::Field = crate::Field::new(1, 1, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_TIMER0: crate::Field = crate::Field::new(1, 2, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_USB: crate::Field = crate::Field::new(1, 3, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_AUDIO: crate::Field = crate::Field::new(1, 4, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_COM: crate::Field = crate::Field::new(1, 5, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_RTC: crate::Field = crate::Field::new(1, 6, WAKEUP_SOURCE);
        pub const WAKEUP_SOURCE_CONSOLE: crate::Field = crate::Field::new(1, 7, WAKEUP_SOURCE);

        pub const ACTIVITY_RATE: crate::Register = crate::Register::new(3, 0x7fffffff);
        pub const ACTIVITY_RATE_COUNTS_AWAKE: crate::Field = crate::Field::new(31, 0, ACTIVITY_RATE);

        pub const SAMPLING_PERIOD: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const SAMPLING_PERIOD_SAMPLE_PERIOD: crate::Field = crate::Field::new(31, 0, SAMPLING_PERIOD);
        pub const SAMPLING_PERIOD_KILL_SAMPLER: crate::Field = crate::Field::new(1, 31, SAMPLING_PERIOD);

        pub const VIBE: crate::Register = crate::Register::new(5, 0x1);
        pub const VIBE_VIBE: crate::Field = crate::Field::new(1, 0, VIBE);

        pub const EV_STATUS: crate::Register = crate::Register::new(6, 0x3);
        pub const EV_STATUS_USB_ATTACH: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_ACTIVITY_UPDATE: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(7, 0x3);
        pub const EV_PENDING_USB_ATTACH: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_ACTIVITY_UPDATE: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(8, 0x3);
        pub const EV_ENABLE_USB_ATTACH: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_ACTIVITY_UPDATE: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const POWER_IRQ: usize = 10;
        pub const HW_POWER_BASE: usize = 0xf0014000;
    }

    pub mod spinor_soft_int {
        pub const SPINOR_SOFT_INT_NUMREGS: usize = 4;

        pub const EV_STATUS: crate::Register = crate::Register::new(0, 0x1);
        pub const EV_STATUS_SPINOR_INT: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(1, 0x1);
        pub const EV_PENDING_SPINOR_INT: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(2, 0x1);
        pub const EV_ENABLE_SPINOR_INT: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const SOFTINT: crate::Register = crate::Register::new(3, 0x1);
        pub const SOFTINT_SOFTINT: crate::Field = crate::Field::new(1, 0, SOFTINT);

        pub const SPINOR_SOFT_INT_IRQ: usize = 11;
        pub const HW_SPINOR_SOFT_INT_BASE: usize = 0xf0015000;
    }

    pub mod spinor {
        pub const SPINOR_NUMREGS: usize = 13;

        pub const CONFIG: crate::Register = crate::Register::new(0, 0x1f);
        pub const CONFIG_DUMMY: crate::Field = crate::Field::new(5, 0, CONFIG);

        pub const DELAY_CONFIG: crate::Register = crate::Register::new(1, 0x3f);
        pub const DELAY_CONFIG_D: crate::Field = crate::Field::new(5, 0, DELAY_CONFIG);
        pub const DELAY_CONFIG_LOAD: crate::Field = crate::Field::new(1, 5, DELAY_CONFIG);

        pub const DELAY_STATUS: crate::Register = crate::Register::new(2, 0x1f);
        pub const DELAY_STATUS_Q: crate::Field = crate::Field::new(5, 0, DELAY_STATUS);

        pub const COMMAND: crate::Register = crate::Register::new(3, 0x1ffffff);
        pub const COMMAND_WAKEUP: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_EXEC_CMD: crate::Field = crate::Field::new(1, 1, COMMAND);
        pub const COMMAND_CMD_CODE: crate::Field = crate::Field::new(8, 2, COMMAND);
        pub const COMMAND_HAS_ARG: crate::Field = crate::Field::new(1, 10, COMMAND);
        pub const COMMAND_DUMMY_CYCLES: crate::Field = crate::Field::new(5, 11, COMMAND);
        pub const COMMAND_DATA_WORDS: crate::Field = crate::Field::new(8, 16, COMMAND);
        pub const COMMAND_LOCK_READS: crate::Field = crate::Field::new(1, 24, COMMAND);

        pub const CMD_ARG: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const CMD_ARG_CMD_ARG: crate::Field = crate::Field::new(32, 0, CMD_ARG);

        pub const CMD_RBK_DATA: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const CMD_RBK_DATA_CMD_RBK_DATA: crate::Field = crate::Field::new(32, 0, CMD_RBK_DATA);

        pub const STATUS: crate::Register = crate::Register::new(6, 0x1);
        pub const STATUS_WIP: crate::Field = crate::Field::new(1, 0, STATUS);

        pub const WDATA: crate::Register = crate::Register::new(7, 0xffff);
        pub const WDATA_WDATA: crate::Field = crate::Field::new(16, 0, WDATA);

        pub const EV_STATUS: crate::Register = crate::Register::new(8, 0x1);
        pub const EV_STATUS_ECC_ERROR: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(9, 0x1);
        pub const EV_PENDING_ECC_ERROR: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(10, 0x1);
        pub const EV_ENABLE_ECC_ERROR: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const ECC_ADDRESS: crate::Register = crate::Register::new(11, 0xffffffff);
        pub const ECC_ADDRESS_ECC_ADDRESS: crate::Field = crate::Field::new(32, 0, ECC_ADDRESS);

        pub const ECC_STATUS: crate::Register = crate::Register::new(12, 0x3);
        pub const ECC_STATUS_ECC_ERROR: crate::Field = crate::Field::new(1, 0, ECC_STATUS);
        pub const ECC_STATUS_ECC_OVERFLOW: crate::Field = crate::Field::new(1, 1, ECC_STATUS);

        pub const SPINOR_IRQ: usize = 12;
        pub const HW_SPINOR_BASE: usize = 0xf0016000;
    }

    pub mod keyboard {
        pub const KEYBOARD_NUMREGS: usize = 13;

        pub const UART_CHAR: crate::Register = crate::Register::new(0, 0x1ff);
        pub const UART_CHAR_CHAR: crate::Field = crate::Field::new(8, 0, UART_CHAR);
        pub const UART_CHAR_STB: crate::Field = crate::Field::new(1, 8, UART_CHAR);

        pub const ROW0DAT: crate::Register = crate::Register::new(1, 0x3ff);
        pub const ROW0DAT_ROW0DAT: crate::Field = crate::Field::new(10, 0, ROW0DAT);

        pub const ROW1DAT: crate::Register = crate::Register::new(2, 0x3ff);
        pub const ROW1DAT_ROW1DAT: crate::Field = crate::Field::new(10, 0, ROW1DAT);

        pub const ROW2DAT: crate::Register = crate::Register::new(3, 0x3ff);
        pub const ROW2DAT_ROW2DAT: crate::Field = crate::Field::new(10, 0, ROW2DAT);

        pub const ROW3DAT: crate::Register = crate::Register::new(4, 0x3ff);
        pub const ROW3DAT_ROW3DAT: crate::Field = crate::Field::new(10, 0, ROW3DAT);

        pub const ROW4DAT: crate::Register = crate::Register::new(5, 0x3ff);
        pub const ROW4DAT_ROW4DAT: crate::Field = crate::Field::new(10, 0, ROW4DAT);

        pub const ROW5DAT: crate::Register = crate::Register::new(6, 0x3ff);
        pub const ROW5DAT_ROW5DAT: crate::Field = crate::Field::new(10, 0, ROW5DAT);

        pub const ROW6DAT: crate::Register = crate::Register::new(7, 0x3ff);
        pub const ROW6DAT_ROW6DAT: crate::Field = crate::Field::new(10, 0, ROW6DAT);

        pub const ROW7DAT: crate::Register = crate::Register::new(8, 0x3ff);
        pub const ROW7DAT_ROW7DAT: crate::Field = crate::Field::new(10, 0, ROW7DAT);

        pub const ROW8DAT: crate::Register = crate::Register::new(9, 0x3ff);
        pub const ROW8DAT_ROW8DAT: crate::Field = crate::Field::new(10, 0, ROW8DAT);

        pub const EV_STATUS: crate::Register = crate::Register::new(10, 0x3);
        pub const EV_STATUS_KEYPRESSED: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_INJECT: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(11, 0x3);
        pub const EV_PENDING_KEYPRESSED: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_INJECT: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(12, 0x3);
        pub const EV_ENABLE_KEYPRESSED: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_INJECT: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const KEYBOARD_IRQ: usize = 13;
        pub const HW_KEYBOARD_BASE: usize = 0xf0017000;
    }

    pub mod keyinject {
        pub const KEYINJECT_NUMREGS: usize = 2;

        pub const UART_CHAR: crate::Register = crate::Register::new(0, 0xff);
        pub const UART_CHAR_CHAR: crate::Field = crate::Field::new(8, 0, UART_CHAR);

        pub const DISABLE: crate::Register = crate::Register::new(1, 0x1);
        pub const DISABLE_DISABLE: crate::Field = crate::Field::new(1, 0, DISABLE);

        pub const HW_KEYINJECT_BASE: usize = 0xf0018000;
    }

    pub mod seed {
        pub const SEED_NUMREGS: usize = 2;

        pub const SEED1: crate::Register = crate::Register::new(0, 0xffffffff);
        pub const SEED1_SEED: crate::Field = crate::Field::new(32, 0, SEED1);

        pub const SEED0: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const SEED0_SEED: crate::Field = crate::Field::new(32, 0, SEED0);

        pub const HW_SEED_BASE: usize = 0xf0019000;
    }

    pub mod keyrom {
        pub const KEYROM_NUMREGS: usize = 4;

        pub const ADDRESS: crate::Register = crate::Register::new(0, 0xff);
        pub const ADDRESS_ADDRESS: crate::Field = crate::Field::new(8, 0, ADDRESS);

        pub const DATA: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const DATA_DATA: crate::Field = crate::Field::new(32, 0, DATA);

        pub const LOCKADDR: crate::Register = crate::Register::new(2, 0xff);
        pub const LOCKADDR_LOCKADDR: crate::Field = crate::Field::new(8, 0, LOCKADDR);

        pub const LOCKSTAT: crate::Register = crate::Register::new(3, 0x1);
        pub const LOCKSTAT_LOCKSTAT: crate::Field = crate::Field::new(1, 0, LOCKSTAT);

        pub const HW_KEYROM_BASE: usize = 0xf001a000;
    }

    pub mod audio {
        pub const AUDIO_NUMREGS: usize = 9;

        pub const EV_STATUS: crate::Register = crate::Register::new(0, 0xf);
        pub const EV_STATUS_RX_READY: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_RX_ERROR: crate::Field = crate::Field::new(1, 1, EV_STATUS);
        pub const EV_STATUS_TX_READY: crate::Field = crate::Field::new(1, 2, EV_STATUS);
        pub const EV_STATUS_TX_ERROR: crate::Field = crate::Field::new(1, 3, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(1, 0xf);
        pub const EV_PENDING_RX_READY: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_RX_ERROR: crate::Field = crate::Field::new(1, 1, EV_PENDING);
        pub const EV_PENDING_TX_READY: crate::Field = crate::Field::new(1, 2, EV_PENDING);
        pub const EV_PENDING_TX_ERROR: crate::Field = crate::Field::new(1, 3, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(2, 0xf);
        pub const EV_ENABLE_RX_READY: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_RX_ERROR: crate::Field = crate::Field::new(1, 1, EV_ENABLE);
        pub const EV_ENABLE_TX_READY: crate::Field = crate::Field::new(1, 2, EV_ENABLE);
        pub const EV_ENABLE_TX_ERROR: crate::Field = crate::Field::new(1, 3, EV_ENABLE);

        pub const RX_CTL: crate::Register = crate::Register::new(3, 0x3);
        pub const RX_CTL_ENABLE: crate::Field = crate::Field::new(1, 0, RX_CTL);
        pub const RX_CTL_RESET: crate::Field = crate::Field::new(1, 1, RX_CTL);

        pub const RX_STAT: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const RX_STAT_OVERFLOW: crate::Field = crate::Field::new(1, 0, RX_STAT);
        pub const RX_STAT_UNDERFLOW: crate::Field = crate::Field::new(1, 1, RX_STAT);
        pub const RX_STAT_DATAREADY: crate::Field = crate::Field::new(1, 2, RX_STAT);
        pub const RX_STAT_EMPTY: crate::Field = crate::Field::new(1, 3, RX_STAT);
        pub const RX_STAT_WRCOUNT: crate::Field = crate::Field::new(9, 4, RX_STAT);
        pub const RX_STAT_RDCOUNT: crate::Field = crate::Field::new(9, 13, RX_STAT);
        pub const RX_STAT_FIFO_DEPTH: crate::Field = crate::Field::new(9, 22, RX_STAT);
        pub const RX_STAT_CONCATENATE_CHANNELS: crate::Field = crate::Field::new(1, 31, RX_STAT);

        pub const RX_CONF: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const RX_CONF_FORMAT: crate::Field = crate::Field::new(2, 0, RX_CONF);
        pub const RX_CONF_SAMPLE_WIDTH: crate::Field = crate::Field::new(6, 2, RX_CONF);
        pub const RX_CONF_LRCK_FREQ: crate::Field = crate::Field::new(24, 8, RX_CONF);

        pub const TX_CTL: crate::Register = crate::Register::new(6, 0x3);
        pub const TX_CTL_ENABLE: crate::Field = crate::Field::new(1, 0, TX_CTL);
        pub const TX_CTL_RESET: crate::Field = crate::Field::new(1, 1, TX_CTL);

        pub const TX_STAT: crate::Register = crate::Register::new(7, 0x1ffffff);
        pub const TX_STAT_OVERFLOW: crate::Field = crate::Field::new(1, 0, TX_STAT);
        pub const TX_STAT_UNDERFLOW: crate::Field = crate::Field::new(1, 1, TX_STAT);
        pub const TX_STAT_FREE: crate::Field = crate::Field::new(1, 2, TX_STAT);
        pub const TX_STAT_ALMOSTFULL: crate::Field = crate::Field::new(1, 3, TX_STAT);
        pub const TX_STAT_FULL: crate::Field = crate::Field::new(1, 4, TX_STAT);
        pub const TX_STAT_EMPTY: crate::Field = crate::Field::new(1, 5, TX_STAT);
        pub const TX_STAT_WRCOUNT: crate::Field = crate::Field::new(9, 6, TX_STAT);
        pub const TX_STAT_RDCOUNT: crate::Field = crate::Field::new(9, 15, TX_STAT);
        pub const TX_STAT_CONCATENATE_CHANNELS: crate::Field = crate::Field::new(1, 24, TX_STAT);

        pub const TX_CONF: crate::Register = crate::Register::new(8, 0xffffffff);
        pub const TX_CONF_FORMAT: crate::Field = crate::Field::new(2, 0, TX_CONF);
        pub const TX_CONF_SAMPLE_WIDTH: crate::Field = crate::Field::new(6, 2, TX_CONF);
        pub const TX_CONF_LRCK_FREQ: crate::Field = crate::Field::new(24, 8, TX_CONF);

        pub const AUDIO_IRQ: usize = 14;
        pub const HW_AUDIO_BASE: usize = 0xf001b000;
    }

    pub mod trng_kernel {
        pub const TRNG_KERNEL_NUMREGS: usize = 7;

        pub const STATUS: crate::Register = crate::Register::new(0, 0x3fffff);
        pub const STATUS_READY: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_AVAIL: crate::Field = crate::Field::new(1, 1, STATUS);
        pub const STATUS_RDCOUNT: crate::Field = crate::Field::new(10, 2, STATUS);
        pub const STATUS_WRCOUNT: crate::Field = crate::Field::new(10, 12, STATUS);

        pub const DATA: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const DATA_DATA: crate::Field = crate::Field::new(32, 0, DATA);

        pub const URANDOM: crate::Register = crate::Register::new(2, 0xffffffff);
        pub const URANDOM_URANDOM: crate::Field = crate::Field::new(32, 0, URANDOM);

        pub const URANDOM_VALID: crate::Register = crate::Register::new(3, 0x1);
        pub const URANDOM_VALID_URANDOM_VALID: crate::Field = crate::Field::new(1, 0, URANDOM_VALID);

        pub const EV_STATUS: crate::Register = crate::Register::new(4, 0x3);
        pub const EV_STATUS_AVAIL: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_ERROR: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(5, 0x3);
        pub const EV_PENDING_AVAIL: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_ERROR: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(6, 0x3);
        pub const EV_ENABLE_AVAIL: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_ERROR: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const TRNG_KERNEL_IRQ: usize = 15;
        pub const HW_TRNG_KERNEL_BASE: usize = 0xf001c000;
    }

    pub mod trng_server {
        pub const TRNG_SERVER_NUMREGS: usize = 58;

        pub const CONTROL: crate::Register = crate::Register::new(0, 0x1f);
        pub const CONTROL_ENABLE: crate::Field = crate::Field::new(1, 0, CONTROL);
        pub const CONTROL_RO_DIS: crate::Field = crate::Field::new(1, 1, CONTROL);
        pub const CONTROL_AV_DIS: crate::Field = crate::Field::new(1, 2, CONTROL);
        pub const CONTROL_POWERSAVE: crate::Field = crate::Field::new(1, 3, CONTROL);
        pub const CONTROL_CLR_ERR: crate::Field = crate::Field::new(1, 4, CONTROL);

        pub const DATA: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const DATA_DATA: crate::Field = crate::Field::new(32, 0, DATA);

        pub const STATUS: crate::Register = crate::Register::new(2, 0x7fffff);
        pub const STATUS_AVAIL: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_RDCOUNT: crate::Field = crate::Field::new(10, 1, STATUS);
        pub const STATUS_WRCOUNT: crate::Field = crate::Field::new(10, 11, STATUS);
        pub const STATUS_FULL: crate::Field = crate::Field::new(1, 21, STATUS);
        pub const STATUS_CHACHA_READY: crate::Field = crate::Field::new(1, 22, STATUS);

        pub const AV_CONFIG: crate::Register = crate::Register::new(3, 0x3fffffff);
        pub const AV_CONFIG_POWERDELAY: crate::Field = crate::Field::new(20, 0, AV_CONFIG);
        pub const AV_CONFIG_SAMPLES: crate::Field = crate::Field::new(8, 20, AV_CONFIG);
        pub const AV_CONFIG_TEST: crate::Field = crate::Field::new(1, 28, AV_CONFIG);
        pub const AV_CONFIG_REQUIRED: crate::Field = crate::Field::new(1, 29, AV_CONFIG);

        pub const RO_CONFIG: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const RO_CONFIG_GANG: crate::Field = crate::Field::new(1, 0, RO_CONFIG);
        pub const RO_CONFIG_DWELL: crate::Field = crate::Field::new(12, 1, RO_CONFIG);
        pub const RO_CONFIG_DELAY: crate::Field = crate::Field::new(10, 13, RO_CONFIG);
        pub const RO_CONFIG_FUZZ: crate::Field = crate::Field::new(1, 23, RO_CONFIG);
        pub const RO_CONFIG_OVERSAMPLING: crate::Field = crate::Field::new(8, 24, RO_CONFIG);

        pub const AV_NIST: crate::Register = crate::Register::new(5, 0xffff);
        pub const AV_NIST_REPCOUNT_CUTOFF: crate::Field = crate::Field::new(7, 0, AV_NIST);
        pub const AV_NIST_ADAPTIVE_CUTOFF: crate::Field = crate::Field::new(9, 7, AV_NIST);

        pub const RO_NIST: crate::Register = crate::Register::new(6, 0x1ffff);
        pub const RO_NIST_REPCOUNT_CUTOFF: crate::Field = crate::Field::new(7, 0, RO_NIST);
        pub const RO_NIST_ADAPTIVE_CUTOFF: crate::Field = crate::Field::new(10, 7, RO_NIST);

        pub const UNDERRUNS: crate::Register = crate::Register::new(7, 0xfffff);
        pub const UNDERRUNS_SERVER_UNDERRUN: crate::Field = crate::Field::new(10, 0, UNDERRUNS);
        pub const UNDERRUNS_KERNEL_UNDERRUN: crate::Field = crate::Field::new(10, 10, UNDERRUNS);

        pub const NIST_ERRORS: crate::Register = crate::Register::new(8, 0xffff);
        pub const NIST_ERRORS_AV_REPCOUNT: crate::Field = crate::Field::new(2, 0, NIST_ERRORS);
        pub const NIST_ERRORS_AV_ADAPTIVE: crate::Field = crate::Field::new(2, 2, NIST_ERRORS);
        pub const NIST_ERRORS_RO_REPCOUNT: crate::Field = crate::Field::new(4, 4, NIST_ERRORS);
        pub const NIST_ERRORS_RO_ADAPTIVE: crate::Field = crate::Field::new(4, 8, NIST_ERRORS);
        pub const NIST_ERRORS_RO_MINIRUNS: crate::Field = crate::Field::new(4, 12, NIST_ERRORS);

        pub const NIST_RO_STAT0: crate::Register = crate::Register::new(9, 0x3ffff);
        pub const NIST_RO_STAT0_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_RO_STAT0);
        pub const NIST_RO_STAT0_FRESH: crate::Field = crate::Field::new(1, 10, NIST_RO_STAT0);
        pub const NIST_RO_STAT0_REP_B: crate::Field = crate::Field::new(7, 11, NIST_RO_STAT0);

        pub const NIST_RO_STAT1: crate::Register = crate::Register::new(10, 0x3ffff);
        pub const NIST_RO_STAT1_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_RO_STAT1);
        pub const NIST_RO_STAT1_FRESH: crate::Field = crate::Field::new(1, 10, NIST_RO_STAT1);
        pub const NIST_RO_STAT1_REP_B: crate::Field = crate::Field::new(7, 11, NIST_RO_STAT1);

        pub const NIST_RO_STAT2: crate::Register = crate::Register::new(11, 0x3ffff);
        pub const NIST_RO_STAT2_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_RO_STAT2);
        pub const NIST_RO_STAT2_FRESH: crate::Field = crate::Field::new(1, 10, NIST_RO_STAT2);
        pub const NIST_RO_STAT2_REP_B: crate::Field = crate::Field::new(7, 11, NIST_RO_STAT2);

        pub const NIST_RO_STAT3: crate::Register = crate::Register::new(12, 0x3ffff);
        pub const NIST_RO_STAT3_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_RO_STAT3);
        pub const NIST_RO_STAT3_FRESH: crate::Field = crate::Field::new(1, 10, NIST_RO_STAT3);
        pub const NIST_RO_STAT3_REP_B: crate::Field = crate::Field::new(7, 11, NIST_RO_STAT3);

        pub const NIST_AV_STAT0: crate::Register = crate::Register::new(13, 0x3ffff);
        pub const NIST_AV_STAT0_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_AV_STAT0);
        pub const NIST_AV_STAT0_FRESH: crate::Field = crate::Field::new(1, 10, NIST_AV_STAT0);
        pub const NIST_AV_STAT0_REP_B: crate::Field = crate::Field::new(7, 11, NIST_AV_STAT0);

        pub const NIST_AV_STAT1: crate::Register = crate::Register::new(14, 0x3ffff);
        pub const NIST_AV_STAT1_ADAP_B: crate::Field = crate::Field::new(10, 0, NIST_AV_STAT1);
        pub const NIST_AV_STAT1_FRESH: crate::Field = crate::Field::new(1, 10, NIST_AV_STAT1);
        pub const NIST_AV_STAT1_REP_B: crate::Field = crate::Field::new(7, 11, NIST_AV_STAT1);

        pub const RO_RUNSLIMIT1: crate::Register = crate::Register::new(15, 0x3fffff);
        pub const RO_RUNSLIMIT1_MIN: crate::Field = crate::Field::new(11, 0, RO_RUNSLIMIT1);
        pub const RO_RUNSLIMIT1_MAX: crate::Field = crate::Field::new(11, 11, RO_RUNSLIMIT1);

        pub const RO_RUNSLIMIT2: crate::Register = crate::Register::new(16, 0x3fffff);
        pub const RO_RUNSLIMIT2_MIN: crate::Field = crate::Field::new(11, 0, RO_RUNSLIMIT2);
        pub const RO_RUNSLIMIT2_MAX: crate::Field = crate::Field::new(11, 11, RO_RUNSLIMIT2);

        pub const RO_RUNSLIMIT3: crate::Register = crate::Register::new(17, 0x3fffff);
        pub const RO_RUNSLIMIT3_MIN: crate::Field = crate::Field::new(11, 0, RO_RUNSLIMIT3);
        pub const RO_RUNSLIMIT3_MAX: crate::Field = crate::Field::new(11, 11, RO_RUNSLIMIT3);

        pub const RO_RUNSLIMIT4: crate::Register = crate::Register::new(18, 0x3fffff);
        pub const RO_RUNSLIMIT4_MIN: crate::Field = crate::Field::new(11, 0, RO_RUNSLIMIT4);
        pub const RO_RUNSLIMIT4_MAX: crate::Field = crate::Field::new(11, 11, RO_RUNSLIMIT4);

        pub const RO_RUN0_CTRL: crate::Register = crate::Register::new(19, 0x7ff);
        pub const RO_RUN0_CTRL_WINDOW: crate::Field = crate::Field::new(11, 0, RO_RUN0_CTRL);

        pub const RO_RUN0_FRESH: crate::Register = crate::Register::new(20, 0xf);
        pub const RO_RUN0_FRESH_RO_RUN0_FRESH: crate::Field = crate::Field::new(4, 0, RO_RUN0_FRESH);

        pub const RO_RUN0_COUNT1: crate::Register = crate::Register::new(21, 0x7ff);
        pub const RO_RUN0_COUNT1_RO_RUN0_COUNT1: crate::Field = crate::Field::new(11, 0, RO_RUN0_COUNT1);

        pub const RO_RUN0_COUNT2: crate::Register = crate::Register::new(22, 0x7ff);
        pub const RO_RUN0_COUNT2_RO_RUN0_COUNT2: crate::Field = crate::Field::new(11, 0, RO_RUN0_COUNT2);

        pub const RO_RUN0_COUNT3: crate::Register = crate::Register::new(23, 0x7ff);
        pub const RO_RUN0_COUNT3_RO_RUN0_COUNT3: crate::Field = crate::Field::new(11, 0, RO_RUN0_COUNT3);

        pub const RO_RUN0_COUNT4: crate::Register = crate::Register::new(24, 0x7ff);
        pub const RO_RUN0_COUNT4_RO_RUN0_COUNT4: crate::Field = crate::Field::new(11, 0, RO_RUN0_COUNT4);

        pub const RO_RUN1_CTRL: crate::Register = crate::Register::new(25, 0x7ff);
        pub const RO_RUN1_CTRL_WINDOW: crate::Field = crate::Field::new(11, 0, RO_RUN1_CTRL);

        pub const RO_RUN1_FRESH: crate::Register = crate::Register::new(26, 0xf);
        pub const RO_RUN1_FRESH_RO_RUN1_FRESH: crate::Field = crate::Field::new(4, 0, RO_RUN1_FRESH);

        pub const RO_RUN1_COUNT1: crate::Register = crate::Register::new(27, 0x7ff);
        pub const RO_RUN1_COUNT1_RO_RUN1_COUNT1: crate::Field = crate::Field::new(11, 0, RO_RUN1_COUNT1);

        pub const RO_RUN1_COUNT2: crate::Register = crate::Register::new(28, 0x7ff);
        pub const RO_RUN1_COUNT2_RO_RUN1_COUNT2: crate::Field = crate::Field::new(11, 0, RO_RUN1_COUNT2);

        pub const RO_RUN1_COUNT3: crate::Register = crate::Register::new(29, 0x7ff);
        pub const RO_RUN1_COUNT3_RO_RUN1_COUNT3: crate::Field = crate::Field::new(11, 0, RO_RUN1_COUNT3);

        pub const RO_RUN1_COUNT4: crate::Register = crate::Register::new(30, 0x7ff);
        pub const RO_RUN1_COUNT4_RO_RUN1_COUNT4: crate::Field = crate::Field::new(11, 0, RO_RUN1_COUNT4);

        pub const RO_RUN2_CTRL: crate::Register = crate::Register::new(31, 0x7ff);
        pub const RO_RUN2_CTRL_WINDOW: crate::Field = crate::Field::new(11, 0, RO_RUN2_CTRL);

        pub const RO_RUN2_FRESH: crate::Register = crate::Register::new(32, 0xf);
        pub const RO_RUN2_FRESH_RO_RUN2_FRESH: crate::Field = crate::Field::new(4, 0, RO_RUN2_FRESH);

        pub const RO_RUN2_COUNT1: crate::Register = crate::Register::new(33, 0x7ff);
        pub const RO_RUN2_COUNT1_RO_RUN2_COUNT1: crate::Field = crate::Field::new(11, 0, RO_RUN2_COUNT1);

        pub const RO_RUN2_COUNT2: crate::Register = crate::Register::new(34, 0x7ff);
        pub const RO_RUN2_COUNT2_RO_RUN2_COUNT2: crate::Field = crate::Field::new(11, 0, RO_RUN2_COUNT2);

        pub const RO_RUN2_COUNT3: crate::Register = crate::Register::new(35, 0x7ff);
        pub const RO_RUN2_COUNT3_RO_RUN2_COUNT3: crate::Field = crate::Field::new(11, 0, RO_RUN2_COUNT3);

        pub const RO_RUN2_COUNT4: crate::Register = crate::Register::new(36, 0x7ff);
        pub const RO_RUN2_COUNT4_RO_RUN2_COUNT4: crate::Field = crate::Field::new(11, 0, RO_RUN2_COUNT4);

        pub const RO_RUN3_CTRL: crate::Register = crate::Register::new(37, 0x7ff);
        pub const RO_RUN3_CTRL_WINDOW: crate::Field = crate::Field::new(11, 0, RO_RUN3_CTRL);

        pub const RO_RUN3_FRESH: crate::Register = crate::Register::new(38, 0xf);
        pub const RO_RUN3_FRESH_RO_RUN3_FRESH: crate::Field = crate::Field::new(4, 0, RO_RUN3_FRESH);

        pub const RO_RUN3_COUNT1: crate::Register = crate::Register::new(39, 0x7ff);
        pub const RO_RUN3_COUNT1_RO_RUN3_COUNT1: crate::Field = crate::Field::new(11, 0, RO_RUN3_COUNT1);

        pub const RO_RUN3_COUNT2: crate::Register = crate::Register::new(40, 0x7ff);
        pub const RO_RUN3_COUNT2_RO_RUN3_COUNT2: crate::Field = crate::Field::new(11, 0, RO_RUN3_COUNT2);

        pub const RO_RUN3_COUNT3: crate::Register = crate::Register::new(41, 0x7ff);
        pub const RO_RUN3_COUNT3_RO_RUN3_COUNT3: crate::Field = crate::Field::new(11, 0, RO_RUN3_COUNT3);

        pub const RO_RUN3_COUNT4: crate::Register = crate::Register::new(42, 0x7ff);
        pub const RO_RUN3_COUNT4_RO_RUN3_COUNT4: crate::Field = crate::Field::new(11, 0, RO_RUN3_COUNT4);

        pub const AV_EXCURSION0_CTRL: crate::Register = crate::Register::new(43, 0xffffffff);
        pub const AV_EXCURSION0_CTRL_CUTOFF: crate::Field = crate::Field::new(12, 0, AV_EXCURSION0_CTRL);
        pub const AV_EXCURSION0_CTRL_RESET: crate::Field = crate::Field::new(1, 12, AV_EXCURSION0_CTRL);
        pub const AV_EXCURSION0_CTRL_WINDOW: crate::Field = crate::Field::new(19, 13, AV_EXCURSION0_CTRL);

        pub const AV_EXCURSION0_STAT: crate::Register = crate::Register::new(44, 0xffffff);
        pub const AV_EXCURSION0_STAT_MIN: crate::Field = crate::Field::new(12, 0, AV_EXCURSION0_STAT);
        pub const AV_EXCURSION0_STAT_MAX: crate::Field = crate::Field::new(12, 12, AV_EXCURSION0_STAT);

        pub const AV_EXCURSION0_LAST_ERR: crate::Register = crate::Register::new(45, 0xffffff);
        pub const AV_EXCURSION0_LAST_ERR_MIN: crate::Field = crate::Field::new(12, 0, AV_EXCURSION0_LAST_ERR);
        pub const AV_EXCURSION0_LAST_ERR_MAX: crate::Field = crate::Field::new(12, 12, AV_EXCURSION0_LAST_ERR);

        pub const AV_EXCURSION1_CTRL: crate::Register = crate::Register::new(46, 0xffffffff);
        pub const AV_EXCURSION1_CTRL_CUTOFF: crate::Field = crate::Field::new(12, 0, AV_EXCURSION1_CTRL);
        pub const AV_EXCURSION1_CTRL_RESET: crate::Field = crate::Field::new(1, 12, AV_EXCURSION1_CTRL);
        pub const AV_EXCURSION1_CTRL_WINDOW: crate::Field = crate::Field::new(19, 13, AV_EXCURSION1_CTRL);

        pub const AV_EXCURSION1_STAT: crate::Register = crate::Register::new(47, 0xffffff);
        pub const AV_EXCURSION1_STAT_MIN: crate::Field = crate::Field::new(12, 0, AV_EXCURSION1_STAT);
        pub const AV_EXCURSION1_STAT_MAX: crate::Field = crate::Field::new(12, 12, AV_EXCURSION1_STAT);

        pub const AV_EXCURSION1_LAST_ERR: crate::Register = crate::Register::new(48, 0xffffff);
        pub const AV_EXCURSION1_LAST_ERR_MIN: crate::Field = crate::Field::new(12, 0, AV_EXCURSION1_LAST_ERR);
        pub const AV_EXCURSION1_LAST_ERR_MAX: crate::Field = crate::Field::new(12, 12, AV_EXCURSION1_LAST_ERR);

        pub const READY: crate::Register = crate::Register::new(49, 0xff);
        pub const READY_AV_EXCURSION: crate::Field = crate::Field::new(2, 0, READY);
        pub const READY_AV_ADAPROP: crate::Field = crate::Field::new(2, 2, READY);
        pub const READY_RO_ADAPROP: crate::Field = crate::Field::new(4, 4, READY);

        pub const EV_STATUS: crate::Register = crate::Register::new(50, 0x1f);
        pub const EV_STATUS_AVAIL: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_ERROR: crate::Field = crate::Field::new(1, 1, EV_STATUS);
        pub const EV_STATUS_HEALTH: crate::Field = crate::Field::new(1, 2, EV_STATUS);
        pub const EV_STATUS_EXCURSION0: crate::Field = crate::Field::new(1, 3, EV_STATUS);
        pub const EV_STATUS_EXCURSION1: crate::Field = crate::Field::new(1, 4, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(51, 0x1f);
        pub const EV_PENDING_AVAIL: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_ERROR: crate::Field = crate::Field::new(1, 1, EV_PENDING);
        pub const EV_PENDING_HEALTH: crate::Field = crate::Field::new(1, 2, EV_PENDING);
        pub const EV_PENDING_EXCURSION0: crate::Field = crate::Field::new(1, 3, EV_PENDING);
        pub const EV_PENDING_EXCURSION1: crate::Field = crate::Field::new(1, 4, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(52, 0x1f);
        pub const EV_ENABLE_AVAIL: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_ERROR: crate::Field = crate::Field::new(1, 1, EV_ENABLE);
        pub const EV_ENABLE_HEALTH: crate::Field = crate::Field::new(1, 2, EV_ENABLE);
        pub const EV_ENABLE_EXCURSION0: crate::Field = crate::Field::new(1, 3, EV_ENABLE);
        pub const EV_ENABLE_EXCURSION1: crate::Field = crate::Field::new(1, 4, EV_ENABLE);

        pub const CHACHA: crate::Register = crate::Register::new(53, 0x1fffffff);
        pub const CHACHA_RESEED_INTERVAL: crate::Field = crate::Field::new(12, 0, CHACHA);
        pub const CHACHA_SELFMIX_INTERVAL: crate::Field = crate::Field::new(16, 12, CHACHA);
        pub const CHACHA_SELFMIX_ENA: crate::Field = crate::Field::new(1, 28, CHACHA);

        pub const SEED: crate::Register = crate::Register::new(54, 0xffffffff);
        pub const SEED_SEED: crate::Field = crate::Field::new(32, 0, SEED);

        pub const URANDOM: crate::Register = crate::Register::new(55, 0xffffffff);
        pub const URANDOM_URANDOM: crate::Field = crate::Field::new(32, 0, URANDOM);

        pub const URANDOM_VALID: crate::Register = crate::Register::new(56, 0x1);
        pub const URANDOM_VALID_URANDOM_VALID: crate::Field = crate::Field::new(1, 0, URANDOM_VALID);

        pub const TEST: crate::Register = crate::Register::new(57, 0x1);
        pub const TEST_SIMULTANEOUS: crate::Field = crate::Field::new(1, 0, TEST);

        pub const TRNG_SERVER_IRQ: usize = 16;
        pub const HW_TRNG_SERVER_BASE: usize = 0xf001d000;
    }

    pub mod trng {
        pub const TRNG_NUMREGS: usize = 20;

        pub const XADC_TEMPERATURE: crate::Register = crate::Register::new(0, 0xfff);
        pub const XADC_TEMPERATURE_XADC_TEMPERATURE: crate::Field = crate::Field::new(12, 0, XADC_TEMPERATURE);

        pub const XADC_VCCINT: crate::Register = crate::Register::new(1, 0xfff);
        pub const XADC_VCCINT_XADC_VCCINT: crate::Field = crate::Field::new(12, 0, XADC_VCCINT);

        pub const XADC_VCCAUX: crate::Register = crate::Register::new(2, 0xfff);
        pub const XADC_VCCAUX_XADC_VCCAUX: crate::Field = crate::Field::new(12, 0, XADC_VCCAUX);

        pub const XADC_VCCBRAM: crate::Register = crate::Register::new(3, 0xfff);
        pub const XADC_VCCBRAM_XADC_VCCBRAM: crate::Field = crate::Field::new(12, 0, XADC_VCCBRAM);

        pub const XADC_VBUS: crate::Register = crate::Register::new(4, 0xfff);
        pub const XADC_VBUS_XADC_VBUS: crate::Field = crate::Field::new(12, 0, XADC_VBUS);

        pub const XADC_USB_P: crate::Register = crate::Register::new(5, 0xfff);
        pub const XADC_USB_P_XADC_USB_P: crate::Field = crate::Field::new(12, 0, XADC_USB_P);

        pub const XADC_USB_N: crate::Register = crate::Register::new(6, 0xfff);
        pub const XADC_USB_N_XADC_USB_N: crate::Field = crate::Field::new(12, 0, XADC_USB_N);

        pub const XADC_NOISE0: crate::Register = crate::Register::new(7, 0xfff);
        pub const XADC_NOISE0_XADC_NOISE0: crate::Field = crate::Field::new(12, 0, XADC_NOISE0);

        pub const XADC_NOISE1: crate::Register = crate::Register::new(8, 0xfff);
        pub const XADC_NOISE1_XADC_NOISE1: crate::Field = crate::Field::new(12, 0, XADC_NOISE1);

        pub const XADC_EOC: crate::Register = crate::Register::new(9, 0x1);
        pub const XADC_EOC_XADC_EOC: crate::Field = crate::Field::new(1, 0, XADC_EOC);

        pub const XADC_EOS: crate::Register = crate::Register::new(10, 0x1);
        pub const XADC_EOS_XADC_EOS: crate::Field = crate::Field::new(1, 0, XADC_EOS);

        pub const XADC_GPIO5: crate::Register = crate::Register::new(11, 0xfff);
        pub const XADC_GPIO5_XADC_GPIO5: crate::Field = crate::Field::new(12, 0, XADC_GPIO5);

        pub const XADC_GPIO2: crate::Register = crate::Register::new(12, 0xfff);
        pub const XADC_GPIO2_XADC_GPIO2: crate::Field = crate::Field::new(12, 0, XADC_GPIO2);

        pub const XADC_DRP_ENABLE: crate::Register = crate::Register::new(13, 0x1);
        pub const XADC_DRP_ENABLE_XADC_DRP_ENABLE: crate::Field = crate::Field::new(1, 0, XADC_DRP_ENABLE);

        pub const XADC_DRP_READ: crate::Register = crate::Register::new(14, 0x1);
        pub const XADC_DRP_READ_XADC_DRP_READ: crate::Field = crate::Field::new(1, 0, XADC_DRP_READ);

        pub const XADC_DRP_WRITE: crate::Register = crate::Register::new(15, 0x1);
        pub const XADC_DRP_WRITE_XADC_DRP_WRITE: crate::Field = crate::Field::new(1, 0, XADC_DRP_WRITE);

        pub const XADC_DRP_DRDY: crate::Register = crate::Register::new(16, 0x1);
        pub const XADC_DRP_DRDY_XADC_DRP_DRDY: crate::Field = crate::Field::new(1, 0, XADC_DRP_DRDY);

        pub const XADC_DRP_ADR: crate::Register = crate::Register::new(17, 0x7f);
        pub const XADC_DRP_ADR_XADC_DRP_ADR: crate::Field = crate::Field::new(7, 0, XADC_DRP_ADR);

        pub const XADC_DRP_DAT_W: crate::Register = crate::Register::new(18, 0xffff);
        pub const XADC_DRP_DAT_W_XADC_DRP_DAT_W: crate::Field = crate::Field::new(16, 0, XADC_DRP_DAT_W);

        pub const XADC_DRP_DAT_R: crate::Register = crate::Register::new(19, 0xffff);
        pub const XADC_DRP_DAT_R_XADC_DRP_DAT_R: crate::Field = crate::Field::new(16, 0, XADC_DRP_DAT_R);

        pub const HW_TRNG_BASE: usize = 0xf001e000;
    }

    pub mod sha512 {
        pub const SHA512_NUMREGS: usize = 25;

        pub const POWER: crate::Register = crate::Register::new(0, 0x1);
        pub const POWER_ON: crate::Field = crate::Field::new(1, 0, POWER);

        pub const CONFIG: crate::Register = crate::Register::new(1, 0x1f);
        pub const CONFIG_SHA_EN: crate::Field = crate::Field::new(1, 0, CONFIG);
        pub const CONFIG_ENDIAN_SWAP: crate::Field = crate::Field::new(1, 1, CONFIG);
        pub const CONFIG_DIGEST_SWAP: crate::Field = crate::Field::new(1, 2, CONFIG);
        pub const CONFIG_SELECT_256: crate::Field = crate::Field::new(1, 3, CONFIG);
        pub const CONFIG_RESET: crate::Field = crate::Field::new(1, 4, CONFIG);

        pub const COMMAND: crate::Register = crate::Register::new(2, 0x3);
        pub const COMMAND_HASH_START: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_HASH_PROCESS: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const DIGEST01: crate::Register = crate::Register::new(3, 0xffffffff);
        pub const DIGEST01_DIGEST0: crate::Field = crate::Field::new(32, 0, DIGEST01);

        pub const DIGEST00: crate::Register = crate::Register::new(4, 0xffffffff);
        pub const DIGEST00_DIGEST0: crate::Field = crate::Field::new(32, 0, DIGEST00);

        pub const DIGEST11: crate::Register = crate::Register::new(5, 0xffffffff);
        pub const DIGEST11_DIGEST1: crate::Field = crate::Field::new(32, 0, DIGEST11);

        pub const DIGEST10: crate::Register = crate::Register::new(6, 0xffffffff);
        pub const DIGEST10_DIGEST1: crate::Field = crate::Field::new(32, 0, DIGEST10);

        pub const DIGEST21: crate::Register = crate::Register::new(7, 0xffffffff);
        pub const DIGEST21_DIGEST2: crate::Field = crate::Field::new(32, 0, DIGEST21);

        pub const DIGEST20: crate::Register = crate::Register::new(8, 0xffffffff);
        pub const DIGEST20_DIGEST2: crate::Field = crate::Field::new(32, 0, DIGEST20);

        pub const DIGEST31: crate::Register = crate::Register::new(9, 0xffffffff);
        pub const DIGEST31_DIGEST3: crate::Field = crate::Field::new(32, 0, DIGEST31);

        pub const DIGEST30: crate::Register = crate::Register::new(10, 0xffffffff);
        pub const DIGEST30_DIGEST3: crate::Field = crate::Field::new(32, 0, DIGEST30);

        pub const DIGEST41: crate::Register = crate::Register::new(11, 0xffffffff);
        pub const DIGEST41_DIGEST4: crate::Field = crate::Field::new(32, 0, DIGEST41);

        pub const DIGEST40: crate::Register = crate::Register::new(12, 0xffffffff);
        pub const DIGEST40_DIGEST4: crate::Field = crate::Field::new(32, 0, DIGEST40);

        pub const DIGEST51: crate::Register = crate::Register::new(13, 0xffffffff);
        pub const DIGEST51_DIGEST5: crate::Field = crate::Field::new(32, 0, DIGEST51);

        pub const DIGEST50: crate::Register = crate::Register::new(14, 0xffffffff);
        pub const DIGEST50_DIGEST5: crate::Field = crate::Field::new(32, 0, DIGEST50);

        pub const DIGEST61: crate::Register = crate::Register::new(15, 0xffffffff);
        pub const DIGEST61_DIGEST6: crate::Field = crate::Field::new(32, 0, DIGEST61);

        pub const DIGEST60: crate::Register = crate::Register::new(16, 0xffffffff);
        pub const DIGEST60_DIGEST6: crate::Field = crate::Field::new(32, 0, DIGEST60);

        pub const DIGEST71: crate::Register = crate::Register::new(17, 0xffffffff);
        pub const DIGEST71_DIGEST7: crate::Field = crate::Field::new(32, 0, DIGEST71);

        pub const DIGEST70: crate::Register = crate::Register::new(18, 0xffffffff);
        pub const DIGEST70_DIGEST7: crate::Field = crate::Field::new(32, 0, DIGEST70);

        pub const MSG_LENGTH1: crate::Register = crate::Register::new(19, 0xffffffff);
        pub const MSG_LENGTH1_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH1);

        pub const MSG_LENGTH0: crate::Register = crate::Register::new(20, 0xffffffff);
        pub const MSG_LENGTH0_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH0);

        pub const EV_STATUS: crate::Register = crate::Register::new(21, 0x7);
        pub const EV_STATUS_ERR_VALID: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_FIFO_FULL: crate::Field = crate::Field::new(1, 1, EV_STATUS);
        pub const EV_STATUS_SHA512_DONE: crate::Field = crate::Field::new(1, 2, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(22, 0x7);
        pub const EV_PENDING_ERR_VALID: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_FIFO_FULL: crate::Field = crate::Field::new(1, 1, EV_PENDING);
        pub const EV_PENDING_SHA512_DONE: crate::Field = crate::Field::new(1, 2, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(23, 0x7);
        pub const EV_ENABLE_ERR_VALID: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_FIFO_FULL: crate::Field = crate::Field::new(1, 1, EV_ENABLE);
        pub const EV_ENABLE_SHA512_DONE: crate::Field = crate::Field::new(1, 2, EV_ENABLE);

        pub const FIFO: crate::Register = crate::Register::new(24, 0xffffff);
        pub const FIFO_RESET_STATUS: crate::Field = crate::Field::new(1, 0, FIFO);
        pub const FIFO_READ_COUNT: crate::Field = crate::Field::new(9, 1, FIFO);
        pub const FIFO_WRITE_COUNT: crate::Field = crate::Field::new(9, 10, FIFO);
        pub const FIFO_READ_ERROR: crate::Field = crate::Field::new(1, 19, FIFO);
        pub const FIFO_WRITE_ERROR: crate::Field = crate::Field::new(1, 20, FIFO);
        pub const FIFO_ALMOST_FULL: crate::Field = crate::Field::new(1, 21, FIFO);
        pub const FIFO_ALMOST_EMPTY: crate::Field = crate::Field::new(1, 22, FIFO);
        pub const FIFO_RUNNING: crate::Field = crate::Field::new(1, 23, FIFO);

        pub const SHA512_IRQ: usize = 17;
        pub const HW_SHA512_BASE: usize = 0xf001f000;
    }

    pub mod engine {
        pub const ENGINE_NUMREGS: usize = 11;

        pub const WINDOW: crate::Register = crate::Register::new(0, 0xf);
        pub const WINDOW_WINDOW: crate::Field = crate::Field::new(4, 0, WINDOW);

        pub const MPSTART: crate::Register = crate::Register::new(1, 0x3ff);
        pub const MPSTART_MPSTART: crate::Field = crate::Field::new(10, 0, MPSTART);

        pub const MPLEN: crate::Register = crate::Register::new(2, 0x3ff);
        pub const MPLEN_MPLEN: crate::Field = crate::Field::new(10, 0, MPLEN);

        pub const CONTROL: crate::Register = crate::Register::new(3, 0x1);
        pub const CONTROL_GO: crate::Field = crate::Field::new(1, 0, CONTROL);

        pub const MPRESUME: crate::Register = crate::Register::new(4, 0x3ff);
        pub const MPRESUME_MPRESUME: crate::Field = crate::Field::new(10, 0, MPRESUME);

        pub const POWER: crate::Register = crate::Register::new(5, 0x3);
        pub const POWER_ON: crate::Field = crate::Field::new(1, 0, POWER);
        pub const POWER_PAUSE_REQ: crate::Field = crate::Field::new(1, 1, POWER);

        pub const STATUS: crate::Register = crate::Register::new(6, 0xfff);
        pub const STATUS_RUNNING: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_MPC: crate::Field = crate::Field::new(10, 1, STATUS);
        pub const STATUS_PAUSE_GNT: crate::Field = crate::Field::new(1, 11, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(7, 0x3);
        pub const EV_STATUS_FINISHED: crate::Field = crate::Field::new(1, 0, EV_STATUS);
        pub const EV_STATUS_ILLEGAL_OPCODE: crate::Field = crate::Field::new(1, 1, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(8, 0x3);
        pub const EV_PENDING_FINISHED: crate::Field = crate::Field::new(1, 0, EV_PENDING);
        pub const EV_PENDING_ILLEGAL_OPCODE: crate::Field = crate::Field::new(1, 1, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(9, 0x3);
        pub const EV_ENABLE_FINISHED: crate::Field = crate::Field::new(1, 0, EV_ENABLE);
        pub const EV_ENABLE_ILLEGAL_OPCODE: crate::Field = crate::Field::new(1, 1, EV_ENABLE);

        pub const INSTRUCTION: crate::Register = crate::Register::new(10, 0xffffffff);
        pub const INSTRUCTION_OPCODE: crate::Field = crate::Field::new(6, 0, INSTRUCTION);
        pub const INSTRUCTION_RA: crate::Field = crate::Field::new(5, 6, INSTRUCTION);
        pub const INSTRUCTION_CA: crate::Field = crate::Field::new(1, 11, INSTRUCTION);
        pub const INSTRUCTION_RB: crate::Field = crate::Field::new(5, 12, INSTRUCTION);
        pub const INSTRUCTION_CB: crate::Field = crate::Field::new(1, 17, INSTRUCTION);
        pub const INSTRUCTION_WD: crate::Field = crate::Field::new(5, 18, INSTRUCTION);
        pub const INSTRUCTION_IMMEDIATE: crate::Field = crate::Field::new(9, 23, INSTRUCTION);

        pub const ENGINE_IRQ: usize = 18;
        pub const HW_ENGINE_BASE: usize = 0xf0020000;
    }

    pub mod jtag {
        pub const JTAG_NUMREGS: usize = 2;

        pub const NEXT: crate::Register = crate::Register::new(0, 0x3);
        pub const NEXT_TDI: crate::Field = crate::Field::new(1, 0, NEXT);
        pub const NEXT_TMS: crate::Field = crate::Field::new(1, 1, NEXT);

        pub const TDO: crate::Register = crate::Register::new(1, 0x3);
        pub const TDO_TDO: crate::Field = crate::Field::new(1, 0, TDO);
        pub const TDO_READY: crate::Field = crate::Field::new(1, 1, TDO);

        pub const HW_JTAG_BASE: usize = 0xf0021000;
    }

    pub mod wdt {
        pub const WDT_NUMREGS: usize = 3;

        pub const WATCHDOG: crate::Register = crate::Register::new(0, 0x3);
        pub const WATCHDOG_RESET_WDT: crate::Field = crate::Field::new(1, 0, WATCHDOG);
        pub const WATCHDOG_ENABLE: crate::Field = crate::Field::new(1, 1, WATCHDOG);

        pub const PERIOD: crate::Register = crate::Register::new(1, 0xffffffff);
        pub const PERIOD_PERIOD: crate::Field = crate::Field::new(32, 0, PERIOD);

        pub const STATE: crate::Register = crate::Register::new(2, 0xf);
        pub const STATE_ENABLED: crate::Field = crate::Field::new(1, 0, STATE);
        pub const STATE_ARMED1: crate::Field = crate::Field::new(1, 1, STATE);
        pub const STATE_ARMED2: crate::Field = crate::Field::new(1, 2, STATE);
        pub const STATE_DISARMED: crate::Field = crate::Field::new(1, 3, STATE);

        pub const HW_WDT_BASE: usize = 0xf0022000;
    }

    pub mod usbdev {
        pub const USBDEV_NUMREGS: usize = 5;

        pub const USBDISABLE: crate::Register = crate::Register::new(0, 0x1);
        pub const USBDISABLE_USBDISABLE: crate::Field = crate::Field::new(1, 0, USBDISABLE);

        pub const USBSELECT: crate::Register = crate::Register::new(1, 0x3);
        pub const USBSELECT_SELECT_DEVICE: crate::Field = crate::Field::new(1, 0, USBSELECT);
        pub const USBSELECT_FORCE_RESET: crate::Field = crate::Field::new(1, 1, USBSELECT);

        pub const EV_STATUS: crate::Register = crate::Register::new(2, 0x1);
        pub const EV_STATUS_USB: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(3, 0x1);
        pub const EV_PENDING_USB: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(4, 0x1);
        pub const EV_ENABLE_USB: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const USBDEV_IRQ: usize = 19;
        pub const HW_USBDEV_BASE: usize = 0xf0023000;
    }

    pub mod d11ctime {
        pub const D11CTIME_NUMREGS: usize = 2;

        pub const CONTROL: crate::Register = crate::Register::new(0, 0x7fff);
        pub const CONTROL_COUNT: crate::Field = crate::Field::new(15, 0, CONTROL);

        pub const HEARTBEAT: crate::Register = crate::Register::new(1, 0x1);
        pub const HEARTBEAT_BEAT: crate::Field = crate::Field::new(1, 0, HEARTBEAT);

        pub const HW_D11CTIME_BASE: usize = 0xf0024000;
    }

    pub mod wfi {
        pub const WFI_NUMREGS: usize = 2;

        pub const WFI: crate::Register = crate::Register::new(0, 0x1);
        pub const WFI_WFI: crate::Field = crate::Field::new(1, 0, WFI);

        pub const IGNORE_LOCKED: crate::Register = crate::Register::new(1, 0x1);
        pub const IGNORE_LOCKED_IGNORE_LOCKED: crate::Field = crate::Field::new(1, 0, IGNORE_LOCKED);

        pub const HW_WFI_BASE: usize = 0xf0025000;
    }

    pub mod identifier_mem {
        pub const IDENTIFIER_MEM_NUMREGS: usize = 1;

        pub const IDENTIFIER_MEM: crate::Register = crate::Register::new(0, 0xff);
        pub const IDENTIFIER_MEM_IDENTIFIER_MEM: crate::Field = crate::Field::new(8, 0, IDENTIFIER_MEM);

        pub const HW_IDENTIFIER_MEM_BASE: usize = 0xf0026000;
    }
}

// Litex auto-generated constants
pub const LITEX_CONFIG_CLOCK_FREQUENCY: usize = 100000000;
pub const LITEX_CONFIG_CPU_HAS_INTERRUPT: &str = "None";
pub const LITEX_CONFIG_CPU_RESET_ADDR: usize = 2147483648;
pub const LITEX_CONFIG_CPU_HAS_DCACHE: &str = "None";
pub const LITEX_CONFIG_CPU_HAS_ICACHE: &str = "None";
pub const LITEX_CONFIG_CPU_TYPE_VEXRISCV: &str = "None";
pub const LITEX_CONFIG_CPU_VARIANT_STANDARD: &str = "None";
pub const LITEX_CONFIG_CPU_HUMAN_NAME: &str = "VexRiscv";
pub const LITEX_CONFIG_CPU_NOP: &str = "nop";
pub const LITEX_CONFIG_CSR_DATA_WIDTH: usize = 32;
pub const LITEX_CONFIG_CSR_ALIGNMENT: usize = 32;
pub const LITEX_CONFIG_BUS_STANDARD: &str = "WISHBONE";
pub const LITEX_CONFIG_BUS_DATA_WIDTH: usize = 32;
pub const LITEX_CONFIG_BUS_ADDRESS_WIDTH: usize = 32;
pub const LITEX_APP_UART_INTERRUPT: usize = 4;
pub const LITEX_AUDIO_INTERRUPT: usize = 14;
pub const LITEX_BTEVENTS_INTERRUPT: usize = 7;
pub const LITEX_COM_INTERRUPT: usize = 5;
pub const LITEX_CONSOLE_INTERRUPT: usize = 3;
pub const LITEX_ENGINE_INTERRUPT: usize = 18;
pub const LITEX_GPIO_INTERRUPT: usize = 1;
pub const LITEX_I2C_INTERRUPT: usize = 6;
pub const LITEX_KEYBOARD_INTERRUPT: usize = 13;
pub const LITEX_POWER_INTERRUPT: usize = 10;
pub const LITEX_SHA512_INTERRUPT: usize = 17;
pub const LITEX_SPINOR_INTERRUPT: usize = 12;
pub const LITEX_SPINOR_SOFT_INT_INTERRUPT: usize = 11;
pub const LITEX_SUSRES_INTERRUPT: usize = 9;
pub const LITEX_TICKTIMER_INTERRUPT: usize = 8;
pub const LITEX_TIMER0_INTERRUPT: usize = 0;
pub const LITEX_TRNG_KERNEL_INTERRUPT: usize = 15;
pub const LITEX_TRNG_SERVER_INTERRUPT: usize = 16;
pub const LITEX_UART_INTERRUPT: usize = 2;
pub const LITEX_USBDEV_INTERRUPT: usize = 19;


#[cfg(test)]
mod tests {

    #[test]
    #[ignore]
    fn compile_check_reboot_csr() {
        use super::*;
        let mut reboot_csr = CSR::new(HW_REBOOT_BASE as *mut u32);

        let foo = reboot_csr.r(utra::reboot::SOC_RESET);
        reboot_csr.wo(utra::reboot::SOC_RESET, foo);
        let bar = reboot_csr.rf(utra::reboot::SOC_RESET_SOC_RESET);
        reboot_csr.rmwf(utra::reboot::SOC_RESET_SOC_RESET, bar);
        let mut baz = reboot_csr.zf(utra::reboot::SOC_RESET_SOC_RESET, bar);
        baz |= reboot_csr.ms(utra::reboot::SOC_RESET_SOC_RESET, 1);
        reboot_csr.wfo(utra::reboot::SOC_RESET_SOC_RESET, baz);

        let foo = reboot_csr.r(utra::reboot::ADDR);
        reboot_csr.wo(utra::reboot::ADDR, foo);
        let bar = reboot_csr.rf(utra::reboot::ADDR_ADDR);
        reboot_csr.rmwf(utra::reboot::ADDR_ADDR, bar);
        let mut baz = reboot_csr.zf(utra::reboot::ADDR_ADDR, bar);
        baz |= reboot_csr.ms(utra::reboot::ADDR_ADDR, 1);
        reboot_csr.wfo(utra::reboot::ADDR_ADDR, baz);

        let foo = reboot_csr.r(utra::reboot::CPU_RESET);
        reboot_csr.wo(utra::reboot::CPU_RESET, foo);
        let bar = reboot_csr.rf(utra::reboot::CPU_RESET_CPU_RESET);
        reboot_csr.rmwf(utra::reboot::CPU_RESET_CPU_RESET, bar);
        let mut baz = reboot_csr.zf(utra::reboot::CPU_RESET_CPU_RESET, bar);
        baz |= reboot_csr.ms(utra::reboot::CPU_RESET_CPU_RESET, 1);
        reboot_csr.wfo(utra::reboot::CPU_RESET_CPU_RESET, baz);
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
    fn compile_check_crg_csr() {
        use super::*;
        let mut crg_csr = CSR::new(HW_CRG_BASE as *mut u32);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_RESET);
        crg_csr.wo(utra::crg::MMCM_DRP_RESET, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_RESET_MMCM_DRP_RESET);
        crg_csr.rmwf(utra::crg::MMCM_DRP_RESET_MMCM_DRP_RESET, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_RESET_MMCM_DRP_RESET, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_RESET_MMCM_DRP_RESET, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_RESET_MMCM_DRP_RESET, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_LOCKED);
        crg_csr.wo(utra::crg::MMCM_DRP_LOCKED, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_LOCKED_MMCM_DRP_LOCKED);
        crg_csr.rmwf(utra::crg::MMCM_DRP_LOCKED_MMCM_DRP_LOCKED, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_LOCKED_MMCM_DRP_LOCKED, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_LOCKED_MMCM_DRP_LOCKED, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_LOCKED_MMCM_DRP_LOCKED, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_READ);
        crg_csr.wo(utra::crg::MMCM_DRP_READ, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_READ_MMCM_DRP_READ);
        crg_csr.rmwf(utra::crg::MMCM_DRP_READ_MMCM_DRP_READ, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_READ_MMCM_DRP_READ, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_READ_MMCM_DRP_READ, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_READ_MMCM_DRP_READ, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_WRITE);
        crg_csr.wo(utra::crg::MMCM_DRP_WRITE, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_WRITE_MMCM_DRP_WRITE);
        crg_csr.rmwf(utra::crg::MMCM_DRP_WRITE_MMCM_DRP_WRITE, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_WRITE_MMCM_DRP_WRITE, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_WRITE_MMCM_DRP_WRITE, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_WRITE_MMCM_DRP_WRITE, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_DRDY);
        crg_csr.wo(utra::crg::MMCM_DRP_DRDY, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_DRDY_MMCM_DRP_DRDY);
        crg_csr.rmwf(utra::crg::MMCM_DRP_DRDY_MMCM_DRP_DRDY, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_DRDY_MMCM_DRP_DRDY, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_DRDY_MMCM_DRP_DRDY, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_DRDY_MMCM_DRP_DRDY, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_ADR);
        crg_csr.wo(utra::crg::MMCM_DRP_ADR, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_ADR_MMCM_DRP_ADR);
        crg_csr.rmwf(utra::crg::MMCM_DRP_ADR_MMCM_DRP_ADR, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_ADR_MMCM_DRP_ADR, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_ADR_MMCM_DRP_ADR, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_ADR_MMCM_DRP_ADR, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_DAT_W);
        crg_csr.wo(utra::crg::MMCM_DRP_DAT_W, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_DAT_W_MMCM_DRP_DAT_W);
        crg_csr.rmwf(utra::crg::MMCM_DRP_DAT_W_MMCM_DRP_DAT_W, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_DAT_W_MMCM_DRP_DAT_W, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_DAT_W_MMCM_DRP_DAT_W, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_DAT_W_MMCM_DRP_DAT_W, baz);

        let foo = crg_csr.r(utra::crg::MMCM_DRP_DAT_R);
        crg_csr.wo(utra::crg::MMCM_DRP_DAT_R, foo);
        let bar = crg_csr.rf(utra::crg::MMCM_DRP_DAT_R_MMCM_DRP_DAT_R);
        crg_csr.rmwf(utra::crg::MMCM_DRP_DAT_R_MMCM_DRP_DAT_R, bar);
        let mut baz = crg_csr.zf(utra::crg::MMCM_DRP_DAT_R_MMCM_DRP_DAT_R, bar);
        baz |= crg_csr.ms(utra::crg::MMCM_DRP_DAT_R_MMCM_DRP_DAT_R, 1);
        crg_csr.wfo(utra::crg::MMCM_DRP_DAT_R_MMCM_DRP_DAT_R, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_gpio_csr() {
        use super::*;
        let mut gpio_csr = CSR::new(HW_GPIO_BASE as *mut u32);

        let foo = gpio_csr.r(utra::gpio::OUTPUT);
        gpio_csr.wo(utra::gpio::OUTPUT, foo);
        let bar = gpio_csr.rf(utra::gpio::OUTPUT_OUTPUT);
        gpio_csr.rmwf(utra::gpio::OUTPUT_OUTPUT, bar);
        let mut baz = gpio_csr.zf(utra::gpio::OUTPUT_OUTPUT, bar);
        baz |= gpio_csr.ms(utra::gpio::OUTPUT_OUTPUT, 1);
        gpio_csr.wfo(utra::gpio::OUTPUT_OUTPUT, baz);

        let foo = gpio_csr.r(utra::gpio::INPUT);
        gpio_csr.wo(utra::gpio::INPUT, foo);
        let bar = gpio_csr.rf(utra::gpio::INPUT_INPUT);
        gpio_csr.rmwf(utra::gpio::INPUT_INPUT, bar);
        let mut baz = gpio_csr.zf(utra::gpio::INPUT_INPUT, bar);
        baz |= gpio_csr.ms(utra::gpio::INPUT_INPUT, 1);
        gpio_csr.wfo(utra::gpio::INPUT_INPUT, baz);

        let foo = gpio_csr.r(utra::gpio::DRIVE);
        gpio_csr.wo(utra::gpio::DRIVE, foo);
        let bar = gpio_csr.rf(utra::gpio::DRIVE_DRIVE);
        gpio_csr.rmwf(utra::gpio::DRIVE_DRIVE, bar);
        let mut baz = gpio_csr.zf(utra::gpio::DRIVE_DRIVE, bar);
        baz |= gpio_csr.ms(utra::gpio::DRIVE_DRIVE, 1);
        gpio_csr.wfo(utra::gpio::DRIVE_DRIVE, baz);

        let foo = gpio_csr.r(utra::gpio::INTENA);
        gpio_csr.wo(utra::gpio::INTENA, foo);
        let bar = gpio_csr.rf(utra::gpio::INTENA_INTENA);
        gpio_csr.rmwf(utra::gpio::INTENA_INTENA, bar);
        let mut baz = gpio_csr.zf(utra::gpio::INTENA_INTENA, bar);
        baz |= gpio_csr.ms(utra::gpio::INTENA_INTENA, 1);
        gpio_csr.wfo(utra::gpio::INTENA_INTENA, baz);

        let foo = gpio_csr.r(utra::gpio::INTPOL);
        gpio_csr.wo(utra::gpio::INTPOL, foo);
        let bar = gpio_csr.rf(utra::gpio::INTPOL_INTPOL);
        gpio_csr.rmwf(utra::gpio::INTPOL_INTPOL, bar);
        let mut baz = gpio_csr.zf(utra::gpio::INTPOL_INTPOL, bar);
        baz |= gpio_csr.ms(utra::gpio::INTPOL_INTPOL, 1);
        gpio_csr.wfo(utra::gpio::INTPOL_INTPOL, baz);

        let foo = gpio_csr.r(utra::gpio::UARTSEL);
        gpio_csr.wo(utra::gpio::UARTSEL, foo);
        let bar = gpio_csr.rf(utra::gpio::UARTSEL_UARTSEL);
        gpio_csr.rmwf(utra::gpio::UARTSEL_UARTSEL, bar);
        let mut baz = gpio_csr.zf(utra::gpio::UARTSEL_UARTSEL, bar);
        baz |= gpio_csr.ms(utra::gpio::UARTSEL_UARTSEL, 1);
        gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, baz);

        let foo = gpio_csr.r(utra::gpio::DEBUG);
        gpio_csr.wo(utra::gpio::DEBUG, foo);
        let bar = gpio_csr.rf(utra::gpio::DEBUG_WFI);
        gpio_csr.rmwf(utra::gpio::DEBUG_WFI, bar);
        let mut baz = gpio_csr.zf(utra::gpio::DEBUG_WFI, bar);
        baz |= gpio_csr.ms(utra::gpio::DEBUG_WFI, 1);
        gpio_csr.wfo(utra::gpio::DEBUG_WFI, baz);
        let bar = gpio_csr.rf(utra::gpio::DEBUG_WAKEUP);
        gpio_csr.rmwf(utra::gpio::DEBUG_WAKEUP, bar);
        let mut baz = gpio_csr.zf(utra::gpio::DEBUG_WAKEUP, bar);
        baz |= gpio_csr.ms(utra::gpio::DEBUG_WAKEUP, 1);
        gpio_csr.wfo(utra::gpio::DEBUG_WAKEUP, baz);

        let foo = gpio_csr.r(utra::gpio::EV_STATUS);
        gpio_csr.wo(utra::gpio::EV_STATUS, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT0);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT0, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT0, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT0, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT0, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT1);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT1, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT1, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT1, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT1, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT2);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT2, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT2, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT2, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT2, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT3);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT3, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT3, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT3, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT3, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT4);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT4, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT4, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT4, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT4, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT5);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT5, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT5, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT5, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT5, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT6);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT6, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT6, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT6, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT6, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_EVENT7);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_EVENT7, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_EVENT7, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_EVENT7, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_EVENT7, baz);

        let foo = gpio_csr.r(utra::gpio::EV_PENDING);
        gpio_csr.wo(utra::gpio::EV_PENDING, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT0);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT0, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT0, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT0, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT0, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT1);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT1, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT1, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT1, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT1, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT2);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT2, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT2, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT2, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT2, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT3);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT3, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT3, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT3, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT3, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT4);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT4, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT4, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT4, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT4, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT5);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT5, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT5, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT5, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT5, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT6);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT6, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT6, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT6, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT6, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_EVENT7);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_EVENT7, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_EVENT7, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_EVENT7, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_EVENT7, baz);

        let foo = gpio_csr.r(utra::gpio::EV_ENABLE);
        gpio_csr.wo(utra::gpio::EV_ENABLE, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT0);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT0, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT0, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT0, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT0, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT1);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT1, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT1, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT1, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT1, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT2);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT2, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT2, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT2, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT2, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT3);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT3, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT3, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT3, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT3, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT4);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT4, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT4, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT4, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT4, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT5);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT5, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT5, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT5, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT5, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT6);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT6, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT6, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT6, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT6, baz);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_EVENT7);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_EVENT7, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_EVENT7, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_EVENT7, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_EVENT7, baz);
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
    fn compile_check_console_csr() {
        use super::*;
        let mut console_csr = CSR::new(HW_CONSOLE_BASE as *mut u32);

        let foo = console_csr.r(utra::console::RXTX);
        console_csr.wo(utra::console::RXTX, foo);
        let bar = console_csr.rf(utra::console::RXTX_RXTX);
        console_csr.rmwf(utra::console::RXTX_RXTX, bar);
        let mut baz = console_csr.zf(utra::console::RXTX_RXTX, bar);
        baz |= console_csr.ms(utra::console::RXTX_RXTX, 1);
        console_csr.wfo(utra::console::RXTX_RXTX, baz);

        let foo = console_csr.r(utra::console::TXFULL);
        console_csr.wo(utra::console::TXFULL, foo);
        let bar = console_csr.rf(utra::console::TXFULL_TXFULL);
        console_csr.rmwf(utra::console::TXFULL_TXFULL, bar);
        let mut baz = console_csr.zf(utra::console::TXFULL_TXFULL, bar);
        baz |= console_csr.ms(utra::console::TXFULL_TXFULL, 1);
        console_csr.wfo(utra::console::TXFULL_TXFULL, baz);

        let foo = console_csr.r(utra::console::RXEMPTY);
        console_csr.wo(utra::console::RXEMPTY, foo);
        let bar = console_csr.rf(utra::console::RXEMPTY_RXEMPTY);
        console_csr.rmwf(utra::console::RXEMPTY_RXEMPTY, bar);
        let mut baz = console_csr.zf(utra::console::RXEMPTY_RXEMPTY, bar);
        baz |= console_csr.ms(utra::console::RXEMPTY_RXEMPTY, 1);
        console_csr.wfo(utra::console::RXEMPTY_RXEMPTY, baz);

        let foo = console_csr.r(utra::console::EV_STATUS);
        console_csr.wo(utra::console::EV_STATUS, foo);
        let bar = console_csr.rf(utra::console::EV_STATUS_TX);
        console_csr.rmwf(utra::console::EV_STATUS_TX, bar);
        let mut baz = console_csr.zf(utra::console::EV_STATUS_TX, bar);
        baz |= console_csr.ms(utra::console::EV_STATUS_TX, 1);
        console_csr.wfo(utra::console::EV_STATUS_TX, baz);
        let bar = console_csr.rf(utra::console::EV_STATUS_RX);
        console_csr.rmwf(utra::console::EV_STATUS_RX, bar);
        let mut baz = console_csr.zf(utra::console::EV_STATUS_RX, bar);
        baz |= console_csr.ms(utra::console::EV_STATUS_RX, 1);
        console_csr.wfo(utra::console::EV_STATUS_RX, baz);

        let foo = console_csr.r(utra::console::EV_PENDING);
        console_csr.wo(utra::console::EV_PENDING, foo);
        let bar = console_csr.rf(utra::console::EV_PENDING_TX);
        console_csr.rmwf(utra::console::EV_PENDING_TX, bar);
        let mut baz = console_csr.zf(utra::console::EV_PENDING_TX, bar);
        baz |= console_csr.ms(utra::console::EV_PENDING_TX, 1);
        console_csr.wfo(utra::console::EV_PENDING_TX, baz);
        let bar = console_csr.rf(utra::console::EV_PENDING_RX);
        console_csr.rmwf(utra::console::EV_PENDING_RX, bar);
        let mut baz = console_csr.zf(utra::console::EV_PENDING_RX, bar);
        baz |= console_csr.ms(utra::console::EV_PENDING_RX, 1);
        console_csr.wfo(utra::console::EV_PENDING_RX, baz);

        let foo = console_csr.r(utra::console::EV_ENABLE);
        console_csr.wo(utra::console::EV_ENABLE, foo);
        let bar = console_csr.rf(utra::console::EV_ENABLE_TX);
        console_csr.rmwf(utra::console::EV_ENABLE_TX, bar);
        let mut baz = console_csr.zf(utra::console::EV_ENABLE_TX, bar);
        baz |= console_csr.ms(utra::console::EV_ENABLE_TX, 1);
        console_csr.wfo(utra::console::EV_ENABLE_TX, baz);
        let bar = console_csr.rf(utra::console::EV_ENABLE_RX);
        console_csr.rmwf(utra::console::EV_ENABLE_RX, bar);
        let mut baz = console_csr.zf(utra::console::EV_ENABLE_RX, bar);
        baz |= console_csr.ms(utra::console::EV_ENABLE_RX, 1);
        console_csr.wfo(utra::console::EV_ENABLE_RX, baz);

        let foo = console_csr.r(utra::console::TXEMPTY);
        console_csr.wo(utra::console::TXEMPTY, foo);
        let bar = console_csr.rf(utra::console::TXEMPTY_TXEMPTY);
        console_csr.rmwf(utra::console::TXEMPTY_TXEMPTY, bar);
        let mut baz = console_csr.zf(utra::console::TXEMPTY_TXEMPTY, bar);
        baz |= console_csr.ms(utra::console::TXEMPTY_TXEMPTY, 1);
        console_csr.wfo(utra::console::TXEMPTY_TXEMPTY, baz);

        let foo = console_csr.r(utra::console::RXFULL);
        console_csr.wo(utra::console::RXFULL, foo);
        let bar = console_csr.rf(utra::console::RXFULL_RXFULL);
        console_csr.rmwf(utra::console::RXFULL_RXFULL, bar);
        let mut baz = console_csr.zf(utra::console::RXFULL_RXFULL, bar);
        baz |= console_csr.ms(utra::console::RXFULL_RXFULL, 1);
        console_csr.wfo(utra::console::RXFULL_RXFULL, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_app_uart_csr() {
        use super::*;
        let mut app_uart_csr = CSR::new(HW_APP_UART_BASE as *mut u32);

        let foo = app_uart_csr.r(utra::app_uart::RXTX);
        app_uart_csr.wo(utra::app_uart::RXTX, foo);
        let bar = app_uart_csr.rf(utra::app_uart::RXTX_RXTX);
        app_uart_csr.rmwf(utra::app_uart::RXTX_RXTX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::RXTX_RXTX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::RXTX_RXTX, 1);
        app_uart_csr.wfo(utra::app_uart::RXTX_RXTX, baz);

        let foo = app_uart_csr.r(utra::app_uart::TXFULL);
        app_uart_csr.wo(utra::app_uart::TXFULL, foo);
        let bar = app_uart_csr.rf(utra::app_uart::TXFULL_TXFULL);
        app_uart_csr.rmwf(utra::app_uart::TXFULL_TXFULL, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::TXFULL_TXFULL, bar);
        baz |= app_uart_csr.ms(utra::app_uart::TXFULL_TXFULL, 1);
        app_uart_csr.wfo(utra::app_uart::TXFULL_TXFULL, baz);

        let foo = app_uart_csr.r(utra::app_uart::RXEMPTY);
        app_uart_csr.wo(utra::app_uart::RXEMPTY, foo);
        let bar = app_uart_csr.rf(utra::app_uart::RXEMPTY_RXEMPTY);
        app_uart_csr.rmwf(utra::app_uart::RXEMPTY_RXEMPTY, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::RXEMPTY_RXEMPTY, bar);
        baz |= app_uart_csr.ms(utra::app_uart::RXEMPTY_RXEMPTY, 1);
        app_uart_csr.wfo(utra::app_uart::RXEMPTY_RXEMPTY, baz);

        let foo = app_uart_csr.r(utra::app_uart::EV_STATUS);
        app_uart_csr.wo(utra::app_uart::EV_STATUS, foo);
        let bar = app_uart_csr.rf(utra::app_uart::EV_STATUS_TX);
        app_uart_csr.rmwf(utra::app_uart::EV_STATUS_TX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_STATUS_TX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_STATUS_TX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_STATUS_TX, baz);
        let bar = app_uart_csr.rf(utra::app_uart::EV_STATUS_RX);
        app_uart_csr.rmwf(utra::app_uart::EV_STATUS_RX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_STATUS_RX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_STATUS_RX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_STATUS_RX, baz);

        let foo = app_uart_csr.r(utra::app_uart::EV_PENDING);
        app_uart_csr.wo(utra::app_uart::EV_PENDING, foo);
        let bar = app_uart_csr.rf(utra::app_uart::EV_PENDING_TX);
        app_uart_csr.rmwf(utra::app_uart::EV_PENDING_TX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_PENDING_TX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_PENDING_TX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_PENDING_TX, baz);
        let bar = app_uart_csr.rf(utra::app_uart::EV_PENDING_RX);
        app_uart_csr.rmwf(utra::app_uart::EV_PENDING_RX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_PENDING_RX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_PENDING_RX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_PENDING_RX, baz);

        let foo = app_uart_csr.r(utra::app_uart::EV_ENABLE);
        app_uart_csr.wo(utra::app_uart::EV_ENABLE, foo);
        let bar = app_uart_csr.rf(utra::app_uart::EV_ENABLE_TX);
        app_uart_csr.rmwf(utra::app_uart::EV_ENABLE_TX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_ENABLE_TX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_ENABLE_TX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_ENABLE_TX, baz);
        let bar = app_uart_csr.rf(utra::app_uart::EV_ENABLE_RX);
        app_uart_csr.rmwf(utra::app_uart::EV_ENABLE_RX, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::EV_ENABLE_RX, bar);
        baz |= app_uart_csr.ms(utra::app_uart::EV_ENABLE_RX, 1);
        app_uart_csr.wfo(utra::app_uart::EV_ENABLE_RX, baz);

        let foo = app_uart_csr.r(utra::app_uart::TXEMPTY);
        app_uart_csr.wo(utra::app_uart::TXEMPTY, foo);
        let bar = app_uart_csr.rf(utra::app_uart::TXEMPTY_TXEMPTY);
        app_uart_csr.rmwf(utra::app_uart::TXEMPTY_TXEMPTY, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::TXEMPTY_TXEMPTY, bar);
        baz |= app_uart_csr.ms(utra::app_uart::TXEMPTY_TXEMPTY, 1);
        app_uart_csr.wfo(utra::app_uart::TXEMPTY_TXEMPTY, baz);

        let foo = app_uart_csr.r(utra::app_uart::RXFULL);
        app_uart_csr.wo(utra::app_uart::RXFULL, foo);
        let bar = app_uart_csr.rf(utra::app_uart::RXFULL_RXFULL);
        app_uart_csr.rmwf(utra::app_uart::RXFULL_RXFULL, bar);
        let mut baz = app_uart_csr.zf(utra::app_uart::RXFULL_RXFULL, bar);
        baz |= app_uart_csr.ms(utra::app_uart::RXFULL_RXFULL, 1);
        app_uart_csr.wfo(utra::app_uart::RXFULL_RXFULL, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_info_csr() {
        use super::*;
        let mut info_csr = CSR::new(HW_INFO_BASE as *mut u32);

        let foo = info_csr.r(utra::info::DNA_ID1);
        info_csr.wo(utra::info::DNA_ID1, foo);
        let bar = info_csr.rf(utra::info::DNA_ID1_DNA_ID);
        info_csr.rmwf(utra::info::DNA_ID1_DNA_ID, bar);
        let mut baz = info_csr.zf(utra::info::DNA_ID1_DNA_ID, bar);
        baz |= info_csr.ms(utra::info::DNA_ID1_DNA_ID, 1);
        info_csr.wfo(utra::info::DNA_ID1_DNA_ID, baz);

        let foo = info_csr.r(utra::info::DNA_ID0);
        info_csr.wo(utra::info::DNA_ID0, foo);
        let bar = info_csr.rf(utra::info::DNA_ID0_DNA_ID);
        info_csr.rmwf(utra::info::DNA_ID0_DNA_ID, bar);
        let mut baz = info_csr.zf(utra::info::DNA_ID0_DNA_ID, bar);
        baz |= info_csr.ms(utra::info::DNA_ID0_DNA_ID, 1);
        info_csr.wfo(utra::info::DNA_ID0_DNA_ID, baz);

        let foo = info_csr.r(utra::info::GIT_MAJOR);
        info_csr.wo(utra::info::GIT_MAJOR, foo);
        let bar = info_csr.rf(utra::info::GIT_MAJOR_GIT_MAJOR);
        info_csr.rmwf(utra::info::GIT_MAJOR_GIT_MAJOR, bar);
        let mut baz = info_csr.zf(utra::info::GIT_MAJOR_GIT_MAJOR, bar);
        baz |= info_csr.ms(utra::info::GIT_MAJOR_GIT_MAJOR, 1);
        info_csr.wfo(utra::info::GIT_MAJOR_GIT_MAJOR, baz);

        let foo = info_csr.r(utra::info::GIT_MINOR);
        info_csr.wo(utra::info::GIT_MINOR, foo);
        let bar = info_csr.rf(utra::info::GIT_MINOR_GIT_MINOR);
        info_csr.rmwf(utra::info::GIT_MINOR_GIT_MINOR, bar);
        let mut baz = info_csr.zf(utra::info::GIT_MINOR_GIT_MINOR, bar);
        baz |= info_csr.ms(utra::info::GIT_MINOR_GIT_MINOR, 1);
        info_csr.wfo(utra::info::GIT_MINOR_GIT_MINOR, baz);

        let foo = info_csr.r(utra::info::GIT_REVISION);
        info_csr.wo(utra::info::GIT_REVISION, foo);
        let bar = info_csr.rf(utra::info::GIT_REVISION_GIT_REVISION);
        info_csr.rmwf(utra::info::GIT_REVISION_GIT_REVISION, bar);
        let mut baz = info_csr.zf(utra::info::GIT_REVISION_GIT_REVISION, bar);
        baz |= info_csr.ms(utra::info::GIT_REVISION_GIT_REVISION, 1);
        info_csr.wfo(utra::info::GIT_REVISION_GIT_REVISION, baz);

        let foo = info_csr.r(utra::info::GIT_GITREV);
        info_csr.wo(utra::info::GIT_GITREV, foo);
        let bar = info_csr.rf(utra::info::GIT_GITREV_GIT_GITREV);
        info_csr.rmwf(utra::info::GIT_GITREV_GIT_GITREV, bar);
        let mut baz = info_csr.zf(utra::info::GIT_GITREV_GIT_GITREV, bar);
        baz |= info_csr.ms(utra::info::GIT_GITREV_GIT_GITREV, 1);
        info_csr.wfo(utra::info::GIT_GITREV_GIT_GITREV, baz);

        let foo = info_csr.r(utra::info::GIT_GITEXTRA);
        info_csr.wo(utra::info::GIT_GITEXTRA, foo);
        let bar = info_csr.rf(utra::info::GIT_GITEXTRA_GIT_GITEXTRA);
        info_csr.rmwf(utra::info::GIT_GITEXTRA_GIT_GITEXTRA, bar);
        let mut baz = info_csr.zf(utra::info::GIT_GITEXTRA_GIT_GITEXTRA, bar);
        baz |= info_csr.ms(utra::info::GIT_GITEXTRA_GIT_GITEXTRA, 1);
        info_csr.wfo(utra::info::GIT_GITEXTRA_GIT_GITEXTRA, baz);

        let foo = info_csr.r(utra::info::GIT_DIRTY);
        info_csr.wo(utra::info::GIT_DIRTY, foo);
        let bar = info_csr.rf(utra::info::GIT_DIRTY_DIRTY);
        info_csr.rmwf(utra::info::GIT_DIRTY_DIRTY, bar);
        let mut baz = info_csr.zf(utra::info::GIT_DIRTY_DIRTY, bar);
        baz |= info_csr.ms(utra::info::GIT_DIRTY_DIRTY, 1);
        info_csr.wfo(utra::info::GIT_DIRTY_DIRTY, baz);

        let foo = info_csr.r(utra::info::PLATFORM_PLATFORM1);
        info_csr.wo(utra::info::PLATFORM_PLATFORM1, foo);
        let bar = info_csr.rf(utra::info::PLATFORM_PLATFORM1_PLATFORM_PLATFORM);
        info_csr.rmwf(utra::info::PLATFORM_PLATFORM1_PLATFORM_PLATFORM, bar);
        let mut baz = info_csr.zf(utra::info::PLATFORM_PLATFORM1_PLATFORM_PLATFORM, bar);
        baz |= info_csr.ms(utra::info::PLATFORM_PLATFORM1_PLATFORM_PLATFORM, 1);
        info_csr.wfo(utra::info::PLATFORM_PLATFORM1_PLATFORM_PLATFORM, baz);

        let foo = info_csr.r(utra::info::PLATFORM_PLATFORM0);
        info_csr.wo(utra::info::PLATFORM_PLATFORM0, foo);
        let bar = info_csr.rf(utra::info::PLATFORM_PLATFORM0_PLATFORM_PLATFORM);
        info_csr.rmwf(utra::info::PLATFORM_PLATFORM0_PLATFORM_PLATFORM, bar);
        let mut baz = info_csr.zf(utra::info::PLATFORM_PLATFORM0_PLATFORM_PLATFORM, bar);
        baz |= info_csr.ms(utra::info::PLATFORM_PLATFORM0_PLATFORM_PLATFORM, 1);
        info_csr.wfo(utra::info::PLATFORM_PLATFORM0_PLATFORM_PLATFORM, baz);

        let foo = info_csr.r(utra::info::PLATFORM_TARGET1);
        info_csr.wo(utra::info::PLATFORM_TARGET1, foo);
        let bar = info_csr.rf(utra::info::PLATFORM_TARGET1_PLATFORM_TARGET);
        info_csr.rmwf(utra::info::PLATFORM_TARGET1_PLATFORM_TARGET, bar);
        let mut baz = info_csr.zf(utra::info::PLATFORM_TARGET1_PLATFORM_TARGET, bar);
        baz |= info_csr.ms(utra::info::PLATFORM_TARGET1_PLATFORM_TARGET, 1);
        info_csr.wfo(utra::info::PLATFORM_TARGET1_PLATFORM_TARGET, baz);

        let foo = info_csr.r(utra::info::PLATFORM_TARGET0);
        info_csr.wo(utra::info::PLATFORM_TARGET0, foo);
        let bar = info_csr.rf(utra::info::PLATFORM_TARGET0_PLATFORM_TARGET);
        info_csr.rmwf(utra::info::PLATFORM_TARGET0_PLATFORM_TARGET, bar);
        let mut baz = info_csr.zf(utra::info::PLATFORM_TARGET0_PLATFORM_TARGET, bar);
        baz |= info_csr.ms(utra::info::PLATFORM_TARGET0_PLATFORM_TARGET, 1);
        info_csr.wfo(utra::info::PLATFORM_TARGET0_PLATFORM_TARGET, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_sram_ext_csr() {
        use super::*;
        let mut sram_ext_csr = CSR::new(HW_SRAM_EXT_BASE as *mut u32);

        let foo = sram_ext_csr.r(utra::sram_ext::CONFIG_STATUS);
        sram_ext_csr.wo(utra::sram_ext::CONFIG_STATUS, foo);
        let bar = sram_ext_csr.rf(utra::sram_ext::CONFIG_STATUS_MODE);
        sram_ext_csr.rmwf(utra::sram_ext::CONFIG_STATUS_MODE, bar);
        let mut baz = sram_ext_csr.zf(utra::sram_ext::CONFIG_STATUS_MODE, bar);
        baz |= sram_ext_csr.ms(utra::sram_ext::CONFIG_STATUS_MODE, 1);
        sram_ext_csr.wfo(utra::sram_ext::CONFIG_STATUS_MODE, baz);

        let foo = sram_ext_csr.r(utra::sram_ext::READ_CONFIG);
        sram_ext_csr.wo(utra::sram_ext::READ_CONFIG, foo);
        let bar = sram_ext_csr.rf(utra::sram_ext::READ_CONFIG_TRIGGER);
        sram_ext_csr.rmwf(utra::sram_ext::READ_CONFIG_TRIGGER, bar);
        let mut baz = sram_ext_csr.zf(utra::sram_ext::READ_CONFIG_TRIGGER, bar);
        baz |= sram_ext_csr.ms(utra::sram_ext::READ_CONFIG_TRIGGER, 1);
        sram_ext_csr.wfo(utra::sram_ext::READ_CONFIG_TRIGGER, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_memlcd_csr() {
        use super::*;
        let mut memlcd_csr = CSR::new(HW_MEMLCD_BASE as *mut u32);

        let foo = memlcd_csr.r(utra::memlcd::COMMAND);
        memlcd_csr.wo(utra::memlcd::COMMAND, foo);
        let bar = memlcd_csr.rf(utra::memlcd::COMMAND_UPDATEDIRTY);
        memlcd_csr.rmwf(utra::memlcd::COMMAND_UPDATEDIRTY, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::COMMAND_UPDATEDIRTY, bar);
        baz |= memlcd_csr.ms(utra::memlcd::COMMAND_UPDATEDIRTY, 1);
        memlcd_csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, baz);
        let bar = memlcd_csr.rf(utra::memlcd::COMMAND_UPDATEALL);
        memlcd_csr.rmwf(utra::memlcd::COMMAND_UPDATEALL, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::COMMAND_UPDATEALL, bar);
        baz |= memlcd_csr.ms(utra::memlcd::COMMAND_UPDATEALL, 1);
        memlcd_csr.wfo(utra::memlcd::COMMAND_UPDATEALL, baz);

        let foo = memlcd_csr.r(utra::memlcd::BUSY);
        memlcd_csr.wo(utra::memlcd::BUSY, foo);
        let bar = memlcd_csr.rf(utra::memlcd::BUSY_BUSY);
        memlcd_csr.rmwf(utra::memlcd::BUSY_BUSY, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::BUSY_BUSY, bar);
        baz |= memlcd_csr.ms(utra::memlcd::BUSY_BUSY, 1);
        memlcd_csr.wfo(utra::memlcd::BUSY_BUSY, baz);

        let foo = memlcd_csr.r(utra::memlcd::PRESCALER);
        memlcd_csr.wo(utra::memlcd::PRESCALER, foo);
        let bar = memlcd_csr.rf(utra::memlcd::PRESCALER_PRESCALER);
        memlcd_csr.rmwf(utra::memlcd::PRESCALER_PRESCALER, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::PRESCALER_PRESCALER, bar);
        baz |= memlcd_csr.ms(utra::memlcd::PRESCALER_PRESCALER, 1);
        memlcd_csr.wfo(utra::memlcd::PRESCALER_PRESCALER, baz);

        let foo = memlcd_csr.r(utra::memlcd::EV_STATUS);
        memlcd_csr.wo(utra::memlcd::EV_STATUS, foo);
        let bar = memlcd_csr.rf(utra::memlcd::EV_STATUS_DONE);
        memlcd_csr.rmwf(utra::memlcd::EV_STATUS_DONE, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_STATUS_DONE, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_STATUS_DONE, 1);
        memlcd_csr.wfo(utra::memlcd::EV_STATUS_DONE, baz);

        let foo = memlcd_csr.r(utra::memlcd::EV_PENDING);
        memlcd_csr.wo(utra::memlcd::EV_PENDING, foo);
        let bar = memlcd_csr.rf(utra::memlcd::EV_PENDING_DONE);
        memlcd_csr.rmwf(utra::memlcd::EV_PENDING_DONE, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_PENDING_DONE, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_PENDING_DONE, 1);
        memlcd_csr.wfo(utra::memlcd::EV_PENDING_DONE, baz);

        let foo = memlcd_csr.r(utra::memlcd::EV_ENABLE);
        memlcd_csr.wo(utra::memlcd::EV_ENABLE, foo);
        let bar = memlcd_csr.rf(utra::memlcd::EV_ENABLE_DONE);
        memlcd_csr.rmwf(utra::memlcd::EV_ENABLE_DONE, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_ENABLE_DONE, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_ENABLE_DONE, 1);
        memlcd_csr.wfo(utra::memlcd::EV_ENABLE_DONE, baz);

        let foo = memlcd_csr.r(utra::memlcd::DEVBOOT);
        memlcd_csr.wo(utra::memlcd::DEVBOOT, foo);
        let bar = memlcd_csr.rf(utra::memlcd::DEVBOOT_DEVBOOT);
        memlcd_csr.rmwf(utra::memlcd::DEVBOOT_DEVBOOT, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::DEVBOOT_DEVBOOT, bar);
        baz |= memlcd_csr.ms(utra::memlcd::DEVBOOT_DEVBOOT, 1);
        memlcd_csr.wfo(utra::memlcd::DEVBOOT_DEVBOOT, baz);

        let foo = memlcd_csr.r(utra::memlcd::DEVSTATUS);
        memlcd_csr.wo(utra::memlcd::DEVSTATUS, foo);
        let bar = memlcd_csr.rf(utra::memlcd::DEVSTATUS_DEVSTATUS);
        memlcd_csr.rmwf(utra::memlcd::DEVSTATUS_DEVSTATUS, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::DEVSTATUS_DEVSTATUS, bar);
        baz |= memlcd_csr.ms(utra::memlcd::DEVSTATUS_DEVSTATUS, 1);
        memlcd_csr.wfo(utra::memlcd::DEVSTATUS_DEVSTATUS, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_com_csr() {
        use super::*;
        let mut com_csr = CSR::new(HW_COM_BASE as *mut u32);

        let foo = com_csr.r(utra::com::TX);
        com_csr.wo(utra::com::TX, foo);
        let bar = com_csr.rf(utra::com::TX_TX);
        com_csr.rmwf(utra::com::TX_TX, bar);
        let mut baz = com_csr.zf(utra::com::TX_TX, bar);
        baz |= com_csr.ms(utra::com::TX_TX, 1);
        com_csr.wfo(utra::com::TX_TX, baz);

        let foo = com_csr.r(utra::com::RX);
        com_csr.wo(utra::com::RX, foo);
        let bar = com_csr.rf(utra::com::RX_RX);
        com_csr.rmwf(utra::com::RX_RX, bar);
        let mut baz = com_csr.zf(utra::com::RX_RX, bar);
        baz |= com_csr.ms(utra::com::RX_RX, 1);
        com_csr.wfo(utra::com::RX_RX, baz);

        let foo = com_csr.r(utra::com::CONTROL);
        com_csr.wo(utra::com::CONTROL, foo);
        let bar = com_csr.rf(utra::com::CONTROL_INTENA);
        com_csr.rmwf(utra::com::CONTROL_INTENA, bar);
        let mut baz = com_csr.zf(utra::com::CONTROL_INTENA, bar);
        baz |= com_csr.ms(utra::com::CONTROL_INTENA, 1);
        com_csr.wfo(utra::com::CONTROL_INTENA, baz);
        let bar = com_csr.rf(utra::com::CONTROL_AUTOHOLD);
        com_csr.rmwf(utra::com::CONTROL_AUTOHOLD, bar);
        let mut baz = com_csr.zf(utra::com::CONTROL_AUTOHOLD, bar);
        baz |= com_csr.ms(utra::com::CONTROL_AUTOHOLD, 1);
        com_csr.wfo(utra::com::CONTROL_AUTOHOLD, baz);

        let foo = com_csr.r(utra::com::STATUS);
        com_csr.wo(utra::com::STATUS, foo);
        let bar = com_csr.rf(utra::com::STATUS_TIP);
        com_csr.rmwf(utra::com::STATUS_TIP, bar);
        let mut baz = com_csr.zf(utra::com::STATUS_TIP, bar);
        baz |= com_csr.ms(utra::com::STATUS_TIP, 1);
        com_csr.wfo(utra::com::STATUS_TIP, baz);
        let bar = com_csr.rf(utra::com::STATUS_HOLD);
        com_csr.rmwf(utra::com::STATUS_HOLD, bar);
        let mut baz = com_csr.zf(utra::com::STATUS_HOLD, bar);
        baz |= com_csr.ms(utra::com::STATUS_HOLD, 1);
        com_csr.wfo(utra::com::STATUS_HOLD, baz);

        let foo = com_csr.r(utra::com::EV_STATUS);
        com_csr.wo(utra::com::EV_STATUS, foo);
        let bar = com_csr.rf(utra::com::EV_STATUS_SPI_INT);
        com_csr.rmwf(utra::com::EV_STATUS_SPI_INT, bar);
        let mut baz = com_csr.zf(utra::com::EV_STATUS_SPI_INT, bar);
        baz |= com_csr.ms(utra::com::EV_STATUS_SPI_INT, 1);
        com_csr.wfo(utra::com::EV_STATUS_SPI_INT, baz);
        let bar = com_csr.rf(utra::com::EV_STATUS_SPI_HOLD);
        com_csr.rmwf(utra::com::EV_STATUS_SPI_HOLD, bar);
        let mut baz = com_csr.zf(utra::com::EV_STATUS_SPI_HOLD, bar);
        baz |= com_csr.ms(utra::com::EV_STATUS_SPI_HOLD, 1);
        com_csr.wfo(utra::com::EV_STATUS_SPI_HOLD, baz);

        let foo = com_csr.r(utra::com::EV_PENDING);
        com_csr.wo(utra::com::EV_PENDING, foo);
        let bar = com_csr.rf(utra::com::EV_PENDING_SPI_INT);
        com_csr.rmwf(utra::com::EV_PENDING_SPI_INT, bar);
        let mut baz = com_csr.zf(utra::com::EV_PENDING_SPI_INT, bar);
        baz |= com_csr.ms(utra::com::EV_PENDING_SPI_INT, 1);
        com_csr.wfo(utra::com::EV_PENDING_SPI_INT, baz);
        let bar = com_csr.rf(utra::com::EV_PENDING_SPI_HOLD);
        com_csr.rmwf(utra::com::EV_PENDING_SPI_HOLD, bar);
        let mut baz = com_csr.zf(utra::com::EV_PENDING_SPI_HOLD, bar);
        baz |= com_csr.ms(utra::com::EV_PENDING_SPI_HOLD, 1);
        com_csr.wfo(utra::com::EV_PENDING_SPI_HOLD, baz);

        let foo = com_csr.r(utra::com::EV_ENABLE);
        com_csr.wo(utra::com::EV_ENABLE, foo);
        let bar = com_csr.rf(utra::com::EV_ENABLE_SPI_INT);
        com_csr.rmwf(utra::com::EV_ENABLE_SPI_INT, bar);
        let mut baz = com_csr.zf(utra::com::EV_ENABLE_SPI_INT, bar);
        baz |= com_csr.ms(utra::com::EV_ENABLE_SPI_INT, 1);
        com_csr.wfo(utra::com::EV_ENABLE_SPI_INT, baz);
        let bar = com_csr.rf(utra::com::EV_ENABLE_SPI_HOLD);
        com_csr.rmwf(utra::com::EV_ENABLE_SPI_HOLD, bar);
        let mut baz = com_csr.zf(utra::com::EV_ENABLE_SPI_HOLD, bar);
        baz |= com_csr.ms(utra::com::EV_ENABLE_SPI_HOLD, 1);
        com_csr.wfo(utra::com::EV_ENABLE_SPI_HOLD, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_i2c_csr() {
        use super::*;
        let mut i2c_csr = CSR::new(HW_I2C_BASE as *mut u32);

        let foo = i2c_csr.r(utra::i2c::PRESCALE);
        i2c_csr.wo(utra::i2c::PRESCALE, foo);
        let bar = i2c_csr.rf(utra::i2c::PRESCALE_PRESCALE);
        i2c_csr.rmwf(utra::i2c::PRESCALE_PRESCALE, bar);
        let mut baz = i2c_csr.zf(utra::i2c::PRESCALE_PRESCALE, bar);
        baz |= i2c_csr.ms(utra::i2c::PRESCALE_PRESCALE, 1);
        i2c_csr.wfo(utra::i2c::PRESCALE_PRESCALE, baz);

        let foo = i2c_csr.r(utra::i2c::CONTROL);
        i2c_csr.wo(utra::i2c::CONTROL, foo);
        let bar = i2c_csr.rf(utra::i2c::CONTROL_RESVD);
        i2c_csr.rmwf(utra::i2c::CONTROL_RESVD, bar);
        let mut baz = i2c_csr.zf(utra::i2c::CONTROL_RESVD, bar);
        baz |= i2c_csr.ms(utra::i2c::CONTROL_RESVD, 1);
        i2c_csr.wfo(utra::i2c::CONTROL_RESVD, baz);
        let bar = i2c_csr.rf(utra::i2c::CONTROL_IEN);
        i2c_csr.rmwf(utra::i2c::CONTROL_IEN, bar);
        let mut baz = i2c_csr.zf(utra::i2c::CONTROL_IEN, bar);
        baz |= i2c_csr.ms(utra::i2c::CONTROL_IEN, 1);
        i2c_csr.wfo(utra::i2c::CONTROL_IEN, baz);
        let bar = i2c_csr.rf(utra::i2c::CONTROL_EN);
        i2c_csr.rmwf(utra::i2c::CONTROL_EN, bar);
        let mut baz = i2c_csr.zf(utra::i2c::CONTROL_EN, bar);
        baz |= i2c_csr.ms(utra::i2c::CONTROL_EN, 1);
        i2c_csr.wfo(utra::i2c::CONTROL_EN, baz);

        let foo = i2c_csr.r(utra::i2c::TXR);
        i2c_csr.wo(utra::i2c::TXR, foo);
        let bar = i2c_csr.rf(utra::i2c::TXR_TXR);
        i2c_csr.rmwf(utra::i2c::TXR_TXR, bar);
        let mut baz = i2c_csr.zf(utra::i2c::TXR_TXR, bar);
        baz |= i2c_csr.ms(utra::i2c::TXR_TXR, 1);
        i2c_csr.wfo(utra::i2c::TXR_TXR, baz);

        let foo = i2c_csr.r(utra::i2c::RXR);
        i2c_csr.wo(utra::i2c::RXR, foo);
        let bar = i2c_csr.rf(utra::i2c::RXR_RXR);
        i2c_csr.rmwf(utra::i2c::RXR_RXR, bar);
        let mut baz = i2c_csr.zf(utra::i2c::RXR_RXR, bar);
        baz |= i2c_csr.ms(utra::i2c::RXR_RXR, 1);
        i2c_csr.wfo(utra::i2c::RXR_RXR, baz);

        let foo = i2c_csr.r(utra::i2c::COMMAND);
        i2c_csr.wo(utra::i2c::COMMAND, foo);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_IACK);
        i2c_csr.rmwf(utra::i2c::COMMAND_IACK, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_IACK, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_IACK, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_IACK, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_RESVD);
        i2c_csr.rmwf(utra::i2c::COMMAND_RESVD, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_RESVD, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_RESVD, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_RESVD, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_ACK);
        i2c_csr.rmwf(utra::i2c::COMMAND_ACK, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_ACK, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_ACK, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_ACK, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_WR);
        i2c_csr.rmwf(utra::i2c::COMMAND_WR, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_WR, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_WR, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_WR, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_RD);
        i2c_csr.rmwf(utra::i2c::COMMAND_RD, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_RD, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_RD, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_RD, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_STO);
        i2c_csr.rmwf(utra::i2c::COMMAND_STO, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_STO, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_STO, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_STO, baz);
        let bar = i2c_csr.rf(utra::i2c::COMMAND_STA);
        i2c_csr.rmwf(utra::i2c::COMMAND_STA, bar);
        let mut baz = i2c_csr.zf(utra::i2c::COMMAND_STA, bar);
        baz |= i2c_csr.ms(utra::i2c::COMMAND_STA, 1);
        i2c_csr.wfo(utra::i2c::COMMAND_STA, baz);

        let foo = i2c_csr.r(utra::i2c::STATUS);
        i2c_csr.wo(utra::i2c::STATUS, foo);
        let bar = i2c_csr.rf(utra::i2c::STATUS_IF);
        i2c_csr.rmwf(utra::i2c::STATUS_IF, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_IF, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_IF, 1);
        i2c_csr.wfo(utra::i2c::STATUS_IF, baz);
        let bar = i2c_csr.rf(utra::i2c::STATUS_TIP);
        i2c_csr.rmwf(utra::i2c::STATUS_TIP, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_TIP, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_TIP, 1);
        i2c_csr.wfo(utra::i2c::STATUS_TIP, baz);
        let bar = i2c_csr.rf(utra::i2c::STATUS_RESVD);
        i2c_csr.rmwf(utra::i2c::STATUS_RESVD, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_RESVD, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_RESVD, 1);
        i2c_csr.wfo(utra::i2c::STATUS_RESVD, baz);
        let bar = i2c_csr.rf(utra::i2c::STATUS_ARBLOST);
        i2c_csr.rmwf(utra::i2c::STATUS_ARBLOST, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_ARBLOST, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_ARBLOST, 1);
        i2c_csr.wfo(utra::i2c::STATUS_ARBLOST, baz);
        let bar = i2c_csr.rf(utra::i2c::STATUS_BUSY);
        i2c_csr.rmwf(utra::i2c::STATUS_BUSY, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_BUSY, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_BUSY, 1);
        i2c_csr.wfo(utra::i2c::STATUS_BUSY, baz);
        let bar = i2c_csr.rf(utra::i2c::STATUS_RXACK);
        i2c_csr.rmwf(utra::i2c::STATUS_RXACK, bar);
        let mut baz = i2c_csr.zf(utra::i2c::STATUS_RXACK, bar);
        baz |= i2c_csr.ms(utra::i2c::STATUS_RXACK, 1);
        i2c_csr.wfo(utra::i2c::STATUS_RXACK, baz);

        let foo = i2c_csr.r(utra::i2c::CORE_RESET);
        i2c_csr.wo(utra::i2c::CORE_RESET, foo);
        let bar = i2c_csr.rf(utra::i2c::CORE_RESET_RESET);
        i2c_csr.rmwf(utra::i2c::CORE_RESET_RESET, bar);
        let mut baz = i2c_csr.zf(utra::i2c::CORE_RESET_RESET, bar);
        baz |= i2c_csr.ms(utra::i2c::CORE_RESET_RESET, 1);
        i2c_csr.wfo(utra::i2c::CORE_RESET_RESET, baz);

        let foo = i2c_csr.r(utra::i2c::EV_STATUS);
        i2c_csr.wo(utra::i2c::EV_STATUS, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_STATUS_I2C_INT);
        i2c_csr.rmwf(utra::i2c::EV_STATUS_I2C_INT, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_STATUS_I2C_INT, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_STATUS_I2C_INT, 1);
        i2c_csr.wfo(utra::i2c::EV_STATUS_I2C_INT, baz);
        let bar = i2c_csr.rf(utra::i2c::EV_STATUS_TXRX_DONE);
        i2c_csr.rmwf(utra::i2c::EV_STATUS_TXRX_DONE, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_STATUS_TXRX_DONE, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_STATUS_TXRX_DONE, 1);
        i2c_csr.wfo(utra::i2c::EV_STATUS_TXRX_DONE, baz);

        let foo = i2c_csr.r(utra::i2c::EV_PENDING);
        i2c_csr.wo(utra::i2c::EV_PENDING, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_PENDING_I2C_INT);
        i2c_csr.rmwf(utra::i2c::EV_PENDING_I2C_INT, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_PENDING_I2C_INT, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_PENDING_I2C_INT, 1);
        i2c_csr.wfo(utra::i2c::EV_PENDING_I2C_INT, baz);
        let bar = i2c_csr.rf(utra::i2c::EV_PENDING_TXRX_DONE);
        i2c_csr.rmwf(utra::i2c::EV_PENDING_TXRX_DONE, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_PENDING_TXRX_DONE, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_PENDING_TXRX_DONE, 1);
        i2c_csr.wfo(utra::i2c::EV_PENDING_TXRX_DONE, baz);

        let foo = i2c_csr.r(utra::i2c::EV_ENABLE);
        i2c_csr.wo(utra::i2c::EV_ENABLE, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_ENABLE_I2C_INT);
        i2c_csr.rmwf(utra::i2c::EV_ENABLE_I2C_INT, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_ENABLE_I2C_INT, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_ENABLE_I2C_INT, 1);
        i2c_csr.wfo(utra::i2c::EV_ENABLE_I2C_INT, baz);
        let bar = i2c_csr.rf(utra::i2c::EV_ENABLE_TXRX_DONE);
        i2c_csr.rmwf(utra::i2c::EV_ENABLE_TXRX_DONE, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_ENABLE_TXRX_DONE, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_ENABLE_TXRX_DONE, 1);
        i2c_csr.wfo(utra::i2c::EV_ENABLE_TXRX_DONE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_btevents_csr() {
        use super::*;
        let mut btevents_csr = CSR::new(HW_BTEVENTS_BASE as *mut u32);

        let foo = btevents_csr.r(utra::btevents::EV_STATUS);
        btevents_csr.wo(utra::btevents::EV_STATUS, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_STATUS_COM_INT);
        btevents_csr.rmwf(utra::btevents::EV_STATUS_COM_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_STATUS_COM_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_STATUS_COM_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_STATUS_COM_INT, baz);
        let bar = btevents_csr.rf(utra::btevents::EV_STATUS_RTC_INT);
        btevents_csr.rmwf(utra::btevents::EV_STATUS_RTC_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_STATUS_RTC_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_STATUS_RTC_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_STATUS_RTC_INT, baz);

        let foo = btevents_csr.r(utra::btevents::EV_PENDING);
        btevents_csr.wo(utra::btevents::EV_PENDING, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_PENDING_COM_INT);
        btevents_csr.rmwf(utra::btevents::EV_PENDING_COM_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_PENDING_COM_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_PENDING_COM_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_PENDING_COM_INT, baz);
        let bar = btevents_csr.rf(utra::btevents::EV_PENDING_RTC_INT);
        btevents_csr.rmwf(utra::btevents::EV_PENDING_RTC_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_PENDING_RTC_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_PENDING_RTC_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_PENDING_RTC_INT, baz);

        let foo = btevents_csr.r(utra::btevents::EV_ENABLE);
        btevents_csr.wo(utra::btevents::EV_ENABLE, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_ENABLE_COM_INT);
        btevents_csr.rmwf(utra::btevents::EV_ENABLE_COM_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_ENABLE_COM_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_ENABLE_COM_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_ENABLE_COM_INT, baz);
        let bar = btevents_csr.rf(utra::btevents::EV_ENABLE_RTC_INT);
        btevents_csr.rmwf(utra::btevents::EV_ENABLE_RTC_INT, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_ENABLE_RTC_INT, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_ENABLE_RTC_INT, 1);
        btevents_csr.wfo(utra::btevents::EV_ENABLE_RTC_INT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_messible_csr() {
        use super::*;
        let mut messible_csr = CSR::new(HW_MESSIBLE_BASE as *mut u32);

        let foo = messible_csr.r(utra::messible::IN);
        messible_csr.wo(utra::messible::IN, foo);
        let bar = messible_csr.rf(utra::messible::IN_IN);
        messible_csr.rmwf(utra::messible::IN_IN, bar);
        let mut baz = messible_csr.zf(utra::messible::IN_IN, bar);
        baz |= messible_csr.ms(utra::messible::IN_IN, 1);
        messible_csr.wfo(utra::messible::IN_IN, baz);

        let foo = messible_csr.r(utra::messible::OUT);
        messible_csr.wo(utra::messible::OUT, foo);
        let bar = messible_csr.rf(utra::messible::OUT_OUT);
        messible_csr.rmwf(utra::messible::OUT_OUT, bar);
        let mut baz = messible_csr.zf(utra::messible::OUT_OUT, bar);
        baz |= messible_csr.ms(utra::messible::OUT_OUT, 1);
        messible_csr.wfo(utra::messible::OUT_OUT, baz);

        let foo = messible_csr.r(utra::messible::STATUS);
        messible_csr.wo(utra::messible::STATUS, foo);
        let bar = messible_csr.rf(utra::messible::STATUS_FULL);
        messible_csr.rmwf(utra::messible::STATUS_FULL, bar);
        let mut baz = messible_csr.zf(utra::messible::STATUS_FULL, bar);
        baz |= messible_csr.ms(utra::messible::STATUS_FULL, 1);
        messible_csr.wfo(utra::messible::STATUS_FULL, baz);
        let bar = messible_csr.rf(utra::messible::STATUS_HAVE);
        messible_csr.rmwf(utra::messible::STATUS_HAVE, bar);
        let mut baz = messible_csr.zf(utra::messible::STATUS_HAVE, bar);
        baz |= messible_csr.ms(utra::messible::STATUS_HAVE, 1);
        messible_csr.wfo(utra::messible::STATUS_HAVE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_messible2_csr() {
        use super::*;
        let mut messible2_csr = CSR::new(HW_MESSIBLE2_BASE as *mut u32);

        let foo = messible2_csr.r(utra::messible2::IN);
        messible2_csr.wo(utra::messible2::IN, foo);
        let bar = messible2_csr.rf(utra::messible2::IN_IN);
        messible2_csr.rmwf(utra::messible2::IN_IN, bar);
        let mut baz = messible2_csr.zf(utra::messible2::IN_IN, bar);
        baz |= messible2_csr.ms(utra::messible2::IN_IN, 1);
        messible2_csr.wfo(utra::messible2::IN_IN, baz);

        let foo = messible2_csr.r(utra::messible2::OUT);
        messible2_csr.wo(utra::messible2::OUT, foo);
        let bar = messible2_csr.rf(utra::messible2::OUT_OUT);
        messible2_csr.rmwf(utra::messible2::OUT_OUT, bar);
        let mut baz = messible2_csr.zf(utra::messible2::OUT_OUT, bar);
        baz |= messible2_csr.ms(utra::messible2::OUT_OUT, 1);
        messible2_csr.wfo(utra::messible2::OUT_OUT, baz);

        let foo = messible2_csr.r(utra::messible2::STATUS);
        messible2_csr.wo(utra::messible2::STATUS, foo);
        let bar = messible2_csr.rf(utra::messible2::STATUS_FULL);
        messible2_csr.rmwf(utra::messible2::STATUS_FULL, bar);
        let mut baz = messible2_csr.zf(utra::messible2::STATUS_FULL, bar);
        baz |= messible2_csr.ms(utra::messible2::STATUS_FULL, 1);
        messible2_csr.wfo(utra::messible2::STATUS_FULL, baz);
        let bar = messible2_csr.rf(utra::messible2::STATUS_HAVE);
        messible2_csr.rmwf(utra::messible2::STATUS_HAVE, bar);
        let mut baz = messible2_csr.zf(utra::messible2::STATUS_HAVE, bar);
        baz |= messible2_csr.ms(utra::messible2::STATUS_HAVE, 1);
        messible2_csr.wfo(utra::messible2::STATUS_HAVE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_ticktimer_csr() {
        use super::*;
        let mut ticktimer_csr = CSR::new(HW_TICKTIMER_BASE as *mut u32);

        let foo = ticktimer_csr.r(utra::ticktimer::CONTROL);
        ticktimer_csr.wo(utra::ticktimer::CONTROL, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::CONTROL_RESET);
        ticktimer_csr.rmwf(utra::ticktimer::CONTROL_RESET, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::CONTROL_RESET, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::CONTROL_RESET, 1);
        ticktimer_csr.wfo(utra::ticktimer::CONTROL_RESET, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::TIME1);
        ticktimer_csr.wo(utra::ticktimer::TIME1, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::TIME1_TIME);
        ticktimer_csr.rmwf(utra::ticktimer::TIME1_TIME, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::TIME1_TIME, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::TIME1_TIME, 1);
        ticktimer_csr.wfo(utra::ticktimer::TIME1_TIME, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::TIME0);
        ticktimer_csr.wo(utra::ticktimer::TIME0, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::TIME0_TIME);
        ticktimer_csr.rmwf(utra::ticktimer::TIME0_TIME, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::TIME0_TIME, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::TIME0_TIME, 1);
        ticktimer_csr.wfo(utra::ticktimer::TIME0_TIME, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::MSLEEP_TARGET1);
        ticktimer_csr.wo(utra::ticktimer::MSLEEP_TARGET1, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::MSLEEP_TARGET1_MSLEEP_TARGET);
        ticktimer_csr.rmwf(utra::ticktimer::MSLEEP_TARGET1_MSLEEP_TARGET, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::MSLEEP_TARGET1_MSLEEP_TARGET, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::MSLEEP_TARGET1_MSLEEP_TARGET, 1);
        ticktimer_csr.wfo(utra::ticktimer::MSLEEP_TARGET1_MSLEEP_TARGET, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::MSLEEP_TARGET0);
        ticktimer_csr.wo(utra::ticktimer::MSLEEP_TARGET0, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::MSLEEP_TARGET0_MSLEEP_TARGET);
        ticktimer_csr.rmwf(utra::ticktimer::MSLEEP_TARGET0_MSLEEP_TARGET, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::MSLEEP_TARGET0_MSLEEP_TARGET, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::MSLEEP_TARGET0_MSLEEP_TARGET, 1);
        ticktimer_csr.wfo(utra::ticktimer::MSLEEP_TARGET0_MSLEEP_TARGET, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::EV_STATUS);
        ticktimer_csr.wo(utra::ticktimer::EV_STATUS, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::EV_STATUS_ALARM);
        ticktimer_csr.rmwf(utra::ticktimer::EV_STATUS_ALARM, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::EV_STATUS_ALARM, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::EV_STATUS_ALARM, 1);
        ticktimer_csr.wfo(utra::ticktimer::EV_STATUS_ALARM, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::EV_PENDING);
        ticktimer_csr.wo(utra::ticktimer::EV_PENDING, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::EV_PENDING_ALARM);
        ticktimer_csr.rmwf(utra::ticktimer::EV_PENDING_ALARM, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::EV_PENDING_ALARM, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::EV_PENDING_ALARM, 1);
        ticktimer_csr.wfo(utra::ticktimer::EV_PENDING_ALARM, baz);

        let foo = ticktimer_csr.r(utra::ticktimer::EV_ENABLE);
        ticktimer_csr.wo(utra::ticktimer::EV_ENABLE, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::EV_ENABLE_ALARM);
        ticktimer_csr.rmwf(utra::ticktimer::EV_ENABLE_ALARM, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::EV_ENABLE_ALARM, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::EV_ENABLE_ALARM, 1);
        ticktimer_csr.wfo(utra::ticktimer::EV_ENABLE_ALARM, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_susres_csr() {
        use super::*;
        let mut susres_csr = CSR::new(HW_SUSRES_BASE as *mut u32);

        let foo = susres_csr.r(utra::susres::CONTROL);
        susres_csr.wo(utra::susres::CONTROL, foo);
        let bar = susres_csr.rf(utra::susres::CONTROL_PAUSE);
        susres_csr.rmwf(utra::susres::CONTROL_PAUSE, bar);
        let mut baz = susres_csr.zf(utra::susres::CONTROL_PAUSE, bar);
        baz |= susres_csr.ms(utra::susres::CONTROL_PAUSE, 1);
        susres_csr.wfo(utra::susres::CONTROL_PAUSE, baz);
        let bar = susres_csr.rf(utra::susres::CONTROL_LOAD);
        susres_csr.rmwf(utra::susres::CONTROL_LOAD, bar);
        let mut baz = susres_csr.zf(utra::susres::CONTROL_LOAD, bar);
        baz |= susres_csr.ms(utra::susres::CONTROL_LOAD, 1);
        susres_csr.wfo(utra::susres::CONTROL_LOAD, baz);

        let foo = susres_csr.r(utra::susres::RESUME_TIME1);
        susres_csr.wo(utra::susres::RESUME_TIME1, foo);
        let bar = susres_csr.rf(utra::susres::RESUME_TIME1_RESUME_TIME);
        susres_csr.rmwf(utra::susres::RESUME_TIME1_RESUME_TIME, bar);
        let mut baz = susres_csr.zf(utra::susres::RESUME_TIME1_RESUME_TIME, bar);
        baz |= susres_csr.ms(utra::susres::RESUME_TIME1_RESUME_TIME, 1);
        susres_csr.wfo(utra::susres::RESUME_TIME1_RESUME_TIME, baz);

        let foo = susres_csr.r(utra::susres::RESUME_TIME0);
        susres_csr.wo(utra::susres::RESUME_TIME0, foo);
        let bar = susres_csr.rf(utra::susres::RESUME_TIME0_RESUME_TIME);
        susres_csr.rmwf(utra::susres::RESUME_TIME0_RESUME_TIME, bar);
        let mut baz = susres_csr.zf(utra::susres::RESUME_TIME0_RESUME_TIME, bar);
        baz |= susres_csr.ms(utra::susres::RESUME_TIME0_RESUME_TIME, 1);
        susres_csr.wfo(utra::susres::RESUME_TIME0_RESUME_TIME, baz);

        let foo = susres_csr.r(utra::susres::TIME1);
        susres_csr.wo(utra::susres::TIME1, foo);
        let bar = susres_csr.rf(utra::susres::TIME1_TIME);
        susres_csr.rmwf(utra::susres::TIME1_TIME, bar);
        let mut baz = susres_csr.zf(utra::susres::TIME1_TIME, bar);
        baz |= susres_csr.ms(utra::susres::TIME1_TIME, 1);
        susres_csr.wfo(utra::susres::TIME1_TIME, baz);

        let foo = susres_csr.r(utra::susres::TIME0);
        susres_csr.wo(utra::susres::TIME0, foo);
        let bar = susres_csr.rf(utra::susres::TIME0_TIME);
        susres_csr.rmwf(utra::susres::TIME0_TIME, bar);
        let mut baz = susres_csr.zf(utra::susres::TIME0_TIME, bar);
        baz |= susres_csr.ms(utra::susres::TIME0_TIME, 1);
        susres_csr.wfo(utra::susres::TIME0_TIME, baz);

        let foo = susres_csr.r(utra::susres::STATUS);
        susres_csr.wo(utra::susres::STATUS, foo);
        let bar = susres_csr.rf(utra::susres::STATUS_PAUSED);
        susres_csr.rmwf(utra::susres::STATUS_PAUSED, bar);
        let mut baz = susres_csr.zf(utra::susres::STATUS_PAUSED, bar);
        baz |= susres_csr.ms(utra::susres::STATUS_PAUSED, 1);
        susres_csr.wfo(utra::susres::STATUS_PAUSED, baz);

        let foo = susres_csr.r(utra::susres::STATE);
        susres_csr.wo(utra::susres::STATE, foo);
        let bar = susres_csr.rf(utra::susres::STATE_RESUME);
        susres_csr.rmwf(utra::susres::STATE_RESUME, bar);
        let mut baz = susres_csr.zf(utra::susres::STATE_RESUME, bar);
        baz |= susres_csr.ms(utra::susres::STATE_RESUME, 1);
        susres_csr.wfo(utra::susres::STATE_RESUME, baz);
        let bar = susres_csr.rf(utra::susres::STATE_WAS_FORCED);
        susres_csr.rmwf(utra::susres::STATE_WAS_FORCED, bar);
        let mut baz = susres_csr.zf(utra::susres::STATE_WAS_FORCED, bar);
        baz |= susres_csr.ms(utra::susres::STATE_WAS_FORCED, 1);
        susres_csr.wfo(utra::susres::STATE_WAS_FORCED, baz);

        let foo = susres_csr.r(utra::susres::POWERDOWN);
        susres_csr.wo(utra::susres::POWERDOWN, foo);
        let bar = susres_csr.rf(utra::susres::POWERDOWN_POWERDOWN);
        susres_csr.rmwf(utra::susres::POWERDOWN_POWERDOWN, bar);
        let mut baz = susres_csr.zf(utra::susres::POWERDOWN_POWERDOWN, bar);
        baz |= susres_csr.ms(utra::susres::POWERDOWN_POWERDOWN, 1);
        susres_csr.wfo(utra::susres::POWERDOWN_POWERDOWN, baz);

        let foo = susres_csr.r(utra::susres::WFI);
        susres_csr.wo(utra::susres::WFI, foo);
        let bar = susres_csr.rf(utra::susres::WFI_OVERRIDE);
        susres_csr.rmwf(utra::susres::WFI_OVERRIDE, bar);
        let mut baz = susres_csr.zf(utra::susres::WFI_OVERRIDE, bar);
        baz |= susres_csr.ms(utra::susres::WFI_OVERRIDE, 1);
        susres_csr.wfo(utra::susres::WFI_OVERRIDE, baz);

        let foo = susres_csr.r(utra::susres::INTERRUPT);
        susres_csr.wo(utra::susres::INTERRUPT, foo);
        let bar = susres_csr.rf(utra::susres::INTERRUPT_INTERRUPT);
        susres_csr.rmwf(utra::susres::INTERRUPT_INTERRUPT, bar);
        let mut baz = susres_csr.zf(utra::susres::INTERRUPT_INTERRUPT, bar);
        baz |= susres_csr.ms(utra::susres::INTERRUPT_INTERRUPT, 1);
        susres_csr.wfo(utra::susres::INTERRUPT_INTERRUPT, baz);

        let foo = susres_csr.r(utra::susres::EV_STATUS);
        susres_csr.wo(utra::susres::EV_STATUS, foo);
        let bar = susres_csr.rf(utra::susres::EV_STATUS_SOFT_INT);
        susres_csr.rmwf(utra::susres::EV_STATUS_SOFT_INT, bar);
        let mut baz = susres_csr.zf(utra::susres::EV_STATUS_SOFT_INT, bar);
        baz |= susres_csr.ms(utra::susres::EV_STATUS_SOFT_INT, 1);
        susres_csr.wfo(utra::susres::EV_STATUS_SOFT_INT, baz);

        let foo = susres_csr.r(utra::susres::EV_PENDING);
        susres_csr.wo(utra::susres::EV_PENDING, foo);
        let bar = susres_csr.rf(utra::susres::EV_PENDING_SOFT_INT);
        susres_csr.rmwf(utra::susres::EV_PENDING_SOFT_INT, bar);
        let mut baz = susres_csr.zf(utra::susres::EV_PENDING_SOFT_INT, bar);
        baz |= susres_csr.ms(utra::susres::EV_PENDING_SOFT_INT, 1);
        susres_csr.wfo(utra::susres::EV_PENDING_SOFT_INT, baz);

        let foo = susres_csr.r(utra::susres::EV_ENABLE);
        susres_csr.wo(utra::susres::EV_ENABLE, foo);
        let bar = susres_csr.rf(utra::susres::EV_ENABLE_SOFT_INT);
        susres_csr.rmwf(utra::susres::EV_ENABLE_SOFT_INT, bar);
        let mut baz = susres_csr.zf(utra::susres::EV_ENABLE_SOFT_INT, bar);
        baz |= susres_csr.ms(utra::susres::EV_ENABLE_SOFT_INT, 1);
        susres_csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_power_csr() {
        use super::*;
        let mut power_csr = CSR::new(HW_POWER_BASE as *mut u32);

        let foo = power_csr.r(utra::power::POWER);
        power_csr.wo(utra::power::POWER, foo);
        let bar = power_csr.rf(utra::power::POWER_AUDIO);
        power_csr.rmwf(utra::power::POWER_AUDIO, bar);
        let mut baz = power_csr.zf(utra::power::POWER_AUDIO, bar);
        baz |= power_csr.ms(utra::power::POWER_AUDIO, 1);
        power_csr.wfo(utra::power::POWER_AUDIO, baz);
        let bar = power_csr.rf(utra::power::POWER_SELF);
        power_csr.rmwf(utra::power::POWER_SELF, bar);
        let mut baz = power_csr.zf(utra::power::POWER_SELF, bar);
        baz |= power_csr.ms(utra::power::POWER_SELF, 1);
        power_csr.wfo(utra::power::POWER_SELF, baz);
        let bar = power_csr.rf(utra::power::POWER_EC_SNOOP);
        power_csr.rmwf(utra::power::POWER_EC_SNOOP, bar);
        let mut baz = power_csr.zf(utra::power::POWER_EC_SNOOP, bar);
        baz |= power_csr.ms(utra::power::POWER_EC_SNOOP, 1);
        power_csr.wfo(utra::power::POWER_EC_SNOOP, baz);
        let bar = power_csr.rf(utra::power::POWER_STATE);
        power_csr.rmwf(utra::power::POWER_STATE, bar);
        let mut baz = power_csr.zf(utra::power::POWER_STATE, bar);
        baz |= power_csr.ms(utra::power::POWER_STATE, 1);
        power_csr.wfo(utra::power::POWER_STATE, baz);
        let bar = power_csr.rf(utra::power::POWER_RESET_EC);
        power_csr.rmwf(utra::power::POWER_RESET_EC, bar);
        let mut baz = power_csr.zf(utra::power::POWER_RESET_EC, bar);
        baz |= power_csr.ms(utra::power::POWER_RESET_EC, 1);
        power_csr.wfo(utra::power::POWER_RESET_EC, baz);
        let bar = power_csr.rf(utra::power::POWER_UP5K_ON);
        power_csr.rmwf(utra::power::POWER_UP5K_ON, bar);
        let mut baz = power_csr.zf(utra::power::POWER_UP5K_ON, bar);
        baz |= power_csr.ms(utra::power::POWER_UP5K_ON, 1);
        power_csr.wfo(utra::power::POWER_UP5K_ON, baz);
        let bar = power_csr.rf(utra::power::POWER_BOOSTMODE);
        power_csr.rmwf(utra::power::POWER_BOOSTMODE, bar);
        let mut baz = power_csr.zf(utra::power::POWER_BOOSTMODE, bar);
        baz |= power_csr.ms(utra::power::POWER_BOOSTMODE, 1);
        power_csr.wfo(utra::power::POWER_BOOSTMODE, baz);
        let bar = power_csr.rf(utra::power::POWER_SELFDESTRUCT);
        power_csr.rmwf(utra::power::POWER_SELFDESTRUCT, bar);
        let mut baz = power_csr.zf(utra::power::POWER_SELFDESTRUCT, bar);
        baz |= power_csr.ms(utra::power::POWER_SELFDESTRUCT, 1);
        power_csr.wfo(utra::power::POWER_SELFDESTRUCT, baz);
        let bar = power_csr.rf(utra::power::POWER_CRYPTO_ON);
        power_csr.rmwf(utra::power::POWER_CRYPTO_ON, bar);
        let mut baz = power_csr.zf(utra::power::POWER_CRYPTO_ON, bar);
        baz |= power_csr.ms(utra::power::POWER_CRYPTO_ON, 1);
        power_csr.wfo(utra::power::POWER_CRYPTO_ON, baz);
        let bar = power_csr.rf(utra::power::POWER_IGNORE_LOCKED);
        power_csr.rmwf(utra::power::POWER_IGNORE_LOCKED, bar);
        let mut baz = power_csr.zf(utra::power::POWER_IGNORE_LOCKED, bar);
        baz |= power_csr.ms(utra::power::POWER_IGNORE_LOCKED, 1);
        power_csr.wfo(utra::power::POWER_IGNORE_LOCKED, baz);
        let bar = power_csr.rf(utra::power::POWER_DISABLE_WFI);
        power_csr.rmwf(utra::power::POWER_DISABLE_WFI, bar);
        let mut baz = power_csr.zf(utra::power::POWER_DISABLE_WFI, bar);
        baz |= power_csr.ms(utra::power::POWER_DISABLE_WFI, 1);
        power_csr.wfo(utra::power::POWER_DISABLE_WFI, baz);

        let foo = power_csr.r(utra::power::CLK_STATUS);
        power_csr.wo(utra::power::CLK_STATUS, foo);
        let bar = power_csr.rf(utra::power::CLK_STATUS_CRYPTO_ON);
        power_csr.rmwf(utra::power::CLK_STATUS_CRYPTO_ON, bar);
        let mut baz = power_csr.zf(utra::power::CLK_STATUS_CRYPTO_ON, bar);
        baz |= power_csr.ms(utra::power::CLK_STATUS_CRYPTO_ON, 1);
        power_csr.wfo(utra::power::CLK_STATUS_CRYPTO_ON, baz);
        let bar = power_csr.rf(utra::power::CLK_STATUS_SHA_ON);
        power_csr.rmwf(utra::power::CLK_STATUS_SHA_ON, bar);
        let mut baz = power_csr.zf(utra::power::CLK_STATUS_SHA_ON, bar);
        baz |= power_csr.ms(utra::power::CLK_STATUS_SHA_ON, 1);
        power_csr.wfo(utra::power::CLK_STATUS_SHA_ON, baz);
        let bar = power_csr.rf(utra::power::CLK_STATUS_ENGINE_ON);
        power_csr.rmwf(utra::power::CLK_STATUS_ENGINE_ON, bar);
        let mut baz = power_csr.zf(utra::power::CLK_STATUS_ENGINE_ON, bar);
        baz |= power_csr.ms(utra::power::CLK_STATUS_ENGINE_ON, 1);
        power_csr.wfo(utra::power::CLK_STATUS_ENGINE_ON, baz);
        let bar = power_csr.rf(utra::power::CLK_STATUS_BTPOWER_ON);
        power_csr.rmwf(utra::power::CLK_STATUS_BTPOWER_ON, bar);
        let mut baz = power_csr.zf(utra::power::CLK_STATUS_BTPOWER_ON, bar);
        baz |= power_csr.ms(utra::power::CLK_STATUS_BTPOWER_ON, 1);
        power_csr.wfo(utra::power::CLK_STATUS_BTPOWER_ON, baz);

        let foo = power_csr.r(utra::power::WAKEUP_SOURCE);
        power_csr.wo(utra::power::WAKEUP_SOURCE, foo);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_KBD);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_KBD, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_KBD, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_KBD, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_KBD, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_TICKTIMER);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_TICKTIMER, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_TICKTIMER, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_TICKTIMER, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_TICKTIMER, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_TIMER0);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_TIMER0, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_TIMER0, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_TIMER0, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_TIMER0, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_USB);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_USB, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_USB, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_USB, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_USB, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_AUDIO);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_AUDIO, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_AUDIO, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_AUDIO, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_AUDIO, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_COM);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_COM, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_COM, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_COM, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_COM, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_RTC);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_RTC, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_RTC, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_RTC, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_RTC, baz);
        let bar = power_csr.rf(utra::power::WAKEUP_SOURCE_CONSOLE);
        power_csr.rmwf(utra::power::WAKEUP_SOURCE_CONSOLE, bar);
        let mut baz = power_csr.zf(utra::power::WAKEUP_SOURCE_CONSOLE, bar);
        baz |= power_csr.ms(utra::power::WAKEUP_SOURCE_CONSOLE, 1);
        power_csr.wfo(utra::power::WAKEUP_SOURCE_CONSOLE, baz);

        let foo = power_csr.r(utra::power::ACTIVITY_RATE);
        power_csr.wo(utra::power::ACTIVITY_RATE, foo);
        let bar = power_csr.rf(utra::power::ACTIVITY_RATE_COUNTS_AWAKE);
        power_csr.rmwf(utra::power::ACTIVITY_RATE_COUNTS_AWAKE, bar);
        let mut baz = power_csr.zf(utra::power::ACTIVITY_RATE_COUNTS_AWAKE, bar);
        baz |= power_csr.ms(utra::power::ACTIVITY_RATE_COUNTS_AWAKE, 1);
        power_csr.wfo(utra::power::ACTIVITY_RATE_COUNTS_AWAKE, baz);

        let foo = power_csr.r(utra::power::SAMPLING_PERIOD);
        power_csr.wo(utra::power::SAMPLING_PERIOD, foo);
        let bar = power_csr.rf(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD);
        power_csr.rmwf(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, bar);
        let mut baz = power_csr.zf(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, bar);
        baz |= power_csr.ms(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, 1);
        power_csr.wfo(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, baz);
        let bar = power_csr.rf(utra::power::SAMPLING_PERIOD_KILL_SAMPLER);
        power_csr.rmwf(utra::power::SAMPLING_PERIOD_KILL_SAMPLER, bar);
        let mut baz = power_csr.zf(utra::power::SAMPLING_PERIOD_KILL_SAMPLER, bar);
        baz |= power_csr.ms(utra::power::SAMPLING_PERIOD_KILL_SAMPLER, 1);
        power_csr.wfo(utra::power::SAMPLING_PERIOD_KILL_SAMPLER, baz);

        let foo = power_csr.r(utra::power::VIBE);
        power_csr.wo(utra::power::VIBE, foo);
        let bar = power_csr.rf(utra::power::VIBE_VIBE);
        power_csr.rmwf(utra::power::VIBE_VIBE, bar);
        let mut baz = power_csr.zf(utra::power::VIBE_VIBE, bar);
        baz |= power_csr.ms(utra::power::VIBE_VIBE, 1);
        power_csr.wfo(utra::power::VIBE_VIBE, baz);

        let foo = power_csr.r(utra::power::EV_STATUS);
        power_csr.wo(utra::power::EV_STATUS, foo);
        let bar = power_csr.rf(utra::power::EV_STATUS_USB_ATTACH);
        power_csr.rmwf(utra::power::EV_STATUS_USB_ATTACH, bar);
        let mut baz = power_csr.zf(utra::power::EV_STATUS_USB_ATTACH, bar);
        baz |= power_csr.ms(utra::power::EV_STATUS_USB_ATTACH, 1);
        power_csr.wfo(utra::power::EV_STATUS_USB_ATTACH, baz);
        let bar = power_csr.rf(utra::power::EV_STATUS_ACTIVITY_UPDATE);
        power_csr.rmwf(utra::power::EV_STATUS_ACTIVITY_UPDATE, bar);
        let mut baz = power_csr.zf(utra::power::EV_STATUS_ACTIVITY_UPDATE, bar);
        baz |= power_csr.ms(utra::power::EV_STATUS_ACTIVITY_UPDATE, 1);
        power_csr.wfo(utra::power::EV_STATUS_ACTIVITY_UPDATE, baz);

        let foo = power_csr.r(utra::power::EV_PENDING);
        power_csr.wo(utra::power::EV_PENDING, foo);
        let bar = power_csr.rf(utra::power::EV_PENDING_USB_ATTACH);
        power_csr.rmwf(utra::power::EV_PENDING_USB_ATTACH, bar);
        let mut baz = power_csr.zf(utra::power::EV_PENDING_USB_ATTACH, bar);
        baz |= power_csr.ms(utra::power::EV_PENDING_USB_ATTACH, 1);
        power_csr.wfo(utra::power::EV_PENDING_USB_ATTACH, baz);
        let bar = power_csr.rf(utra::power::EV_PENDING_ACTIVITY_UPDATE);
        power_csr.rmwf(utra::power::EV_PENDING_ACTIVITY_UPDATE, bar);
        let mut baz = power_csr.zf(utra::power::EV_PENDING_ACTIVITY_UPDATE, bar);
        baz |= power_csr.ms(utra::power::EV_PENDING_ACTIVITY_UPDATE, 1);
        power_csr.wfo(utra::power::EV_PENDING_ACTIVITY_UPDATE, baz);

        let foo = power_csr.r(utra::power::EV_ENABLE);
        power_csr.wo(utra::power::EV_ENABLE, foo);
        let bar = power_csr.rf(utra::power::EV_ENABLE_USB_ATTACH);
        power_csr.rmwf(utra::power::EV_ENABLE_USB_ATTACH, bar);
        let mut baz = power_csr.zf(utra::power::EV_ENABLE_USB_ATTACH, bar);
        baz |= power_csr.ms(utra::power::EV_ENABLE_USB_ATTACH, 1);
        power_csr.wfo(utra::power::EV_ENABLE_USB_ATTACH, baz);
        let bar = power_csr.rf(utra::power::EV_ENABLE_ACTIVITY_UPDATE);
        power_csr.rmwf(utra::power::EV_ENABLE_ACTIVITY_UPDATE, bar);
        let mut baz = power_csr.zf(utra::power::EV_ENABLE_ACTIVITY_UPDATE, bar);
        baz |= power_csr.ms(utra::power::EV_ENABLE_ACTIVITY_UPDATE, 1);
        power_csr.wfo(utra::power::EV_ENABLE_ACTIVITY_UPDATE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_spinor_soft_int_csr() {
        use super::*;
        let mut spinor_soft_int_csr = CSR::new(HW_SPINOR_SOFT_INT_BASE as *mut u32);

        let foo = spinor_soft_int_csr.r(utra::spinor_soft_int::EV_STATUS);
        spinor_soft_int_csr.wo(utra::spinor_soft_int::EV_STATUS, foo);
        let bar = spinor_soft_int_csr.rf(utra::spinor_soft_int::EV_STATUS_SPINOR_INT);
        spinor_soft_int_csr.rmwf(utra::spinor_soft_int::EV_STATUS_SPINOR_INT, bar);
        let mut baz = spinor_soft_int_csr.zf(utra::spinor_soft_int::EV_STATUS_SPINOR_INT, bar);
        baz |= spinor_soft_int_csr.ms(utra::spinor_soft_int::EV_STATUS_SPINOR_INT, 1);
        spinor_soft_int_csr.wfo(utra::spinor_soft_int::EV_STATUS_SPINOR_INT, baz);

        let foo = spinor_soft_int_csr.r(utra::spinor_soft_int::EV_PENDING);
        spinor_soft_int_csr.wo(utra::spinor_soft_int::EV_PENDING, foo);
        let bar = spinor_soft_int_csr.rf(utra::spinor_soft_int::EV_PENDING_SPINOR_INT);
        spinor_soft_int_csr.rmwf(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, bar);
        let mut baz = spinor_soft_int_csr.zf(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, bar);
        baz |= spinor_soft_int_csr.ms(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, 1);
        spinor_soft_int_csr.wfo(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, baz);

        let foo = spinor_soft_int_csr.r(utra::spinor_soft_int::EV_ENABLE);
        spinor_soft_int_csr.wo(utra::spinor_soft_int::EV_ENABLE, foo);
        let bar = spinor_soft_int_csr.rf(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT);
        spinor_soft_int_csr.rmwf(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, bar);
        let mut baz = spinor_soft_int_csr.zf(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, bar);
        baz |= spinor_soft_int_csr.ms(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, 1);
        spinor_soft_int_csr.wfo(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, baz);

        let foo = spinor_soft_int_csr.r(utra::spinor_soft_int::SOFTINT);
        spinor_soft_int_csr.wo(utra::spinor_soft_int::SOFTINT, foo);
        let bar = spinor_soft_int_csr.rf(utra::spinor_soft_int::SOFTINT_SOFTINT);
        spinor_soft_int_csr.rmwf(utra::spinor_soft_int::SOFTINT_SOFTINT, bar);
        let mut baz = spinor_soft_int_csr.zf(utra::spinor_soft_int::SOFTINT_SOFTINT, bar);
        baz |= spinor_soft_int_csr.ms(utra::spinor_soft_int::SOFTINT_SOFTINT, 1);
        spinor_soft_int_csr.wfo(utra::spinor_soft_int::SOFTINT_SOFTINT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_spinor_csr() {
        use super::*;
        let mut spinor_csr = CSR::new(HW_SPINOR_BASE as *mut u32);

        let foo = spinor_csr.r(utra::spinor::CONFIG);
        spinor_csr.wo(utra::spinor::CONFIG, foo);
        let bar = spinor_csr.rf(utra::spinor::CONFIG_DUMMY);
        spinor_csr.rmwf(utra::spinor::CONFIG_DUMMY, bar);
        let mut baz = spinor_csr.zf(utra::spinor::CONFIG_DUMMY, bar);
        baz |= spinor_csr.ms(utra::spinor::CONFIG_DUMMY, 1);
        spinor_csr.wfo(utra::spinor::CONFIG_DUMMY, baz);

        let foo = spinor_csr.r(utra::spinor::DELAY_CONFIG);
        spinor_csr.wo(utra::spinor::DELAY_CONFIG, foo);
        let bar = spinor_csr.rf(utra::spinor::DELAY_CONFIG_D);
        spinor_csr.rmwf(utra::spinor::DELAY_CONFIG_D, bar);
        let mut baz = spinor_csr.zf(utra::spinor::DELAY_CONFIG_D, bar);
        baz |= spinor_csr.ms(utra::spinor::DELAY_CONFIG_D, 1);
        spinor_csr.wfo(utra::spinor::DELAY_CONFIG_D, baz);
        let bar = spinor_csr.rf(utra::spinor::DELAY_CONFIG_LOAD);
        spinor_csr.rmwf(utra::spinor::DELAY_CONFIG_LOAD, bar);
        let mut baz = spinor_csr.zf(utra::spinor::DELAY_CONFIG_LOAD, bar);
        baz |= spinor_csr.ms(utra::spinor::DELAY_CONFIG_LOAD, 1);
        spinor_csr.wfo(utra::spinor::DELAY_CONFIG_LOAD, baz);

        let foo = spinor_csr.r(utra::spinor::DELAY_STATUS);
        spinor_csr.wo(utra::spinor::DELAY_STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::DELAY_STATUS_Q);
        spinor_csr.rmwf(utra::spinor::DELAY_STATUS_Q, bar);
        let mut baz = spinor_csr.zf(utra::spinor::DELAY_STATUS_Q, bar);
        baz |= spinor_csr.ms(utra::spinor::DELAY_STATUS_Q, 1);
        spinor_csr.wfo(utra::spinor::DELAY_STATUS_Q, baz);

        let foo = spinor_csr.r(utra::spinor::COMMAND);
        spinor_csr.wo(utra::spinor::COMMAND, foo);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_WAKEUP);
        spinor_csr.rmwf(utra::spinor::COMMAND_WAKEUP, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_WAKEUP, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_WAKEUP, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_WAKEUP, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_EXEC_CMD);
        spinor_csr.rmwf(utra::spinor::COMMAND_EXEC_CMD, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_EXEC_CMD, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_EXEC_CMD, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_CMD_CODE);
        spinor_csr.rmwf(utra::spinor::COMMAND_CMD_CODE, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_CMD_CODE, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_CMD_CODE, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_CMD_CODE, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_HAS_ARG);
        spinor_csr.rmwf(utra::spinor::COMMAND_HAS_ARG, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_HAS_ARG, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_HAS_ARG, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_HAS_ARG, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_DUMMY_CYCLES);
        spinor_csr.rmwf(utra::spinor::COMMAND_DUMMY_CYCLES, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_DUMMY_CYCLES, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_DUMMY_CYCLES, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_DUMMY_CYCLES, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_DATA_WORDS);
        spinor_csr.rmwf(utra::spinor::COMMAND_DATA_WORDS, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_DATA_WORDS, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_DATA_WORDS, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_DATA_WORDS, baz);
        let bar = spinor_csr.rf(utra::spinor::COMMAND_LOCK_READS);
        spinor_csr.rmwf(utra::spinor::COMMAND_LOCK_READS, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_LOCK_READS, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_LOCK_READS, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_LOCK_READS, baz);

        let foo = spinor_csr.r(utra::spinor::CMD_ARG);
        spinor_csr.wo(utra::spinor::CMD_ARG, foo);
        let bar = spinor_csr.rf(utra::spinor::CMD_ARG_CMD_ARG);
        spinor_csr.rmwf(utra::spinor::CMD_ARG_CMD_ARG, bar);
        let mut baz = spinor_csr.zf(utra::spinor::CMD_ARG_CMD_ARG, bar);
        baz |= spinor_csr.ms(utra::spinor::CMD_ARG_CMD_ARG, 1);
        spinor_csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, baz);

        let foo = spinor_csr.r(utra::spinor::CMD_RBK_DATA);
        spinor_csr.wo(utra::spinor::CMD_RBK_DATA, foo);
        let bar = spinor_csr.rf(utra::spinor::CMD_RBK_DATA_CMD_RBK_DATA);
        spinor_csr.rmwf(utra::spinor::CMD_RBK_DATA_CMD_RBK_DATA, bar);
        let mut baz = spinor_csr.zf(utra::spinor::CMD_RBK_DATA_CMD_RBK_DATA, bar);
        baz |= spinor_csr.ms(utra::spinor::CMD_RBK_DATA_CMD_RBK_DATA, 1);
        spinor_csr.wfo(utra::spinor::CMD_RBK_DATA_CMD_RBK_DATA, baz);

        let foo = spinor_csr.r(utra::spinor::STATUS);
        spinor_csr.wo(utra::spinor::STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::STATUS_WIP);
        spinor_csr.rmwf(utra::spinor::STATUS_WIP, bar);
        let mut baz = spinor_csr.zf(utra::spinor::STATUS_WIP, bar);
        baz |= spinor_csr.ms(utra::spinor::STATUS_WIP, 1);
        spinor_csr.wfo(utra::spinor::STATUS_WIP, baz);

        let foo = spinor_csr.r(utra::spinor::WDATA);
        spinor_csr.wo(utra::spinor::WDATA, foo);
        let bar = spinor_csr.rf(utra::spinor::WDATA_WDATA);
        spinor_csr.rmwf(utra::spinor::WDATA_WDATA, bar);
        let mut baz = spinor_csr.zf(utra::spinor::WDATA_WDATA, bar);
        baz |= spinor_csr.ms(utra::spinor::WDATA_WDATA, 1);
        spinor_csr.wfo(utra::spinor::WDATA_WDATA, baz);

        let foo = spinor_csr.r(utra::spinor::EV_STATUS);
        spinor_csr.wo(utra::spinor::EV_STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_STATUS_ECC_ERROR);
        spinor_csr.rmwf(utra::spinor::EV_STATUS_ECC_ERROR, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_STATUS_ECC_ERROR, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_STATUS_ECC_ERROR, 1);
        spinor_csr.wfo(utra::spinor::EV_STATUS_ECC_ERROR, baz);

        let foo = spinor_csr.r(utra::spinor::EV_PENDING);
        spinor_csr.wo(utra::spinor::EV_PENDING, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_PENDING_ECC_ERROR);
        spinor_csr.rmwf(utra::spinor::EV_PENDING_ECC_ERROR, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_PENDING_ECC_ERROR, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_PENDING_ECC_ERROR, 1);
        spinor_csr.wfo(utra::spinor::EV_PENDING_ECC_ERROR, baz);

        let foo = spinor_csr.r(utra::spinor::EV_ENABLE);
        spinor_csr.wo(utra::spinor::EV_ENABLE, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_ENABLE_ECC_ERROR);
        spinor_csr.rmwf(utra::spinor::EV_ENABLE_ECC_ERROR, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_ENABLE_ECC_ERROR, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_ENABLE_ECC_ERROR, 1);
        spinor_csr.wfo(utra::spinor::EV_ENABLE_ECC_ERROR, baz);

        let foo = spinor_csr.r(utra::spinor::ECC_ADDRESS);
        spinor_csr.wo(utra::spinor::ECC_ADDRESS, foo);
        let bar = spinor_csr.rf(utra::spinor::ECC_ADDRESS_ECC_ADDRESS);
        spinor_csr.rmwf(utra::spinor::ECC_ADDRESS_ECC_ADDRESS, bar);
        let mut baz = spinor_csr.zf(utra::spinor::ECC_ADDRESS_ECC_ADDRESS, bar);
        baz |= spinor_csr.ms(utra::spinor::ECC_ADDRESS_ECC_ADDRESS, 1);
        spinor_csr.wfo(utra::spinor::ECC_ADDRESS_ECC_ADDRESS, baz);

        let foo = spinor_csr.r(utra::spinor::ECC_STATUS);
        spinor_csr.wo(utra::spinor::ECC_STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::ECC_STATUS_ECC_ERROR);
        spinor_csr.rmwf(utra::spinor::ECC_STATUS_ECC_ERROR, bar);
        let mut baz = spinor_csr.zf(utra::spinor::ECC_STATUS_ECC_ERROR, bar);
        baz |= spinor_csr.ms(utra::spinor::ECC_STATUS_ECC_ERROR, 1);
        spinor_csr.wfo(utra::spinor::ECC_STATUS_ECC_ERROR, baz);
        let bar = spinor_csr.rf(utra::spinor::ECC_STATUS_ECC_OVERFLOW);
        spinor_csr.rmwf(utra::spinor::ECC_STATUS_ECC_OVERFLOW, bar);
        let mut baz = spinor_csr.zf(utra::spinor::ECC_STATUS_ECC_OVERFLOW, bar);
        baz |= spinor_csr.ms(utra::spinor::ECC_STATUS_ECC_OVERFLOW, 1);
        spinor_csr.wfo(utra::spinor::ECC_STATUS_ECC_OVERFLOW, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_keyboard_csr() {
        use super::*;
        let mut keyboard_csr = CSR::new(HW_KEYBOARD_BASE as *mut u32);

        let foo = keyboard_csr.r(utra::keyboard::UART_CHAR);
        keyboard_csr.wo(utra::keyboard::UART_CHAR, foo);
        let bar = keyboard_csr.rf(utra::keyboard::UART_CHAR_CHAR);
        keyboard_csr.rmwf(utra::keyboard::UART_CHAR_CHAR, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::UART_CHAR_CHAR, bar);
        baz |= keyboard_csr.ms(utra::keyboard::UART_CHAR_CHAR, 1);
        keyboard_csr.wfo(utra::keyboard::UART_CHAR_CHAR, baz);
        let bar = keyboard_csr.rf(utra::keyboard::UART_CHAR_STB);
        keyboard_csr.rmwf(utra::keyboard::UART_CHAR_STB, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::UART_CHAR_STB, bar);
        baz |= keyboard_csr.ms(utra::keyboard::UART_CHAR_STB, 1);
        keyboard_csr.wfo(utra::keyboard::UART_CHAR_STB, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW0DAT);
        keyboard_csr.wo(utra::keyboard::ROW0DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW0DAT_ROW0DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW0DAT_ROW0DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW0DAT_ROW0DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW0DAT_ROW0DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW0DAT_ROW0DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW1DAT);
        keyboard_csr.wo(utra::keyboard::ROW1DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW1DAT_ROW1DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW1DAT_ROW1DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW1DAT_ROW1DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW1DAT_ROW1DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW1DAT_ROW1DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW2DAT);
        keyboard_csr.wo(utra::keyboard::ROW2DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW2DAT_ROW2DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW2DAT_ROW2DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW2DAT_ROW2DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW2DAT_ROW2DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW2DAT_ROW2DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW3DAT);
        keyboard_csr.wo(utra::keyboard::ROW3DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW3DAT_ROW3DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW3DAT_ROW3DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW3DAT_ROW3DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW3DAT_ROW3DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW3DAT_ROW3DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW4DAT);
        keyboard_csr.wo(utra::keyboard::ROW4DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW4DAT_ROW4DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW4DAT_ROW4DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW4DAT_ROW4DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW4DAT_ROW4DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW4DAT_ROW4DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW5DAT);
        keyboard_csr.wo(utra::keyboard::ROW5DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW5DAT_ROW5DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW5DAT_ROW5DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW5DAT_ROW5DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW5DAT_ROW5DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW5DAT_ROW5DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW6DAT);
        keyboard_csr.wo(utra::keyboard::ROW6DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW6DAT_ROW6DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW6DAT_ROW6DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW6DAT_ROW6DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW6DAT_ROW6DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW6DAT_ROW6DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW7DAT);
        keyboard_csr.wo(utra::keyboard::ROW7DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW7DAT_ROW7DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW7DAT_ROW7DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW7DAT_ROW7DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW7DAT_ROW7DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW7DAT_ROW7DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROW8DAT);
        keyboard_csr.wo(utra::keyboard::ROW8DAT, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROW8DAT_ROW8DAT);
        keyboard_csr.rmwf(utra::keyboard::ROW8DAT_ROW8DAT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROW8DAT_ROW8DAT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROW8DAT_ROW8DAT, 1);
        keyboard_csr.wfo(utra::keyboard::ROW8DAT_ROW8DAT, baz);

        let foo = keyboard_csr.r(utra::keyboard::EV_STATUS);
        keyboard_csr.wo(utra::keyboard::EV_STATUS, foo);
        let bar = keyboard_csr.rf(utra::keyboard::EV_STATUS_KEYPRESSED);
        keyboard_csr.rmwf(utra::keyboard::EV_STATUS_KEYPRESSED, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_STATUS_KEYPRESSED, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_STATUS_KEYPRESSED, 1);
        keyboard_csr.wfo(utra::keyboard::EV_STATUS_KEYPRESSED, baz);
        let bar = keyboard_csr.rf(utra::keyboard::EV_STATUS_INJECT);
        keyboard_csr.rmwf(utra::keyboard::EV_STATUS_INJECT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_STATUS_INJECT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_STATUS_INJECT, 1);
        keyboard_csr.wfo(utra::keyboard::EV_STATUS_INJECT, baz);

        let foo = keyboard_csr.r(utra::keyboard::EV_PENDING);
        keyboard_csr.wo(utra::keyboard::EV_PENDING, foo);
        let bar = keyboard_csr.rf(utra::keyboard::EV_PENDING_KEYPRESSED);
        keyboard_csr.rmwf(utra::keyboard::EV_PENDING_KEYPRESSED, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_PENDING_KEYPRESSED, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_PENDING_KEYPRESSED, 1);
        keyboard_csr.wfo(utra::keyboard::EV_PENDING_KEYPRESSED, baz);
        let bar = keyboard_csr.rf(utra::keyboard::EV_PENDING_INJECT);
        keyboard_csr.rmwf(utra::keyboard::EV_PENDING_INJECT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_PENDING_INJECT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_PENDING_INJECT, 1);
        keyboard_csr.wfo(utra::keyboard::EV_PENDING_INJECT, baz);

        let foo = keyboard_csr.r(utra::keyboard::EV_ENABLE);
        keyboard_csr.wo(utra::keyboard::EV_ENABLE, foo);
        let bar = keyboard_csr.rf(utra::keyboard::EV_ENABLE_KEYPRESSED);
        keyboard_csr.rmwf(utra::keyboard::EV_ENABLE_KEYPRESSED, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_ENABLE_KEYPRESSED, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_ENABLE_KEYPRESSED, 1);
        keyboard_csr.wfo(utra::keyboard::EV_ENABLE_KEYPRESSED, baz);
        let bar = keyboard_csr.rf(utra::keyboard::EV_ENABLE_INJECT);
        keyboard_csr.rmwf(utra::keyboard::EV_ENABLE_INJECT, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_ENABLE_INJECT, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_ENABLE_INJECT, 1);
        keyboard_csr.wfo(utra::keyboard::EV_ENABLE_INJECT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_keyinject_csr() {
        use super::*;
        let mut keyinject_csr = CSR::new(HW_KEYINJECT_BASE as *mut u32);

        let foo = keyinject_csr.r(utra::keyinject::UART_CHAR);
        keyinject_csr.wo(utra::keyinject::UART_CHAR, foo);
        let bar = keyinject_csr.rf(utra::keyinject::UART_CHAR_CHAR);
        keyinject_csr.rmwf(utra::keyinject::UART_CHAR_CHAR, bar);
        let mut baz = keyinject_csr.zf(utra::keyinject::UART_CHAR_CHAR, bar);
        baz |= keyinject_csr.ms(utra::keyinject::UART_CHAR_CHAR, 1);
        keyinject_csr.wfo(utra::keyinject::UART_CHAR_CHAR, baz);

        let foo = keyinject_csr.r(utra::keyinject::DISABLE);
        keyinject_csr.wo(utra::keyinject::DISABLE, foo);
        let bar = keyinject_csr.rf(utra::keyinject::DISABLE_DISABLE);
        keyinject_csr.rmwf(utra::keyinject::DISABLE_DISABLE, bar);
        let mut baz = keyinject_csr.zf(utra::keyinject::DISABLE_DISABLE, bar);
        baz |= keyinject_csr.ms(utra::keyinject::DISABLE_DISABLE, 1);
        keyinject_csr.wfo(utra::keyinject::DISABLE_DISABLE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_seed_csr() {
        use super::*;
        let mut seed_csr = CSR::new(HW_SEED_BASE as *mut u32);

        let foo = seed_csr.r(utra::seed::SEED1);
        seed_csr.wo(utra::seed::SEED1, foo);
        let bar = seed_csr.rf(utra::seed::SEED1_SEED);
        seed_csr.rmwf(utra::seed::SEED1_SEED, bar);
        let mut baz = seed_csr.zf(utra::seed::SEED1_SEED, bar);
        baz |= seed_csr.ms(utra::seed::SEED1_SEED, 1);
        seed_csr.wfo(utra::seed::SEED1_SEED, baz);

        let foo = seed_csr.r(utra::seed::SEED0);
        seed_csr.wo(utra::seed::SEED0, foo);
        let bar = seed_csr.rf(utra::seed::SEED0_SEED);
        seed_csr.rmwf(utra::seed::SEED0_SEED, bar);
        let mut baz = seed_csr.zf(utra::seed::SEED0_SEED, bar);
        baz |= seed_csr.ms(utra::seed::SEED0_SEED, 1);
        seed_csr.wfo(utra::seed::SEED0_SEED, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_keyrom_csr() {
        use super::*;
        let mut keyrom_csr = CSR::new(HW_KEYROM_BASE as *mut u32);

        let foo = keyrom_csr.r(utra::keyrom::ADDRESS);
        keyrom_csr.wo(utra::keyrom::ADDRESS, foo);
        let bar = keyrom_csr.rf(utra::keyrom::ADDRESS_ADDRESS);
        keyrom_csr.rmwf(utra::keyrom::ADDRESS_ADDRESS, bar);
        let mut baz = keyrom_csr.zf(utra::keyrom::ADDRESS_ADDRESS, bar);
        baz |= keyrom_csr.ms(utra::keyrom::ADDRESS_ADDRESS, 1);
        keyrom_csr.wfo(utra::keyrom::ADDRESS_ADDRESS, baz);

        let foo = keyrom_csr.r(utra::keyrom::DATA);
        keyrom_csr.wo(utra::keyrom::DATA, foo);
        let bar = keyrom_csr.rf(utra::keyrom::DATA_DATA);
        keyrom_csr.rmwf(utra::keyrom::DATA_DATA, bar);
        let mut baz = keyrom_csr.zf(utra::keyrom::DATA_DATA, bar);
        baz |= keyrom_csr.ms(utra::keyrom::DATA_DATA, 1);
        keyrom_csr.wfo(utra::keyrom::DATA_DATA, baz);

        let foo = keyrom_csr.r(utra::keyrom::LOCKADDR);
        keyrom_csr.wo(utra::keyrom::LOCKADDR, foo);
        let bar = keyrom_csr.rf(utra::keyrom::LOCKADDR_LOCKADDR);
        keyrom_csr.rmwf(utra::keyrom::LOCKADDR_LOCKADDR, bar);
        let mut baz = keyrom_csr.zf(utra::keyrom::LOCKADDR_LOCKADDR, bar);
        baz |= keyrom_csr.ms(utra::keyrom::LOCKADDR_LOCKADDR, 1);
        keyrom_csr.wfo(utra::keyrom::LOCKADDR_LOCKADDR, baz);

        let foo = keyrom_csr.r(utra::keyrom::LOCKSTAT);
        keyrom_csr.wo(utra::keyrom::LOCKSTAT, foo);
        let bar = keyrom_csr.rf(utra::keyrom::LOCKSTAT_LOCKSTAT);
        keyrom_csr.rmwf(utra::keyrom::LOCKSTAT_LOCKSTAT, bar);
        let mut baz = keyrom_csr.zf(utra::keyrom::LOCKSTAT_LOCKSTAT, bar);
        baz |= keyrom_csr.ms(utra::keyrom::LOCKSTAT_LOCKSTAT, 1);
        keyrom_csr.wfo(utra::keyrom::LOCKSTAT_LOCKSTAT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_audio_csr() {
        use super::*;
        let mut audio_csr = CSR::new(HW_AUDIO_BASE as *mut u32);

        let foo = audio_csr.r(utra::audio::EV_STATUS);
        audio_csr.wo(utra::audio::EV_STATUS, foo);
        let bar = audio_csr.rf(utra::audio::EV_STATUS_RX_READY);
        audio_csr.rmwf(utra::audio::EV_STATUS_RX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_STATUS_RX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_STATUS_RX_READY, 1);
        audio_csr.wfo(utra::audio::EV_STATUS_RX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_STATUS_RX_ERROR);
        audio_csr.rmwf(utra::audio::EV_STATUS_RX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_STATUS_RX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_STATUS_RX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_STATUS_RX_ERROR, baz);
        let bar = audio_csr.rf(utra::audio::EV_STATUS_TX_READY);
        audio_csr.rmwf(utra::audio::EV_STATUS_TX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_STATUS_TX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_STATUS_TX_READY, 1);
        audio_csr.wfo(utra::audio::EV_STATUS_TX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_STATUS_TX_ERROR);
        audio_csr.rmwf(utra::audio::EV_STATUS_TX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_STATUS_TX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_STATUS_TX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_STATUS_TX_ERROR, baz);

        let foo = audio_csr.r(utra::audio::EV_PENDING);
        audio_csr.wo(utra::audio::EV_PENDING, foo);
        let bar = audio_csr.rf(utra::audio::EV_PENDING_RX_READY);
        audio_csr.rmwf(utra::audio::EV_PENDING_RX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_PENDING_RX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_PENDING_RX_READY, 1);
        audio_csr.wfo(utra::audio::EV_PENDING_RX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_PENDING_RX_ERROR);
        audio_csr.rmwf(utra::audio::EV_PENDING_RX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_PENDING_RX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_PENDING_RX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_PENDING_RX_ERROR, baz);
        let bar = audio_csr.rf(utra::audio::EV_PENDING_TX_READY);
        audio_csr.rmwf(utra::audio::EV_PENDING_TX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_PENDING_TX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_PENDING_TX_READY, 1);
        audio_csr.wfo(utra::audio::EV_PENDING_TX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_PENDING_TX_ERROR);
        audio_csr.rmwf(utra::audio::EV_PENDING_TX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_PENDING_TX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_PENDING_TX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_PENDING_TX_ERROR, baz);

        let foo = audio_csr.r(utra::audio::EV_ENABLE);
        audio_csr.wo(utra::audio::EV_ENABLE, foo);
        let bar = audio_csr.rf(utra::audio::EV_ENABLE_RX_READY);
        audio_csr.rmwf(utra::audio::EV_ENABLE_RX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_ENABLE_RX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_ENABLE_RX_READY, 1);
        audio_csr.wfo(utra::audio::EV_ENABLE_RX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_ENABLE_RX_ERROR);
        audio_csr.rmwf(utra::audio::EV_ENABLE_RX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_ENABLE_RX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_ENABLE_RX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_ENABLE_RX_ERROR, baz);
        let bar = audio_csr.rf(utra::audio::EV_ENABLE_TX_READY);
        audio_csr.rmwf(utra::audio::EV_ENABLE_TX_READY, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_ENABLE_TX_READY, bar);
        baz |= audio_csr.ms(utra::audio::EV_ENABLE_TX_READY, 1);
        audio_csr.wfo(utra::audio::EV_ENABLE_TX_READY, baz);
        let bar = audio_csr.rf(utra::audio::EV_ENABLE_TX_ERROR);
        audio_csr.rmwf(utra::audio::EV_ENABLE_TX_ERROR, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_ENABLE_TX_ERROR, bar);
        baz |= audio_csr.ms(utra::audio::EV_ENABLE_TX_ERROR, 1);
        audio_csr.wfo(utra::audio::EV_ENABLE_TX_ERROR, baz);

        let foo = audio_csr.r(utra::audio::RX_CTL);
        audio_csr.wo(utra::audio::RX_CTL, foo);
        let bar = audio_csr.rf(utra::audio::RX_CTL_ENABLE);
        audio_csr.rmwf(utra::audio::RX_CTL_ENABLE, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_CTL_ENABLE, bar);
        baz |= audio_csr.ms(utra::audio::RX_CTL_ENABLE, 1);
        audio_csr.wfo(utra::audio::RX_CTL_ENABLE, baz);
        let bar = audio_csr.rf(utra::audio::RX_CTL_RESET);
        audio_csr.rmwf(utra::audio::RX_CTL_RESET, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_CTL_RESET, bar);
        baz |= audio_csr.ms(utra::audio::RX_CTL_RESET, 1);
        audio_csr.wfo(utra::audio::RX_CTL_RESET, baz);

        let foo = audio_csr.r(utra::audio::RX_STAT);
        audio_csr.wo(utra::audio::RX_STAT, foo);
        let bar = audio_csr.rf(utra::audio::RX_STAT_OVERFLOW);
        audio_csr.rmwf(utra::audio::RX_STAT_OVERFLOW, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_OVERFLOW, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_OVERFLOW, 1);
        audio_csr.wfo(utra::audio::RX_STAT_OVERFLOW, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_UNDERFLOW);
        audio_csr.rmwf(utra::audio::RX_STAT_UNDERFLOW, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_UNDERFLOW, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_UNDERFLOW, 1);
        audio_csr.wfo(utra::audio::RX_STAT_UNDERFLOW, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_DATAREADY);
        audio_csr.rmwf(utra::audio::RX_STAT_DATAREADY, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_DATAREADY, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_DATAREADY, 1);
        audio_csr.wfo(utra::audio::RX_STAT_DATAREADY, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_EMPTY);
        audio_csr.rmwf(utra::audio::RX_STAT_EMPTY, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_EMPTY, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_EMPTY, 1);
        audio_csr.wfo(utra::audio::RX_STAT_EMPTY, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_WRCOUNT);
        audio_csr.rmwf(utra::audio::RX_STAT_WRCOUNT, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_WRCOUNT, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_WRCOUNT, 1);
        audio_csr.wfo(utra::audio::RX_STAT_WRCOUNT, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_RDCOUNT);
        audio_csr.rmwf(utra::audio::RX_STAT_RDCOUNT, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_RDCOUNT, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_RDCOUNT, 1);
        audio_csr.wfo(utra::audio::RX_STAT_RDCOUNT, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_FIFO_DEPTH);
        audio_csr.rmwf(utra::audio::RX_STAT_FIFO_DEPTH, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_FIFO_DEPTH, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_FIFO_DEPTH, 1);
        audio_csr.wfo(utra::audio::RX_STAT_FIFO_DEPTH, baz);
        let bar = audio_csr.rf(utra::audio::RX_STAT_CONCATENATE_CHANNELS);
        audio_csr.rmwf(utra::audio::RX_STAT_CONCATENATE_CHANNELS, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_STAT_CONCATENATE_CHANNELS, bar);
        baz |= audio_csr.ms(utra::audio::RX_STAT_CONCATENATE_CHANNELS, 1);
        audio_csr.wfo(utra::audio::RX_STAT_CONCATENATE_CHANNELS, baz);

        let foo = audio_csr.r(utra::audio::RX_CONF);
        audio_csr.wo(utra::audio::RX_CONF, foo);
        let bar = audio_csr.rf(utra::audio::RX_CONF_FORMAT);
        audio_csr.rmwf(utra::audio::RX_CONF_FORMAT, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_CONF_FORMAT, bar);
        baz |= audio_csr.ms(utra::audio::RX_CONF_FORMAT, 1);
        audio_csr.wfo(utra::audio::RX_CONF_FORMAT, baz);
        let bar = audio_csr.rf(utra::audio::RX_CONF_SAMPLE_WIDTH);
        audio_csr.rmwf(utra::audio::RX_CONF_SAMPLE_WIDTH, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_CONF_SAMPLE_WIDTH, bar);
        baz |= audio_csr.ms(utra::audio::RX_CONF_SAMPLE_WIDTH, 1);
        audio_csr.wfo(utra::audio::RX_CONF_SAMPLE_WIDTH, baz);
        let bar = audio_csr.rf(utra::audio::RX_CONF_LRCK_FREQ);
        audio_csr.rmwf(utra::audio::RX_CONF_LRCK_FREQ, bar);
        let mut baz = audio_csr.zf(utra::audio::RX_CONF_LRCK_FREQ, bar);
        baz |= audio_csr.ms(utra::audio::RX_CONF_LRCK_FREQ, 1);
        audio_csr.wfo(utra::audio::RX_CONF_LRCK_FREQ, baz);

        let foo = audio_csr.r(utra::audio::TX_CTL);
        audio_csr.wo(utra::audio::TX_CTL, foo);
        let bar = audio_csr.rf(utra::audio::TX_CTL_ENABLE);
        audio_csr.rmwf(utra::audio::TX_CTL_ENABLE, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_CTL_ENABLE, bar);
        baz |= audio_csr.ms(utra::audio::TX_CTL_ENABLE, 1);
        audio_csr.wfo(utra::audio::TX_CTL_ENABLE, baz);
        let bar = audio_csr.rf(utra::audio::TX_CTL_RESET);
        audio_csr.rmwf(utra::audio::TX_CTL_RESET, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_CTL_RESET, bar);
        baz |= audio_csr.ms(utra::audio::TX_CTL_RESET, 1);
        audio_csr.wfo(utra::audio::TX_CTL_RESET, baz);

        let foo = audio_csr.r(utra::audio::TX_STAT);
        audio_csr.wo(utra::audio::TX_STAT, foo);
        let bar = audio_csr.rf(utra::audio::TX_STAT_OVERFLOW);
        audio_csr.rmwf(utra::audio::TX_STAT_OVERFLOW, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_OVERFLOW, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_OVERFLOW, 1);
        audio_csr.wfo(utra::audio::TX_STAT_OVERFLOW, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_UNDERFLOW);
        audio_csr.rmwf(utra::audio::TX_STAT_UNDERFLOW, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_UNDERFLOW, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_UNDERFLOW, 1);
        audio_csr.wfo(utra::audio::TX_STAT_UNDERFLOW, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_FREE);
        audio_csr.rmwf(utra::audio::TX_STAT_FREE, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_FREE, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_FREE, 1);
        audio_csr.wfo(utra::audio::TX_STAT_FREE, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_ALMOSTFULL);
        audio_csr.rmwf(utra::audio::TX_STAT_ALMOSTFULL, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_ALMOSTFULL, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_ALMOSTFULL, 1);
        audio_csr.wfo(utra::audio::TX_STAT_ALMOSTFULL, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_FULL);
        audio_csr.rmwf(utra::audio::TX_STAT_FULL, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_FULL, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_FULL, 1);
        audio_csr.wfo(utra::audio::TX_STAT_FULL, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_EMPTY);
        audio_csr.rmwf(utra::audio::TX_STAT_EMPTY, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_EMPTY, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_EMPTY, 1);
        audio_csr.wfo(utra::audio::TX_STAT_EMPTY, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_WRCOUNT);
        audio_csr.rmwf(utra::audio::TX_STAT_WRCOUNT, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_WRCOUNT, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_WRCOUNT, 1);
        audio_csr.wfo(utra::audio::TX_STAT_WRCOUNT, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_RDCOUNT);
        audio_csr.rmwf(utra::audio::TX_STAT_RDCOUNT, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_RDCOUNT, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_RDCOUNT, 1);
        audio_csr.wfo(utra::audio::TX_STAT_RDCOUNT, baz);
        let bar = audio_csr.rf(utra::audio::TX_STAT_CONCATENATE_CHANNELS);
        audio_csr.rmwf(utra::audio::TX_STAT_CONCATENATE_CHANNELS, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_STAT_CONCATENATE_CHANNELS, bar);
        baz |= audio_csr.ms(utra::audio::TX_STAT_CONCATENATE_CHANNELS, 1);
        audio_csr.wfo(utra::audio::TX_STAT_CONCATENATE_CHANNELS, baz);

        let foo = audio_csr.r(utra::audio::TX_CONF);
        audio_csr.wo(utra::audio::TX_CONF, foo);
        let bar = audio_csr.rf(utra::audio::TX_CONF_FORMAT);
        audio_csr.rmwf(utra::audio::TX_CONF_FORMAT, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_CONF_FORMAT, bar);
        baz |= audio_csr.ms(utra::audio::TX_CONF_FORMAT, 1);
        audio_csr.wfo(utra::audio::TX_CONF_FORMAT, baz);
        let bar = audio_csr.rf(utra::audio::TX_CONF_SAMPLE_WIDTH);
        audio_csr.rmwf(utra::audio::TX_CONF_SAMPLE_WIDTH, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_CONF_SAMPLE_WIDTH, bar);
        baz |= audio_csr.ms(utra::audio::TX_CONF_SAMPLE_WIDTH, 1);
        audio_csr.wfo(utra::audio::TX_CONF_SAMPLE_WIDTH, baz);
        let bar = audio_csr.rf(utra::audio::TX_CONF_LRCK_FREQ);
        audio_csr.rmwf(utra::audio::TX_CONF_LRCK_FREQ, bar);
        let mut baz = audio_csr.zf(utra::audio::TX_CONF_LRCK_FREQ, bar);
        baz |= audio_csr.ms(utra::audio::TX_CONF_LRCK_FREQ, 1);
        audio_csr.wfo(utra::audio::TX_CONF_LRCK_FREQ, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_trng_kernel_csr() {
        use super::*;
        let mut trng_kernel_csr = CSR::new(HW_TRNG_KERNEL_BASE as *mut u32);

        let foo = trng_kernel_csr.r(utra::trng_kernel::STATUS);
        trng_kernel_csr.wo(utra::trng_kernel::STATUS, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::STATUS_READY);
        trng_kernel_csr.rmwf(utra::trng_kernel::STATUS_READY, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::STATUS_READY, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::STATUS_READY, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::STATUS_READY, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::STATUS_AVAIL);
        trng_kernel_csr.rmwf(utra::trng_kernel::STATUS_AVAIL, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::STATUS_AVAIL, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::STATUS_AVAIL, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::STATUS_AVAIL, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::STATUS_RDCOUNT);
        trng_kernel_csr.rmwf(utra::trng_kernel::STATUS_RDCOUNT, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::STATUS_RDCOUNT, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::STATUS_RDCOUNT, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::STATUS_RDCOUNT, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::STATUS_WRCOUNT);
        trng_kernel_csr.rmwf(utra::trng_kernel::STATUS_WRCOUNT, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::STATUS_WRCOUNT, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::STATUS_WRCOUNT, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::STATUS_WRCOUNT, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::DATA);
        trng_kernel_csr.wo(utra::trng_kernel::DATA, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::DATA_DATA);
        trng_kernel_csr.rmwf(utra::trng_kernel::DATA_DATA, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::DATA_DATA, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::DATA_DATA, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::DATA_DATA, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::URANDOM);
        trng_kernel_csr.wo(utra::trng_kernel::URANDOM, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::URANDOM_URANDOM);
        trng_kernel_csr.rmwf(utra::trng_kernel::URANDOM_URANDOM, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::URANDOM_URANDOM, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::URANDOM_URANDOM, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::URANDOM_URANDOM, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::URANDOM_VALID);
        trng_kernel_csr.wo(utra::trng_kernel::URANDOM_VALID, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID);
        trng_kernel_csr.rmwf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::URANDOM_VALID_URANDOM_VALID, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::EV_STATUS);
        trng_kernel_csr.wo(utra::trng_kernel::EV_STATUS, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_STATUS_AVAIL);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_STATUS_AVAIL, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_STATUS_AVAIL, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_STATUS_AVAIL, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_STATUS_AVAIL, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_STATUS_ERROR);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_STATUS_ERROR, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_STATUS_ERROR, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_STATUS_ERROR, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_STATUS_ERROR, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::EV_PENDING);
        trng_kernel_csr.wo(utra::trng_kernel::EV_PENDING, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_PENDING_AVAIL);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_PENDING_AVAIL, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_PENDING_AVAIL, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_PENDING_AVAIL, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_PENDING_AVAIL, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_PENDING_ERROR);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_PENDING_ERROR, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_PENDING_ERROR, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_PENDING_ERROR, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_PENDING_ERROR, baz);

        let foo = trng_kernel_csr.r(utra::trng_kernel::EV_ENABLE);
        trng_kernel_csr.wo(utra::trng_kernel::EV_ENABLE, foo);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_ENABLE_AVAIL);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_ENABLE_AVAIL, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_ENABLE_AVAIL, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_ENABLE_AVAIL, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_ENABLE_AVAIL, baz);
        let bar = trng_kernel_csr.rf(utra::trng_kernel::EV_ENABLE_ERROR);
        trng_kernel_csr.rmwf(utra::trng_kernel::EV_ENABLE_ERROR, bar);
        let mut baz = trng_kernel_csr.zf(utra::trng_kernel::EV_ENABLE_ERROR, bar);
        baz |= trng_kernel_csr.ms(utra::trng_kernel::EV_ENABLE_ERROR, 1);
        trng_kernel_csr.wfo(utra::trng_kernel::EV_ENABLE_ERROR, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_trng_server_csr() {
        use super::*;
        let mut trng_server_csr = CSR::new(HW_TRNG_SERVER_BASE as *mut u32);

        let foo = trng_server_csr.r(utra::trng_server::CONTROL);
        trng_server_csr.wo(utra::trng_server::CONTROL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::CONTROL_ENABLE);
        trng_server_csr.rmwf(utra::trng_server::CONTROL_ENABLE, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CONTROL_ENABLE, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CONTROL_ENABLE, 1);
        trng_server_csr.wfo(utra::trng_server::CONTROL_ENABLE, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CONTROL_RO_DIS);
        trng_server_csr.rmwf(utra::trng_server::CONTROL_RO_DIS, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CONTROL_RO_DIS, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CONTROL_RO_DIS, 1);
        trng_server_csr.wfo(utra::trng_server::CONTROL_RO_DIS, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CONTROL_AV_DIS);
        trng_server_csr.rmwf(utra::trng_server::CONTROL_AV_DIS, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CONTROL_AV_DIS, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CONTROL_AV_DIS, 1);
        trng_server_csr.wfo(utra::trng_server::CONTROL_AV_DIS, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CONTROL_POWERSAVE);
        trng_server_csr.rmwf(utra::trng_server::CONTROL_POWERSAVE, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CONTROL_POWERSAVE, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1);
        trng_server_csr.wfo(utra::trng_server::CONTROL_POWERSAVE, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CONTROL_CLR_ERR);
        trng_server_csr.rmwf(utra::trng_server::CONTROL_CLR_ERR, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CONTROL_CLR_ERR, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CONTROL_CLR_ERR, 1);
        trng_server_csr.wfo(utra::trng_server::CONTROL_CLR_ERR, baz);

        let foo = trng_server_csr.r(utra::trng_server::DATA);
        trng_server_csr.wo(utra::trng_server::DATA, foo);
        let bar = trng_server_csr.rf(utra::trng_server::DATA_DATA);
        trng_server_csr.rmwf(utra::trng_server::DATA_DATA, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::DATA_DATA, bar);
        baz |= trng_server_csr.ms(utra::trng_server::DATA_DATA, 1);
        trng_server_csr.wfo(utra::trng_server::DATA_DATA, baz);

        let foo = trng_server_csr.r(utra::trng_server::STATUS);
        trng_server_csr.wo(utra::trng_server::STATUS, foo);
        let bar = trng_server_csr.rf(utra::trng_server::STATUS_AVAIL);
        trng_server_csr.rmwf(utra::trng_server::STATUS_AVAIL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::STATUS_AVAIL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::STATUS_AVAIL, 1);
        trng_server_csr.wfo(utra::trng_server::STATUS_AVAIL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::STATUS_RDCOUNT);
        trng_server_csr.rmwf(utra::trng_server::STATUS_RDCOUNT, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::STATUS_RDCOUNT, bar);
        baz |= trng_server_csr.ms(utra::trng_server::STATUS_RDCOUNT, 1);
        trng_server_csr.wfo(utra::trng_server::STATUS_RDCOUNT, baz);
        let bar = trng_server_csr.rf(utra::trng_server::STATUS_WRCOUNT);
        trng_server_csr.rmwf(utra::trng_server::STATUS_WRCOUNT, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::STATUS_WRCOUNT, bar);
        baz |= trng_server_csr.ms(utra::trng_server::STATUS_WRCOUNT, 1);
        trng_server_csr.wfo(utra::trng_server::STATUS_WRCOUNT, baz);
        let bar = trng_server_csr.rf(utra::trng_server::STATUS_FULL);
        trng_server_csr.rmwf(utra::trng_server::STATUS_FULL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::STATUS_FULL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::STATUS_FULL, 1);
        trng_server_csr.wfo(utra::trng_server::STATUS_FULL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::STATUS_CHACHA_READY);
        trng_server_csr.rmwf(utra::trng_server::STATUS_CHACHA_READY, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::STATUS_CHACHA_READY, bar);
        baz |= trng_server_csr.ms(utra::trng_server::STATUS_CHACHA_READY, 1);
        trng_server_csr.wfo(utra::trng_server::STATUS_CHACHA_READY, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_CONFIG);
        trng_server_csr.wo(utra::trng_server::AV_CONFIG, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_CONFIG_POWERDELAY);
        trng_server_csr.rmwf(utra::trng_server::AV_CONFIG_POWERDELAY, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_CONFIG_POWERDELAY, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_CONFIG_POWERDELAY, 1);
        trng_server_csr.wfo(utra::trng_server::AV_CONFIG_POWERDELAY, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_CONFIG_SAMPLES);
        trng_server_csr.rmwf(utra::trng_server::AV_CONFIG_SAMPLES, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_CONFIG_SAMPLES, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_CONFIG_SAMPLES, 1);
        trng_server_csr.wfo(utra::trng_server::AV_CONFIG_SAMPLES, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_CONFIG_TEST);
        trng_server_csr.rmwf(utra::trng_server::AV_CONFIG_TEST, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_CONFIG_TEST, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_CONFIG_TEST, 1);
        trng_server_csr.wfo(utra::trng_server::AV_CONFIG_TEST, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_CONFIG_REQUIRED);
        trng_server_csr.rmwf(utra::trng_server::AV_CONFIG_REQUIRED, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_CONFIG_REQUIRED, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_CONFIG_REQUIRED, 1);
        trng_server_csr.wfo(utra::trng_server::AV_CONFIG_REQUIRED, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_CONFIG);
        trng_server_csr.wo(utra::trng_server::RO_CONFIG, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_CONFIG_GANG);
        trng_server_csr.rmwf(utra::trng_server::RO_CONFIG_GANG, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_CONFIG_GANG, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_CONFIG_GANG, 1);
        trng_server_csr.wfo(utra::trng_server::RO_CONFIG_GANG, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_CONFIG_DWELL);
        trng_server_csr.rmwf(utra::trng_server::RO_CONFIG_DWELL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_CONFIG_DWELL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_CONFIG_DWELL, 1);
        trng_server_csr.wfo(utra::trng_server::RO_CONFIG_DWELL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_CONFIG_DELAY);
        trng_server_csr.rmwf(utra::trng_server::RO_CONFIG_DELAY, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_CONFIG_DELAY, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_CONFIG_DELAY, 1);
        trng_server_csr.wfo(utra::trng_server::RO_CONFIG_DELAY, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_CONFIG_FUZZ);
        trng_server_csr.rmwf(utra::trng_server::RO_CONFIG_FUZZ, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_CONFIG_FUZZ, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_CONFIG_FUZZ, 1);
        trng_server_csr.wfo(utra::trng_server::RO_CONFIG_FUZZ, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_CONFIG_OVERSAMPLING);
        trng_server_csr.rmwf(utra::trng_server::RO_CONFIG_OVERSAMPLING, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_CONFIG_OVERSAMPLING, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_CONFIG_OVERSAMPLING, 1);
        trng_server_csr.wfo(utra::trng_server::RO_CONFIG_OVERSAMPLING, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_NIST);
        trng_server_csr.wo(utra::trng_server::AV_NIST, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_NIST_REPCOUNT_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::AV_NIST_REPCOUNT_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_NIST_REPCOUNT_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_NIST_REPCOUNT_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::AV_NIST_REPCOUNT_CUTOFF, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_NIST_ADAPTIVE_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::AV_NIST_ADAPTIVE_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_NIST_ADAPTIVE_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_NIST_ADAPTIVE_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::AV_NIST_ADAPTIVE_CUTOFF, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_NIST);
        trng_server_csr.wo(utra::trng_server::RO_NIST, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_NIST_REPCOUNT_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::RO_NIST_REPCOUNT_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_NIST_REPCOUNT_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_NIST_REPCOUNT_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::RO_NIST_REPCOUNT_CUTOFF, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_NIST_ADAPTIVE_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::RO_NIST_ADAPTIVE_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_NIST_ADAPTIVE_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_NIST_ADAPTIVE_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::RO_NIST_ADAPTIVE_CUTOFF, baz);

        let foo = trng_server_csr.r(utra::trng_server::UNDERRUNS);
        trng_server_csr.wo(utra::trng_server::UNDERRUNS, foo);
        let bar = trng_server_csr.rf(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN);
        trng_server_csr.rmwf(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN, 1);
        trng_server_csr.wfo(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN);
        trng_server_csr.rmwf(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN, 1);
        trng_server_csr.wfo(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_ERRORS);
        trng_server_csr.wo(utra::trng_server::NIST_ERRORS, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_ERRORS_AV_REPCOUNT);
        trng_server_csr.rmwf(utra::trng_server::NIST_ERRORS_AV_REPCOUNT, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_ERRORS_AV_REPCOUNT, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_ERRORS_AV_REPCOUNT, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_ERRORS_AV_REPCOUNT, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE);
        trng_server_csr.rmwf(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_ERRORS_RO_REPCOUNT);
        trng_server_csr.rmwf(utra::trng_server::NIST_ERRORS_RO_REPCOUNT, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_ERRORS_RO_REPCOUNT, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_ERRORS_RO_REPCOUNT, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_ERRORS_RO_REPCOUNT, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE);
        trng_server_csr.rmwf(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_ERRORS_RO_MINIRUNS);
        trng_server_csr.rmwf(utra::trng_server::NIST_ERRORS_RO_MINIRUNS, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_ERRORS_RO_MINIRUNS, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_ERRORS_RO_MINIRUNS, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_ERRORS_RO_MINIRUNS, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_RO_STAT0);
        trng_server_csr.wo(utra::trng_server::NIST_RO_STAT0, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT0_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT0_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT0_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT0_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT0_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT0_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT0_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT0_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT0_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT0_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT0_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT0_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT0_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT0_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT0_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_RO_STAT1);
        trng_server_csr.wo(utra::trng_server::NIST_RO_STAT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT1_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT1_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT1_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT1_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT1_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT1_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT1_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT1_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT1_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT1_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT1_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT1_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT1_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT1_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT1_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_RO_STAT2);
        trng_server_csr.wo(utra::trng_server::NIST_RO_STAT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT2_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT2_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT2_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT2_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT2_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT2_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT2_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT2_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT2_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT2_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT2_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT2_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT2_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT2_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT2_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_RO_STAT3);
        trng_server_csr.wo(utra::trng_server::NIST_RO_STAT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT3_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT3_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT3_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT3_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT3_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT3_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT3_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT3_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT3_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT3_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_RO_STAT3_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_RO_STAT3_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_RO_STAT3_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_RO_STAT3_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_RO_STAT3_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_AV_STAT0);
        trng_server_csr.wo(utra::trng_server::NIST_AV_STAT0, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT0_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT0_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT0_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT0_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT0_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT0_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT0_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT0_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT0_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT0_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT0_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT0_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT0_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT0_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT0_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::NIST_AV_STAT1);
        trng_server_csr.wo(utra::trng_server::NIST_AV_STAT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT1_ADAP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT1_ADAP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT1_ADAP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT1_ADAP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT1_ADAP_B, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT1_FRESH);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT1_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT1_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT1_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT1_FRESH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::NIST_AV_STAT1_REP_B);
        trng_server_csr.rmwf(utra::trng_server::NIST_AV_STAT1_REP_B, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::NIST_AV_STAT1_REP_B, bar);
        baz |= trng_server_csr.ms(utra::trng_server::NIST_AV_STAT1_REP_B, 1);
        trng_server_csr.wfo(utra::trng_server::NIST_AV_STAT1_REP_B, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUNSLIMIT1);
        trng_server_csr.wo(utra::trng_server::RO_RUNSLIMIT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT1_MIN);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT1_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT1_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT1_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT1_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT1_MAX);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT1_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT1_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT1_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT1_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUNSLIMIT2);
        trng_server_csr.wo(utra::trng_server::RO_RUNSLIMIT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT2_MIN);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT2_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT2_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT2_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT2_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT2_MAX);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT2_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT2_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT2_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT2_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUNSLIMIT3);
        trng_server_csr.wo(utra::trng_server::RO_RUNSLIMIT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT3_MIN);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT3_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT3_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT3_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT3_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT3_MAX);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT3_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT3_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT3_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT3_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUNSLIMIT4);
        trng_server_csr.wo(utra::trng_server::RO_RUNSLIMIT4, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT4_MIN);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT4_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT4_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT4_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT4_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUNSLIMIT4_MAX);
        trng_server_csr.rmwf(utra::trng_server::RO_RUNSLIMIT4_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUNSLIMIT4_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUNSLIMIT4_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUNSLIMIT4_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_CTRL);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_FRESH);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_FRESH, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_COUNT1);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_COUNT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_COUNT1_RO_RUN0_COUNT1);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_COUNT1_RO_RUN0_COUNT1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_COUNT1_RO_RUN0_COUNT1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_COUNT1_RO_RUN0_COUNT1, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_COUNT1_RO_RUN0_COUNT1, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_COUNT2);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_COUNT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_COUNT2_RO_RUN0_COUNT2);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_COUNT2_RO_RUN0_COUNT2, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_COUNT2_RO_RUN0_COUNT2, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_COUNT2_RO_RUN0_COUNT2, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_COUNT2_RO_RUN0_COUNT2, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_COUNT3);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_COUNT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_COUNT3_RO_RUN0_COUNT3);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_COUNT3_RO_RUN0_COUNT3, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_COUNT3_RO_RUN0_COUNT3, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_COUNT3_RO_RUN0_COUNT3, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_COUNT3_RO_RUN0_COUNT3, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN0_COUNT4);
        trng_server_csr.wo(utra::trng_server::RO_RUN0_COUNT4, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN0_COUNT4_RO_RUN0_COUNT4);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN0_COUNT4_RO_RUN0_COUNT4, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN0_COUNT4_RO_RUN0_COUNT4, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN0_COUNT4_RO_RUN0_COUNT4, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN0_COUNT4_RO_RUN0_COUNT4, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_CTRL);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_FRESH);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_FRESH, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_COUNT1);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_COUNT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_COUNT1_RO_RUN1_COUNT1);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_COUNT1_RO_RUN1_COUNT1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_COUNT1_RO_RUN1_COUNT1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_COUNT1_RO_RUN1_COUNT1, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_COUNT1_RO_RUN1_COUNT1, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_COUNT2);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_COUNT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_COUNT2_RO_RUN1_COUNT2);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_COUNT2_RO_RUN1_COUNT2, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_COUNT2_RO_RUN1_COUNT2, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_COUNT2_RO_RUN1_COUNT2, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_COUNT2_RO_RUN1_COUNT2, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_COUNT3);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_COUNT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_COUNT3_RO_RUN1_COUNT3);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_COUNT3_RO_RUN1_COUNT3, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_COUNT3_RO_RUN1_COUNT3, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_COUNT3_RO_RUN1_COUNT3, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_COUNT3_RO_RUN1_COUNT3, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN1_COUNT4);
        trng_server_csr.wo(utra::trng_server::RO_RUN1_COUNT4, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN1_COUNT4_RO_RUN1_COUNT4);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN1_COUNT4_RO_RUN1_COUNT4, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN1_COUNT4_RO_RUN1_COUNT4, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN1_COUNT4_RO_RUN1_COUNT4, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN1_COUNT4_RO_RUN1_COUNT4, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_CTRL);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_FRESH);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_FRESH, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_COUNT1);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_COUNT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_COUNT1_RO_RUN2_COUNT1);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_COUNT1_RO_RUN2_COUNT1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_COUNT1_RO_RUN2_COUNT1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_COUNT1_RO_RUN2_COUNT1, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_COUNT1_RO_RUN2_COUNT1, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_COUNT2);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_COUNT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_COUNT2_RO_RUN2_COUNT2);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_COUNT2_RO_RUN2_COUNT2, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_COUNT2_RO_RUN2_COUNT2, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_COUNT2_RO_RUN2_COUNT2, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_COUNT2_RO_RUN2_COUNT2, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_COUNT3);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_COUNT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_COUNT3_RO_RUN2_COUNT3);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_COUNT3_RO_RUN2_COUNT3, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_COUNT3_RO_RUN2_COUNT3, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_COUNT3_RO_RUN2_COUNT3, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_COUNT3_RO_RUN2_COUNT3, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN2_COUNT4);
        trng_server_csr.wo(utra::trng_server::RO_RUN2_COUNT4, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN2_COUNT4_RO_RUN2_COUNT4);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN2_COUNT4_RO_RUN2_COUNT4, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN2_COUNT4_RO_RUN2_COUNT4, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN2_COUNT4_RO_RUN2_COUNT4, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN2_COUNT4_RO_RUN2_COUNT4, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_CTRL);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_FRESH);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_FRESH, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_COUNT1);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_COUNT1, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_COUNT1_RO_RUN3_COUNT1);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_COUNT1_RO_RUN3_COUNT1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_COUNT1_RO_RUN3_COUNT1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_COUNT1_RO_RUN3_COUNT1, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_COUNT1_RO_RUN3_COUNT1, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_COUNT2);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_COUNT2, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_COUNT2_RO_RUN3_COUNT2);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_COUNT2_RO_RUN3_COUNT2, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_COUNT2_RO_RUN3_COUNT2, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_COUNT2_RO_RUN3_COUNT2, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_COUNT2_RO_RUN3_COUNT2, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_COUNT3);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_COUNT3, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_COUNT3_RO_RUN3_COUNT3);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_COUNT3_RO_RUN3_COUNT3, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_COUNT3_RO_RUN3_COUNT3, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_COUNT3_RO_RUN3_COUNT3, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_COUNT3_RO_RUN3_COUNT3, baz);

        let foo = trng_server_csr.r(utra::trng_server::RO_RUN3_COUNT4);
        trng_server_csr.wo(utra::trng_server::RO_RUN3_COUNT4, foo);
        let bar = trng_server_csr.rf(utra::trng_server::RO_RUN3_COUNT4_RO_RUN3_COUNT4);
        trng_server_csr.rmwf(utra::trng_server::RO_RUN3_COUNT4_RO_RUN3_COUNT4, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::RO_RUN3_COUNT4_RO_RUN3_COUNT4, bar);
        baz |= trng_server_csr.ms(utra::trng_server::RO_RUN3_COUNT4_RO_RUN3_COUNT4, 1);
        trng_server_csr.wfo(utra::trng_server::RO_RUN3_COUNT4_RO_RUN3_COUNT4, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION0_CTRL);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION0_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_CTRL_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_CTRL_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_CTRL_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_CTRL_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_CTRL_CUTOFF, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_CTRL_RESET);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_CTRL_RESET, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_CTRL_RESET, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_CTRL_RESET, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_CTRL_RESET, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION0_STAT);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION0_STAT, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_STAT_MIN);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_STAT_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_STAT_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_STAT_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_STAT_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_STAT_MAX);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_STAT_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_STAT_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_STAT_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_STAT_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION0_LAST_ERR);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION0_LAST_ERR, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION1_CTRL);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION1_CTRL, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_CTRL_CUTOFF);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_CTRL_CUTOFF, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_CTRL_CUTOFF, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_CTRL_CUTOFF, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_CTRL_CUTOFF, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_CTRL_RESET);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_CTRL_RESET, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_CTRL_RESET, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_CTRL_RESET, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_CTRL_RESET, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_CTRL_WINDOW);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_CTRL_WINDOW, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_CTRL_WINDOW, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_CTRL_WINDOW, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_CTRL_WINDOW, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION1_STAT);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION1_STAT, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_STAT_MIN);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_STAT_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_STAT_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_STAT_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_STAT_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_STAT_MAX);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_STAT_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_STAT_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_STAT_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_STAT_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::AV_EXCURSION1_LAST_ERR);
        trng_server_csr.wo(utra::trng_server::AV_EXCURSION1_LAST_ERR, foo);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN, baz);
        let bar = trng_server_csr.rf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX);
        trng_server_csr.rmwf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX, bar);
        baz |= trng_server_csr.ms(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX, 1);
        trng_server_csr.wfo(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX, baz);

        let foo = trng_server_csr.r(utra::trng_server::READY);
        trng_server_csr.wo(utra::trng_server::READY, foo);
        let bar = trng_server_csr.rf(utra::trng_server::READY_AV_EXCURSION);
        trng_server_csr.rmwf(utra::trng_server::READY_AV_EXCURSION, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::READY_AV_EXCURSION, bar);
        baz |= trng_server_csr.ms(utra::trng_server::READY_AV_EXCURSION, 1);
        trng_server_csr.wfo(utra::trng_server::READY_AV_EXCURSION, baz);
        let bar = trng_server_csr.rf(utra::trng_server::READY_AV_ADAPROP);
        trng_server_csr.rmwf(utra::trng_server::READY_AV_ADAPROP, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::READY_AV_ADAPROP, bar);
        baz |= trng_server_csr.ms(utra::trng_server::READY_AV_ADAPROP, 1);
        trng_server_csr.wfo(utra::trng_server::READY_AV_ADAPROP, baz);
        let bar = trng_server_csr.rf(utra::trng_server::READY_RO_ADAPROP);
        trng_server_csr.rmwf(utra::trng_server::READY_RO_ADAPROP, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::READY_RO_ADAPROP, bar);
        baz |= trng_server_csr.ms(utra::trng_server::READY_RO_ADAPROP, 1);
        trng_server_csr.wfo(utra::trng_server::READY_RO_ADAPROP, baz);

        let foo = trng_server_csr.r(utra::trng_server::EV_STATUS);
        trng_server_csr.wo(utra::trng_server::EV_STATUS, foo);
        let bar = trng_server_csr.rf(utra::trng_server::EV_STATUS_AVAIL);
        trng_server_csr.rmwf(utra::trng_server::EV_STATUS_AVAIL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_STATUS_AVAIL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_STATUS_AVAIL, 1);
        trng_server_csr.wfo(utra::trng_server::EV_STATUS_AVAIL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_STATUS_ERROR);
        trng_server_csr.rmwf(utra::trng_server::EV_STATUS_ERROR, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_STATUS_ERROR, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_STATUS_ERROR, 1);
        trng_server_csr.wfo(utra::trng_server::EV_STATUS_ERROR, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_STATUS_HEALTH);
        trng_server_csr.rmwf(utra::trng_server::EV_STATUS_HEALTH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_STATUS_HEALTH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_STATUS_HEALTH, 1);
        trng_server_csr.wfo(utra::trng_server::EV_STATUS_HEALTH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_STATUS_EXCURSION0);
        trng_server_csr.rmwf(utra::trng_server::EV_STATUS_EXCURSION0, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_STATUS_EXCURSION0, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_STATUS_EXCURSION0, 1);
        trng_server_csr.wfo(utra::trng_server::EV_STATUS_EXCURSION0, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_STATUS_EXCURSION1);
        trng_server_csr.rmwf(utra::trng_server::EV_STATUS_EXCURSION1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_STATUS_EXCURSION1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_STATUS_EXCURSION1, 1);
        trng_server_csr.wfo(utra::trng_server::EV_STATUS_EXCURSION1, baz);

        let foo = trng_server_csr.r(utra::trng_server::EV_PENDING);
        trng_server_csr.wo(utra::trng_server::EV_PENDING, foo);
        let bar = trng_server_csr.rf(utra::trng_server::EV_PENDING_AVAIL);
        trng_server_csr.rmwf(utra::trng_server::EV_PENDING_AVAIL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_PENDING_AVAIL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_PENDING_AVAIL, 1);
        trng_server_csr.wfo(utra::trng_server::EV_PENDING_AVAIL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_PENDING_ERROR);
        trng_server_csr.rmwf(utra::trng_server::EV_PENDING_ERROR, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_PENDING_ERROR, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_PENDING_ERROR, 1);
        trng_server_csr.wfo(utra::trng_server::EV_PENDING_ERROR, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_PENDING_HEALTH);
        trng_server_csr.rmwf(utra::trng_server::EV_PENDING_HEALTH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_PENDING_HEALTH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_PENDING_HEALTH, 1);
        trng_server_csr.wfo(utra::trng_server::EV_PENDING_HEALTH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_PENDING_EXCURSION0);
        trng_server_csr.rmwf(utra::trng_server::EV_PENDING_EXCURSION0, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_PENDING_EXCURSION0, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_PENDING_EXCURSION0, 1);
        trng_server_csr.wfo(utra::trng_server::EV_PENDING_EXCURSION0, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_PENDING_EXCURSION1);
        trng_server_csr.rmwf(utra::trng_server::EV_PENDING_EXCURSION1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_PENDING_EXCURSION1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_PENDING_EXCURSION1, 1);
        trng_server_csr.wfo(utra::trng_server::EV_PENDING_EXCURSION1, baz);

        let foo = trng_server_csr.r(utra::trng_server::EV_ENABLE);
        trng_server_csr.wo(utra::trng_server::EV_ENABLE, foo);
        let bar = trng_server_csr.rf(utra::trng_server::EV_ENABLE_AVAIL);
        trng_server_csr.rmwf(utra::trng_server::EV_ENABLE_AVAIL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_ENABLE_AVAIL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_ENABLE_AVAIL, 1);
        trng_server_csr.wfo(utra::trng_server::EV_ENABLE_AVAIL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_ENABLE_ERROR);
        trng_server_csr.rmwf(utra::trng_server::EV_ENABLE_ERROR, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_ENABLE_ERROR, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_ENABLE_ERROR, 1);
        trng_server_csr.wfo(utra::trng_server::EV_ENABLE_ERROR, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_ENABLE_HEALTH);
        trng_server_csr.rmwf(utra::trng_server::EV_ENABLE_HEALTH, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_ENABLE_HEALTH, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_ENABLE_HEALTH, 1);
        trng_server_csr.wfo(utra::trng_server::EV_ENABLE_HEALTH, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_ENABLE_EXCURSION0);
        trng_server_csr.rmwf(utra::trng_server::EV_ENABLE_EXCURSION0, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_ENABLE_EXCURSION0, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_ENABLE_EXCURSION0, 1);
        trng_server_csr.wfo(utra::trng_server::EV_ENABLE_EXCURSION0, baz);
        let bar = trng_server_csr.rf(utra::trng_server::EV_ENABLE_EXCURSION1);
        trng_server_csr.rmwf(utra::trng_server::EV_ENABLE_EXCURSION1, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::EV_ENABLE_EXCURSION1, bar);
        baz |= trng_server_csr.ms(utra::trng_server::EV_ENABLE_EXCURSION1, 1);
        trng_server_csr.wfo(utra::trng_server::EV_ENABLE_EXCURSION1, baz);

        let foo = trng_server_csr.r(utra::trng_server::CHACHA);
        trng_server_csr.wo(utra::trng_server::CHACHA, foo);
        let bar = trng_server_csr.rf(utra::trng_server::CHACHA_RESEED_INTERVAL);
        trng_server_csr.rmwf(utra::trng_server::CHACHA_RESEED_INTERVAL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CHACHA_RESEED_INTERVAL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CHACHA_RESEED_INTERVAL, 1);
        trng_server_csr.wfo(utra::trng_server::CHACHA_RESEED_INTERVAL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CHACHA_SELFMIX_INTERVAL);
        trng_server_csr.rmwf(utra::trng_server::CHACHA_SELFMIX_INTERVAL, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CHACHA_SELFMIX_INTERVAL, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CHACHA_SELFMIX_INTERVAL, 1);
        trng_server_csr.wfo(utra::trng_server::CHACHA_SELFMIX_INTERVAL, baz);
        let bar = trng_server_csr.rf(utra::trng_server::CHACHA_SELFMIX_ENA);
        trng_server_csr.rmwf(utra::trng_server::CHACHA_SELFMIX_ENA, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::CHACHA_SELFMIX_ENA, bar);
        baz |= trng_server_csr.ms(utra::trng_server::CHACHA_SELFMIX_ENA, 1);
        trng_server_csr.wfo(utra::trng_server::CHACHA_SELFMIX_ENA, baz);

        let foo = trng_server_csr.r(utra::trng_server::SEED);
        trng_server_csr.wo(utra::trng_server::SEED, foo);
        let bar = trng_server_csr.rf(utra::trng_server::SEED_SEED);
        trng_server_csr.rmwf(utra::trng_server::SEED_SEED, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::SEED_SEED, bar);
        baz |= trng_server_csr.ms(utra::trng_server::SEED_SEED, 1);
        trng_server_csr.wfo(utra::trng_server::SEED_SEED, baz);

        let foo = trng_server_csr.r(utra::trng_server::URANDOM);
        trng_server_csr.wo(utra::trng_server::URANDOM, foo);
        let bar = trng_server_csr.rf(utra::trng_server::URANDOM_URANDOM);
        trng_server_csr.rmwf(utra::trng_server::URANDOM_URANDOM, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::URANDOM_URANDOM, bar);
        baz |= trng_server_csr.ms(utra::trng_server::URANDOM_URANDOM, 1);
        trng_server_csr.wfo(utra::trng_server::URANDOM_URANDOM, baz);

        let foo = trng_server_csr.r(utra::trng_server::URANDOM_VALID);
        trng_server_csr.wo(utra::trng_server::URANDOM_VALID, foo);
        let bar = trng_server_csr.rf(utra::trng_server::URANDOM_VALID_URANDOM_VALID);
        trng_server_csr.rmwf(utra::trng_server::URANDOM_VALID_URANDOM_VALID, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::URANDOM_VALID_URANDOM_VALID, bar);
        baz |= trng_server_csr.ms(utra::trng_server::URANDOM_VALID_URANDOM_VALID, 1);
        trng_server_csr.wfo(utra::trng_server::URANDOM_VALID_URANDOM_VALID, baz);

        let foo = trng_server_csr.r(utra::trng_server::TEST);
        trng_server_csr.wo(utra::trng_server::TEST, foo);
        let bar = trng_server_csr.rf(utra::trng_server::TEST_SIMULTANEOUS);
        trng_server_csr.rmwf(utra::trng_server::TEST_SIMULTANEOUS, bar);
        let mut baz = trng_server_csr.zf(utra::trng_server::TEST_SIMULTANEOUS, bar);
        baz |= trng_server_csr.ms(utra::trng_server::TEST_SIMULTANEOUS, 1);
        trng_server_csr.wfo(utra::trng_server::TEST_SIMULTANEOUS, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_trng_csr() {
        use super::*;
        let mut trng_csr = CSR::new(HW_TRNG_BASE as *mut u32);

        let foo = trng_csr.r(utra::trng::XADC_TEMPERATURE);
        trng_csr.wo(utra::trng::XADC_TEMPERATURE, foo);
        let bar = trng_csr.rf(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE);
        trng_csr.rmwf(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE, bar);
        baz |= trng_csr.ms(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE, 1);
        trng_csr.wfo(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE, baz);

        let foo = trng_csr.r(utra::trng::XADC_VCCINT);
        trng_csr.wo(utra::trng::XADC_VCCINT, foo);
        let bar = trng_csr.rf(utra::trng::XADC_VCCINT_XADC_VCCINT);
        trng_csr.rmwf(utra::trng::XADC_VCCINT_XADC_VCCINT, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_VCCINT_XADC_VCCINT, bar);
        baz |= trng_csr.ms(utra::trng::XADC_VCCINT_XADC_VCCINT, 1);
        trng_csr.wfo(utra::trng::XADC_VCCINT_XADC_VCCINT, baz);

        let foo = trng_csr.r(utra::trng::XADC_VCCAUX);
        trng_csr.wo(utra::trng::XADC_VCCAUX, foo);
        let bar = trng_csr.rf(utra::trng::XADC_VCCAUX_XADC_VCCAUX);
        trng_csr.rmwf(utra::trng::XADC_VCCAUX_XADC_VCCAUX, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_VCCAUX_XADC_VCCAUX, bar);
        baz |= trng_csr.ms(utra::trng::XADC_VCCAUX_XADC_VCCAUX, 1);
        trng_csr.wfo(utra::trng::XADC_VCCAUX_XADC_VCCAUX, baz);

        let foo = trng_csr.r(utra::trng::XADC_VCCBRAM);
        trng_csr.wo(utra::trng::XADC_VCCBRAM, foo);
        let bar = trng_csr.rf(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM);
        trng_csr.rmwf(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM, bar);
        baz |= trng_csr.ms(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM, 1);
        trng_csr.wfo(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM, baz);

        let foo = trng_csr.r(utra::trng::XADC_VBUS);
        trng_csr.wo(utra::trng::XADC_VBUS, foo);
        let bar = trng_csr.rf(utra::trng::XADC_VBUS_XADC_VBUS);
        trng_csr.rmwf(utra::trng::XADC_VBUS_XADC_VBUS, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_VBUS_XADC_VBUS, bar);
        baz |= trng_csr.ms(utra::trng::XADC_VBUS_XADC_VBUS, 1);
        trng_csr.wfo(utra::trng::XADC_VBUS_XADC_VBUS, baz);

        let foo = trng_csr.r(utra::trng::XADC_USB_P);
        trng_csr.wo(utra::trng::XADC_USB_P, foo);
        let bar = trng_csr.rf(utra::trng::XADC_USB_P_XADC_USB_P);
        trng_csr.rmwf(utra::trng::XADC_USB_P_XADC_USB_P, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_USB_P_XADC_USB_P, bar);
        baz |= trng_csr.ms(utra::trng::XADC_USB_P_XADC_USB_P, 1);
        trng_csr.wfo(utra::trng::XADC_USB_P_XADC_USB_P, baz);

        let foo = trng_csr.r(utra::trng::XADC_USB_N);
        trng_csr.wo(utra::trng::XADC_USB_N, foo);
        let bar = trng_csr.rf(utra::trng::XADC_USB_N_XADC_USB_N);
        trng_csr.rmwf(utra::trng::XADC_USB_N_XADC_USB_N, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_USB_N_XADC_USB_N, bar);
        baz |= trng_csr.ms(utra::trng::XADC_USB_N_XADC_USB_N, 1);
        trng_csr.wfo(utra::trng::XADC_USB_N_XADC_USB_N, baz);

        let foo = trng_csr.r(utra::trng::XADC_NOISE0);
        trng_csr.wo(utra::trng::XADC_NOISE0, foo);
        let bar = trng_csr.rf(utra::trng::XADC_NOISE0_XADC_NOISE0);
        trng_csr.rmwf(utra::trng::XADC_NOISE0_XADC_NOISE0, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_NOISE0_XADC_NOISE0, bar);
        baz |= trng_csr.ms(utra::trng::XADC_NOISE0_XADC_NOISE0, 1);
        trng_csr.wfo(utra::trng::XADC_NOISE0_XADC_NOISE0, baz);

        let foo = trng_csr.r(utra::trng::XADC_NOISE1);
        trng_csr.wo(utra::trng::XADC_NOISE1, foo);
        let bar = trng_csr.rf(utra::trng::XADC_NOISE1_XADC_NOISE1);
        trng_csr.rmwf(utra::trng::XADC_NOISE1_XADC_NOISE1, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_NOISE1_XADC_NOISE1, bar);
        baz |= trng_csr.ms(utra::trng::XADC_NOISE1_XADC_NOISE1, 1);
        trng_csr.wfo(utra::trng::XADC_NOISE1_XADC_NOISE1, baz);

        let foo = trng_csr.r(utra::trng::XADC_EOC);
        trng_csr.wo(utra::trng::XADC_EOC, foo);
        let bar = trng_csr.rf(utra::trng::XADC_EOC_XADC_EOC);
        trng_csr.rmwf(utra::trng::XADC_EOC_XADC_EOC, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_EOC_XADC_EOC, bar);
        baz |= trng_csr.ms(utra::trng::XADC_EOC_XADC_EOC, 1);
        trng_csr.wfo(utra::trng::XADC_EOC_XADC_EOC, baz);

        let foo = trng_csr.r(utra::trng::XADC_EOS);
        trng_csr.wo(utra::trng::XADC_EOS, foo);
        let bar = trng_csr.rf(utra::trng::XADC_EOS_XADC_EOS);
        trng_csr.rmwf(utra::trng::XADC_EOS_XADC_EOS, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_EOS_XADC_EOS, bar);
        baz |= trng_csr.ms(utra::trng::XADC_EOS_XADC_EOS, 1);
        trng_csr.wfo(utra::trng::XADC_EOS_XADC_EOS, baz);

        let foo = trng_csr.r(utra::trng::XADC_GPIO5);
        trng_csr.wo(utra::trng::XADC_GPIO5, foo);
        let bar = trng_csr.rf(utra::trng::XADC_GPIO5_XADC_GPIO5);
        trng_csr.rmwf(utra::trng::XADC_GPIO5_XADC_GPIO5, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_GPIO5_XADC_GPIO5, bar);
        baz |= trng_csr.ms(utra::trng::XADC_GPIO5_XADC_GPIO5, 1);
        trng_csr.wfo(utra::trng::XADC_GPIO5_XADC_GPIO5, baz);

        let foo = trng_csr.r(utra::trng::XADC_GPIO2);
        trng_csr.wo(utra::trng::XADC_GPIO2, foo);
        let bar = trng_csr.rf(utra::trng::XADC_GPIO2_XADC_GPIO2);
        trng_csr.rmwf(utra::trng::XADC_GPIO2_XADC_GPIO2, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_GPIO2_XADC_GPIO2, bar);
        baz |= trng_csr.ms(utra::trng::XADC_GPIO2_XADC_GPIO2, 1);
        trng_csr.wfo(utra::trng::XADC_GPIO2_XADC_GPIO2, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_ENABLE);
        trng_csr.wo(utra::trng::XADC_DRP_ENABLE, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_ENABLE_XADC_DRP_ENABLE);
        trng_csr.rmwf(utra::trng::XADC_DRP_ENABLE_XADC_DRP_ENABLE, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_ENABLE_XADC_DRP_ENABLE, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_ENABLE_XADC_DRP_ENABLE, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_ENABLE_XADC_DRP_ENABLE, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_READ);
        trng_csr.wo(utra::trng::XADC_DRP_READ, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_READ_XADC_DRP_READ);
        trng_csr.rmwf(utra::trng::XADC_DRP_READ_XADC_DRP_READ, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_READ_XADC_DRP_READ, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_READ_XADC_DRP_READ, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_READ_XADC_DRP_READ, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_WRITE);
        trng_csr.wo(utra::trng::XADC_DRP_WRITE, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_WRITE_XADC_DRP_WRITE);
        trng_csr.rmwf(utra::trng::XADC_DRP_WRITE_XADC_DRP_WRITE, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_WRITE_XADC_DRP_WRITE, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_WRITE_XADC_DRP_WRITE, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_WRITE_XADC_DRP_WRITE, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_DRDY);
        trng_csr.wo(utra::trng::XADC_DRP_DRDY, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_DRDY_XADC_DRP_DRDY);
        trng_csr.rmwf(utra::trng::XADC_DRP_DRDY_XADC_DRP_DRDY, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_DRDY_XADC_DRP_DRDY, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_DRDY_XADC_DRP_DRDY, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_DRDY_XADC_DRP_DRDY, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_ADR);
        trng_csr.wo(utra::trng::XADC_DRP_ADR, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_ADR_XADC_DRP_ADR);
        trng_csr.rmwf(utra::trng::XADC_DRP_ADR_XADC_DRP_ADR, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_ADR_XADC_DRP_ADR, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_ADR_XADC_DRP_ADR, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_ADR_XADC_DRP_ADR, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_DAT_W);
        trng_csr.wo(utra::trng::XADC_DRP_DAT_W, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_DAT_W_XADC_DRP_DAT_W);
        trng_csr.rmwf(utra::trng::XADC_DRP_DAT_W_XADC_DRP_DAT_W, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_DAT_W_XADC_DRP_DAT_W, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_DAT_W_XADC_DRP_DAT_W, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_DAT_W_XADC_DRP_DAT_W, baz);

        let foo = trng_csr.r(utra::trng::XADC_DRP_DAT_R);
        trng_csr.wo(utra::trng::XADC_DRP_DAT_R, foo);
        let bar = trng_csr.rf(utra::trng::XADC_DRP_DAT_R_XADC_DRP_DAT_R);
        trng_csr.rmwf(utra::trng::XADC_DRP_DAT_R_XADC_DRP_DAT_R, bar);
        let mut baz = trng_csr.zf(utra::trng::XADC_DRP_DAT_R_XADC_DRP_DAT_R, bar);
        baz |= trng_csr.ms(utra::trng::XADC_DRP_DAT_R_XADC_DRP_DAT_R, 1);
        trng_csr.wfo(utra::trng::XADC_DRP_DAT_R_XADC_DRP_DAT_R, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_sha512_csr() {
        use super::*;
        let mut sha512_csr = CSR::new(HW_SHA512_BASE as *mut u32);

        let foo = sha512_csr.r(utra::sha512::POWER);
        sha512_csr.wo(utra::sha512::POWER, foo);
        let bar = sha512_csr.rf(utra::sha512::POWER_ON);
        sha512_csr.rmwf(utra::sha512::POWER_ON, bar);
        let mut baz = sha512_csr.zf(utra::sha512::POWER_ON, bar);
        baz |= sha512_csr.ms(utra::sha512::POWER_ON, 1);
        sha512_csr.wfo(utra::sha512::POWER_ON, baz);

        let foo = sha512_csr.r(utra::sha512::CONFIG);
        sha512_csr.wo(utra::sha512::CONFIG, foo);
        let bar = sha512_csr.rf(utra::sha512::CONFIG_SHA_EN);
        sha512_csr.rmwf(utra::sha512::CONFIG_SHA_EN, bar);
        let mut baz = sha512_csr.zf(utra::sha512::CONFIG_SHA_EN, bar);
        baz |= sha512_csr.ms(utra::sha512::CONFIG_SHA_EN, 1);
        sha512_csr.wfo(utra::sha512::CONFIG_SHA_EN, baz);
        let bar = sha512_csr.rf(utra::sha512::CONFIG_ENDIAN_SWAP);
        sha512_csr.rmwf(utra::sha512::CONFIG_ENDIAN_SWAP, bar);
        let mut baz = sha512_csr.zf(utra::sha512::CONFIG_ENDIAN_SWAP, bar);
        baz |= sha512_csr.ms(utra::sha512::CONFIG_ENDIAN_SWAP, 1);
        sha512_csr.wfo(utra::sha512::CONFIG_ENDIAN_SWAP, baz);
        let bar = sha512_csr.rf(utra::sha512::CONFIG_DIGEST_SWAP);
        sha512_csr.rmwf(utra::sha512::CONFIG_DIGEST_SWAP, bar);
        let mut baz = sha512_csr.zf(utra::sha512::CONFIG_DIGEST_SWAP, bar);
        baz |= sha512_csr.ms(utra::sha512::CONFIG_DIGEST_SWAP, 1);
        sha512_csr.wfo(utra::sha512::CONFIG_DIGEST_SWAP, baz);
        let bar = sha512_csr.rf(utra::sha512::CONFIG_SELECT_256);
        sha512_csr.rmwf(utra::sha512::CONFIG_SELECT_256, bar);
        let mut baz = sha512_csr.zf(utra::sha512::CONFIG_SELECT_256, bar);
        baz |= sha512_csr.ms(utra::sha512::CONFIG_SELECT_256, 1);
        sha512_csr.wfo(utra::sha512::CONFIG_SELECT_256, baz);
        let bar = sha512_csr.rf(utra::sha512::CONFIG_RESET);
        sha512_csr.rmwf(utra::sha512::CONFIG_RESET, bar);
        let mut baz = sha512_csr.zf(utra::sha512::CONFIG_RESET, bar);
        baz |= sha512_csr.ms(utra::sha512::CONFIG_RESET, 1);
        sha512_csr.wfo(utra::sha512::CONFIG_RESET, baz);

        let foo = sha512_csr.r(utra::sha512::COMMAND);
        sha512_csr.wo(utra::sha512::COMMAND, foo);
        let bar = sha512_csr.rf(utra::sha512::COMMAND_HASH_START);
        sha512_csr.rmwf(utra::sha512::COMMAND_HASH_START, bar);
        let mut baz = sha512_csr.zf(utra::sha512::COMMAND_HASH_START, bar);
        baz |= sha512_csr.ms(utra::sha512::COMMAND_HASH_START, 1);
        sha512_csr.wfo(utra::sha512::COMMAND_HASH_START, baz);
        let bar = sha512_csr.rf(utra::sha512::COMMAND_HASH_PROCESS);
        sha512_csr.rmwf(utra::sha512::COMMAND_HASH_PROCESS, bar);
        let mut baz = sha512_csr.zf(utra::sha512::COMMAND_HASH_PROCESS, bar);
        baz |= sha512_csr.ms(utra::sha512::COMMAND_HASH_PROCESS, 1);
        sha512_csr.wfo(utra::sha512::COMMAND_HASH_PROCESS, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST01);
        sha512_csr.wo(utra::sha512::DIGEST01, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST01_DIGEST0);
        sha512_csr.rmwf(utra::sha512::DIGEST01_DIGEST0, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST01_DIGEST0, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST01_DIGEST0, 1);
        sha512_csr.wfo(utra::sha512::DIGEST01_DIGEST0, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST00);
        sha512_csr.wo(utra::sha512::DIGEST00, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST00_DIGEST0);
        sha512_csr.rmwf(utra::sha512::DIGEST00_DIGEST0, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST00_DIGEST0, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST00_DIGEST0, 1);
        sha512_csr.wfo(utra::sha512::DIGEST00_DIGEST0, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST11);
        sha512_csr.wo(utra::sha512::DIGEST11, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST11_DIGEST1);
        sha512_csr.rmwf(utra::sha512::DIGEST11_DIGEST1, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST11_DIGEST1, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST11_DIGEST1, 1);
        sha512_csr.wfo(utra::sha512::DIGEST11_DIGEST1, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST10);
        sha512_csr.wo(utra::sha512::DIGEST10, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST10_DIGEST1);
        sha512_csr.rmwf(utra::sha512::DIGEST10_DIGEST1, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST10_DIGEST1, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST10_DIGEST1, 1);
        sha512_csr.wfo(utra::sha512::DIGEST10_DIGEST1, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST21);
        sha512_csr.wo(utra::sha512::DIGEST21, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST21_DIGEST2);
        sha512_csr.rmwf(utra::sha512::DIGEST21_DIGEST2, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST21_DIGEST2, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST21_DIGEST2, 1);
        sha512_csr.wfo(utra::sha512::DIGEST21_DIGEST2, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST20);
        sha512_csr.wo(utra::sha512::DIGEST20, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST20_DIGEST2);
        sha512_csr.rmwf(utra::sha512::DIGEST20_DIGEST2, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST20_DIGEST2, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST20_DIGEST2, 1);
        sha512_csr.wfo(utra::sha512::DIGEST20_DIGEST2, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST31);
        sha512_csr.wo(utra::sha512::DIGEST31, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST31_DIGEST3);
        sha512_csr.rmwf(utra::sha512::DIGEST31_DIGEST3, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST31_DIGEST3, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST31_DIGEST3, 1);
        sha512_csr.wfo(utra::sha512::DIGEST31_DIGEST3, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST30);
        sha512_csr.wo(utra::sha512::DIGEST30, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST30_DIGEST3);
        sha512_csr.rmwf(utra::sha512::DIGEST30_DIGEST3, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST30_DIGEST3, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST30_DIGEST3, 1);
        sha512_csr.wfo(utra::sha512::DIGEST30_DIGEST3, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST41);
        sha512_csr.wo(utra::sha512::DIGEST41, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST41_DIGEST4);
        sha512_csr.rmwf(utra::sha512::DIGEST41_DIGEST4, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST41_DIGEST4, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST41_DIGEST4, 1);
        sha512_csr.wfo(utra::sha512::DIGEST41_DIGEST4, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST40);
        sha512_csr.wo(utra::sha512::DIGEST40, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST40_DIGEST4);
        sha512_csr.rmwf(utra::sha512::DIGEST40_DIGEST4, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST40_DIGEST4, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST40_DIGEST4, 1);
        sha512_csr.wfo(utra::sha512::DIGEST40_DIGEST4, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST51);
        sha512_csr.wo(utra::sha512::DIGEST51, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST51_DIGEST5);
        sha512_csr.rmwf(utra::sha512::DIGEST51_DIGEST5, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST51_DIGEST5, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST51_DIGEST5, 1);
        sha512_csr.wfo(utra::sha512::DIGEST51_DIGEST5, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST50);
        sha512_csr.wo(utra::sha512::DIGEST50, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST50_DIGEST5);
        sha512_csr.rmwf(utra::sha512::DIGEST50_DIGEST5, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST50_DIGEST5, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST50_DIGEST5, 1);
        sha512_csr.wfo(utra::sha512::DIGEST50_DIGEST5, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST61);
        sha512_csr.wo(utra::sha512::DIGEST61, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST61_DIGEST6);
        sha512_csr.rmwf(utra::sha512::DIGEST61_DIGEST6, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST61_DIGEST6, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST61_DIGEST6, 1);
        sha512_csr.wfo(utra::sha512::DIGEST61_DIGEST6, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST60);
        sha512_csr.wo(utra::sha512::DIGEST60, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST60_DIGEST6);
        sha512_csr.rmwf(utra::sha512::DIGEST60_DIGEST6, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST60_DIGEST6, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST60_DIGEST6, 1);
        sha512_csr.wfo(utra::sha512::DIGEST60_DIGEST6, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST71);
        sha512_csr.wo(utra::sha512::DIGEST71, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST71_DIGEST7);
        sha512_csr.rmwf(utra::sha512::DIGEST71_DIGEST7, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST71_DIGEST7, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST71_DIGEST7, 1);
        sha512_csr.wfo(utra::sha512::DIGEST71_DIGEST7, baz);

        let foo = sha512_csr.r(utra::sha512::DIGEST70);
        sha512_csr.wo(utra::sha512::DIGEST70, foo);
        let bar = sha512_csr.rf(utra::sha512::DIGEST70_DIGEST7);
        sha512_csr.rmwf(utra::sha512::DIGEST70_DIGEST7, bar);
        let mut baz = sha512_csr.zf(utra::sha512::DIGEST70_DIGEST7, bar);
        baz |= sha512_csr.ms(utra::sha512::DIGEST70_DIGEST7, 1);
        sha512_csr.wfo(utra::sha512::DIGEST70_DIGEST7, baz);

        let foo = sha512_csr.r(utra::sha512::MSG_LENGTH1);
        sha512_csr.wo(utra::sha512::MSG_LENGTH1, foo);
        let bar = sha512_csr.rf(utra::sha512::MSG_LENGTH1_MSG_LENGTH);
        sha512_csr.rmwf(utra::sha512::MSG_LENGTH1_MSG_LENGTH, bar);
        let mut baz = sha512_csr.zf(utra::sha512::MSG_LENGTH1_MSG_LENGTH, bar);
        baz |= sha512_csr.ms(utra::sha512::MSG_LENGTH1_MSG_LENGTH, 1);
        sha512_csr.wfo(utra::sha512::MSG_LENGTH1_MSG_LENGTH, baz);

        let foo = sha512_csr.r(utra::sha512::MSG_LENGTH0);
        sha512_csr.wo(utra::sha512::MSG_LENGTH0, foo);
        let bar = sha512_csr.rf(utra::sha512::MSG_LENGTH0_MSG_LENGTH);
        sha512_csr.rmwf(utra::sha512::MSG_LENGTH0_MSG_LENGTH, bar);
        let mut baz = sha512_csr.zf(utra::sha512::MSG_LENGTH0_MSG_LENGTH, bar);
        baz |= sha512_csr.ms(utra::sha512::MSG_LENGTH0_MSG_LENGTH, 1);
        sha512_csr.wfo(utra::sha512::MSG_LENGTH0_MSG_LENGTH, baz);

        let foo = sha512_csr.r(utra::sha512::EV_STATUS);
        sha512_csr.wo(utra::sha512::EV_STATUS, foo);
        let bar = sha512_csr.rf(utra::sha512::EV_STATUS_ERR_VALID);
        sha512_csr.rmwf(utra::sha512::EV_STATUS_ERR_VALID, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_STATUS_ERR_VALID, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_STATUS_ERR_VALID, 1);
        sha512_csr.wfo(utra::sha512::EV_STATUS_ERR_VALID, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_STATUS_FIFO_FULL);
        sha512_csr.rmwf(utra::sha512::EV_STATUS_FIFO_FULL, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_STATUS_FIFO_FULL, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_STATUS_FIFO_FULL, 1);
        sha512_csr.wfo(utra::sha512::EV_STATUS_FIFO_FULL, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_STATUS_SHA512_DONE);
        sha512_csr.rmwf(utra::sha512::EV_STATUS_SHA512_DONE, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_STATUS_SHA512_DONE, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_STATUS_SHA512_DONE, 1);
        sha512_csr.wfo(utra::sha512::EV_STATUS_SHA512_DONE, baz);

        let foo = sha512_csr.r(utra::sha512::EV_PENDING);
        sha512_csr.wo(utra::sha512::EV_PENDING, foo);
        let bar = sha512_csr.rf(utra::sha512::EV_PENDING_ERR_VALID);
        sha512_csr.rmwf(utra::sha512::EV_PENDING_ERR_VALID, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_PENDING_ERR_VALID, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_PENDING_ERR_VALID, 1);
        sha512_csr.wfo(utra::sha512::EV_PENDING_ERR_VALID, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_PENDING_FIFO_FULL);
        sha512_csr.rmwf(utra::sha512::EV_PENDING_FIFO_FULL, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_PENDING_FIFO_FULL, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_PENDING_FIFO_FULL, 1);
        sha512_csr.wfo(utra::sha512::EV_PENDING_FIFO_FULL, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_PENDING_SHA512_DONE);
        sha512_csr.rmwf(utra::sha512::EV_PENDING_SHA512_DONE, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_PENDING_SHA512_DONE, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_PENDING_SHA512_DONE, 1);
        sha512_csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, baz);

        let foo = sha512_csr.r(utra::sha512::EV_ENABLE);
        sha512_csr.wo(utra::sha512::EV_ENABLE, foo);
        let bar = sha512_csr.rf(utra::sha512::EV_ENABLE_ERR_VALID);
        sha512_csr.rmwf(utra::sha512::EV_ENABLE_ERR_VALID, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_ENABLE_ERR_VALID, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_ENABLE_ERR_VALID, 1);
        sha512_csr.wfo(utra::sha512::EV_ENABLE_ERR_VALID, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_ENABLE_FIFO_FULL);
        sha512_csr.rmwf(utra::sha512::EV_ENABLE_FIFO_FULL, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_ENABLE_FIFO_FULL, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_ENABLE_FIFO_FULL, 1);
        sha512_csr.wfo(utra::sha512::EV_ENABLE_FIFO_FULL, baz);
        let bar = sha512_csr.rf(utra::sha512::EV_ENABLE_SHA512_DONE);
        sha512_csr.rmwf(utra::sha512::EV_ENABLE_SHA512_DONE, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_ENABLE_SHA512_DONE, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_ENABLE_SHA512_DONE, 1);
        sha512_csr.wfo(utra::sha512::EV_ENABLE_SHA512_DONE, baz);

        let foo = sha512_csr.r(utra::sha512::FIFO);
        sha512_csr.wo(utra::sha512::FIFO, foo);
        let bar = sha512_csr.rf(utra::sha512::FIFO_RESET_STATUS);
        sha512_csr.rmwf(utra::sha512::FIFO_RESET_STATUS, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_RESET_STATUS, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_RESET_STATUS, 1);
        sha512_csr.wfo(utra::sha512::FIFO_RESET_STATUS, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_READ_COUNT);
        sha512_csr.rmwf(utra::sha512::FIFO_READ_COUNT, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_READ_COUNT, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_READ_COUNT, 1);
        sha512_csr.wfo(utra::sha512::FIFO_READ_COUNT, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_WRITE_COUNT);
        sha512_csr.rmwf(utra::sha512::FIFO_WRITE_COUNT, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_WRITE_COUNT, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_WRITE_COUNT, 1);
        sha512_csr.wfo(utra::sha512::FIFO_WRITE_COUNT, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_READ_ERROR);
        sha512_csr.rmwf(utra::sha512::FIFO_READ_ERROR, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_READ_ERROR, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_READ_ERROR, 1);
        sha512_csr.wfo(utra::sha512::FIFO_READ_ERROR, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_WRITE_ERROR);
        sha512_csr.rmwf(utra::sha512::FIFO_WRITE_ERROR, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_WRITE_ERROR, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_WRITE_ERROR, 1);
        sha512_csr.wfo(utra::sha512::FIFO_WRITE_ERROR, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_ALMOST_FULL);
        sha512_csr.rmwf(utra::sha512::FIFO_ALMOST_FULL, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_ALMOST_FULL, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_ALMOST_FULL, 1);
        sha512_csr.wfo(utra::sha512::FIFO_ALMOST_FULL, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_ALMOST_EMPTY);
        sha512_csr.rmwf(utra::sha512::FIFO_ALMOST_EMPTY, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_ALMOST_EMPTY, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_ALMOST_EMPTY, 1);
        sha512_csr.wfo(utra::sha512::FIFO_ALMOST_EMPTY, baz);
        let bar = sha512_csr.rf(utra::sha512::FIFO_RUNNING);
        sha512_csr.rmwf(utra::sha512::FIFO_RUNNING, bar);
        let mut baz = sha512_csr.zf(utra::sha512::FIFO_RUNNING, bar);
        baz |= sha512_csr.ms(utra::sha512::FIFO_RUNNING, 1);
        sha512_csr.wfo(utra::sha512::FIFO_RUNNING, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_engine_csr() {
        use super::*;
        let mut engine_csr = CSR::new(HW_ENGINE_BASE as *mut u32);

        let foo = engine_csr.r(utra::engine::WINDOW);
        engine_csr.wo(utra::engine::WINDOW, foo);
        let bar = engine_csr.rf(utra::engine::WINDOW_WINDOW);
        engine_csr.rmwf(utra::engine::WINDOW_WINDOW, bar);
        let mut baz = engine_csr.zf(utra::engine::WINDOW_WINDOW, bar);
        baz |= engine_csr.ms(utra::engine::WINDOW_WINDOW, 1);
        engine_csr.wfo(utra::engine::WINDOW_WINDOW, baz);

        let foo = engine_csr.r(utra::engine::MPSTART);
        engine_csr.wo(utra::engine::MPSTART, foo);
        let bar = engine_csr.rf(utra::engine::MPSTART_MPSTART);
        engine_csr.rmwf(utra::engine::MPSTART_MPSTART, bar);
        let mut baz = engine_csr.zf(utra::engine::MPSTART_MPSTART, bar);
        baz |= engine_csr.ms(utra::engine::MPSTART_MPSTART, 1);
        engine_csr.wfo(utra::engine::MPSTART_MPSTART, baz);

        let foo = engine_csr.r(utra::engine::MPLEN);
        engine_csr.wo(utra::engine::MPLEN, foo);
        let bar = engine_csr.rf(utra::engine::MPLEN_MPLEN);
        engine_csr.rmwf(utra::engine::MPLEN_MPLEN, bar);
        let mut baz = engine_csr.zf(utra::engine::MPLEN_MPLEN, bar);
        baz |= engine_csr.ms(utra::engine::MPLEN_MPLEN, 1);
        engine_csr.wfo(utra::engine::MPLEN_MPLEN, baz);

        let foo = engine_csr.r(utra::engine::CONTROL);
        engine_csr.wo(utra::engine::CONTROL, foo);
        let bar = engine_csr.rf(utra::engine::CONTROL_GO);
        engine_csr.rmwf(utra::engine::CONTROL_GO, bar);
        let mut baz = engine_csr.zf(utra::engine::CONTROL_GO, bar);
        baz |= engine_csr.ms(utra::engine::CONTROL_GO, 1);
        engine_csr.wfo(utra::engine::CONTROL_GO, baz);

        let foo = engine_csr.r(utra::engine::MPRESUME);
        engine_csr.wo(utra::engine::MPRESUME, foo);
        let bar = engine_csr.rf(utra::engine::MPRESUME_MPRESUME);
        engine_csr.rmwf(utra::engine::MPRESUME_MPRESUME, bar);
        let mut baz = engine_csr.zf(utra::engine::MPRESUME_MPRESUME, bar);
        baz |= engine_csr.ms(utra::engine::MPRESUME_MPRESUME, 1);
        engine_csr.wfo(utra::engine::MPRESUME_MPRESUME, baz);

        let foo = engine_csr.r(utra::engine::POWER);
        engine_csr.wo(utra::engine::POWER, foo);
        let bar = engine_csr.rf(utra::engine::POWER_ON);
        engine_csr.rmwf(utra::engine::POWER_ON, bar);
        let mut baz = engine_csr.zf(utra::engine::POWER_ON, bar);
        baz |= engine_csr.ms(utra::engine::POWER_ON, 1);
        engine_csr.wfo(utra::engine::POWER_ON, baz);
        let bar = engine_csr.rf(utra::engine::POWER_PAUSE_REQ);
        engine_csr.rmwf(utra::engine::POWER_PAUSE_REQ, bar);
        let mut baz = engine_csr.zf(utra::engine::POWER_PAUSE_REQ, bar);
        baz |= engine_csr.ms(utra::engine::POWER_PAUSE_REQ, 1);
        engine_csr.wfo(utra::engine::POWER_PAUSE_REQ, baz);

        let foo = engine_csr.r(utra::engine::STATUS);
        engine_csr.wo(utra::engine::STATUS, foo);
        let bar = engine_csr.rf(utra::engine::STATUS_RUNNING);
        engine_csr.rmwf(utra::engine::STATUS_RUNNING, bar);
        let mut baz = engine_csr.zf(utra::engine::STATUS_RUNNING, bar);
        baz |= engine_csr.ms(utra::engine::STATUS_RUNNING, 1);
        engine_csr.wfo(utra::engine::STATUS_RUNNING, baz);
        let bar = engine_csr.rf(utra::engine::STATUS_MPC);
        engine_csr.rmwf(utra::engine::STATUS_MPC, bar);
        let mut baz = engine_csr.zf(utra::engine::STATUS_MPC, bar);
        baz |= engine_csr.ms(utra::engine::STATUS_MPC, 1);
        engine_csr.wfo(utra::engine::STATUS_MPC, baz);
        let bar = engine_csr.rf(utra::engine::STATUS_PAUSE_GNT);
        engine_csr.rmwf(utra::engine::STATUS_PAUSE_GNT, bar);
        let mut baz = engine_csr.zf(utra::engine::STATUS_PAUSE_GNT, bar);
        baz |= engine_csr.ms(utra::engine::STATUS_PAUSE_GNT, 1);
        engine_csr.wfo(utra::engine::STATUS_PAUSE_GNT, baz);

        let foo = engine_csr.r(utra::engine::EV_STATUS);
        engine_csr.wo(utra::engine::EV_STATUS, foo);
        let bar = engine_csr.rf(utra::engine::EV_STATUS_FINISHED);
        engine_csr.rmwf(utra::engine::EV_STATUS_FINISHED, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_STATUS_FINISHED, bar);
        baz |= engine_csr.ms(utra::engine::EV_STATUS_FINISHED, 1);
        engine_csr.wfo(utra::engine::EV_STATUS_FINISHED, baz);
        let bar = engine_csr.rf(utra::engine::EV_STATUS_ILLEGAL_OPCODE);
        engine_csr.rmwf(utra::engine::EV_STATUS_ILLEGAL_OPCODE, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_STATUS_ILLEGAL_OPCODE, bar);
        baz |= engine_csr.ms(utra::engine::EV_STATUS_ILLEGAL_OPCODE, 1);
        engine_csr.wfo(utra::engine::EV_STATUS_ILLEGAL_OPCODE, baz);

        let foo = engine_csr.r(utra::engine::EV_PENDING);
        engine_csr.wo(utra::engine::EV_PENDING, foo);
        let bar = engine_csr.rf(utra::engine::EV_PENDING_FINISHED);
        engine_csr.rmwf(utra::engine::EV_PENDING_FINISHED, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_PENDING_FINISHED, bar);
        baz |= engine_csr.ms(utra::engine::EV_PENDING_FINISHED, 1);
        engine_csr.wfo(utra::engine::EV_PENDING_FINISHED, baz);
        let bar = engine_csr.rf(utra::engine::EV_PENDING_ILLEGAL_OPCODE);
        engine_csr.rmwf(utra::engine::EV_PENDING_ILLEGAL_OPCODE, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_PENDING_ILLEGAL_OPCODE, bar);
        baz |= engine_csr.ms(utra::engine::EV_PENDING_ILLEGAL_OPCODE, 1);
        engine_csr.wfo(utra::engine::EV_PENDING_ILLEGAL_OPCODE, baz);

        let foo = engine_csr.r(utra::engine::EV_ENABLE);
        engine_csr.wo(utra::engine::EV_ENABLE, foo);
        let bar = engine_csr.rf(utra::engine::EV_ENABLE_FINISHED);
        engine_csr.rmwf(utra::engine::EV_ENABLE_FINISHED, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_ENABLE_FINISHED, bar);
        baz |= engine_csr.ms(utra::engine::EV_ENABLE_FINISHED, 1);
        engine_csr.wfo(utra::engine::EV_ENABLE_FINISHED, baz);
        let bar = engine_csr.rf(utra::engine::EV_ENABLE_ILLEGAL_OPCODE);
        engine_csr.rmwf(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, bar);
        baz |= engine_csr.ms(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, 1);
        engine_csr.wfo(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, baz);

        let foo = engine_csr.r(utra::engine::INSTRUCTION);
        engine_csr.wo(utra::engine::INSTRUCTION, foo);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_OPCODE);
        engine_csr.rmwf(utra::engine::INSTRUCTION_OPCODE, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_OPCODE, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_OPCODE, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_OPCODE, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_RA);
        engine_csr.rmwf(utra::engine::INSTRUCTION_RA, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_RA, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_RA, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_RA, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_CA);
        engine_csr.rmwf(utra::engine::INSTRUCTION_CA, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_CA, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_CA, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_CA, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_RB);
        engine_csr.rmwf(utra::engine::INSTRUCTION_RB, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_RB, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_RB, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_RB, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_CB);
        engine_csr.rmwf(utra::engine::INSTRUCTION_CB, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_CB, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_CB, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_CB, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_WD);
        engine_csr.rmwf(utra::engine::INSTRUCTION_WD, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_WD, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_WD, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_WD, baz);
        let bar = engine_csr.rf(utra::engine::INSTRUCTION_IMMEDIATE);
        engine_csr.rmwf(utra::engine::INSTRUCTION_IMMEDIATE, bar);
        let mut baz = engine_csr.zf(utra::engine::INSTRUCTION_IMMEDIATE, bar);
        baz |= engine_csr.ms(utra::engine::INSTRUCTION_IMMEDIATE, 1);
        engine_csr.wfo(utra::engine::INSTRUCTION_IMMEDIATE, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_jtag_csr() {
        use super::*;
        let mut jtag_csr = CSR::new(HW_JTAG_BASE as *mut u32);

        let foo = jtag_csr.r(utra::jtag::NEXT);
        jtag_csr.wo(utra::jtag::NEXT, foo);
        let bar = jtag_csr.rf(utra::jtag::NEXT_TDI);
        jtag_csr.rmwf(utra::jtag::NEXT_TDI, bar);
        let mut baz = jtag_csr.zf(utra::jtag::NEXT_TDI, bar);
        baz |= jtag_csr.ms(utra::jtag::NEXT_TDI, 1);
        jtag_csr.wfo(utra::jtag::NEXT_TDI, baz);
        let bar = jtag_csr.rf(utra::jtag::NEXT_TMS);
        jtag_csr.rmwf(utra::jtag::NEXT_TMS, bar);
        let mut baz = jtag_csr.zf(utra::jtag::NEXT_TMS, bar);
        baz |= jtag_csr.ms(utra::jtag::NEXT_TMS, 1);
        jtag_csr.wfo(utra::jtag::NEXT_TMS, baz);

        let foo = jtag_csr.r(utra::jtag::TDO);
        jtag_csr.wo(utra::jtag::TDO, foo);
        let bar = jtag_csr.rf(utra::jtag::TDO_TDO);
        jtag_csr.rmwf(utra::jtag::TDO_TDO, bar);
        let mut baz = jtag_csr.zf(utra::jtag::TDO_TDO, bar);
        baz |= jtag_csr.ms(utra::jtag::TDO_TDO, 1);
        jtag_csr.wfo(utra::jtag::TDO_TDO, baz);
        let bar = jtag_csr.rf(utra::jtag::TDO_READY);
        jtag_csr.rmwf(utra::jtag::TDO_READY, bar);
        let mut baz = jtag_csr.zf(utra::jtag::TDO_READY, bar);
        baz |= jtag_csr.ms(utra::jtag::TDO_READY, 1);
        jtag_csr.wfo(utra::jtag::TDO_READY, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_wdt_csr() {
        use super::*;
        let mut wdt_csr = CSR::new(HW_WDT_BASE as *mut u32);

        let foo = wdt_csr.r(utra::wdt::WATCHDOG);
        wdt_csr.wo(utra::wdt::WATCHDOG, foo);
        let bar = wdt_csr.rf(utra::wdt::WATCHDOG_RESET_WDT);
        wdt_csr.rmwf(utra::wdt::WATCHDOG_RESET_WDT, bar);
        let mut baz = wdt_csr.zf(utra::wdt::WATCHDOG_RESET_WDT, bar);
        baz |= wdt_csr.ms(utra::wdt::WATCHDOG_RESET_WDT, 1);
        wdt_csr.wfo(utra::wdt::WATCHDOG_RESET_WDT, baz);
        let bar = wdt_csr.rf(utra::wdt::WATCHDOG_ENABLE);
        wdt_csr.rmwf(utra::wdt::WATCHDOG_ENABLE, bar);
        let mut baz = wdt_csr.zf(utra::wdt::WATCHDOG_ENABLE, bar);
        baz |= wdt_csr.ms(utra::wdt::WATCHDOG_ENABLE, 1);
        wdt_csr.wfo(utra::wdt::WATCHDOG_ENABLE, baz);

        let foo = wdt_csr.r(utra::wdt::PERIOD);
        wdt_csr.wo(utra::wdt::PERIOD, foo);
        let bar = wdt_csr.rf(utra::wdt::PERIOD_PERIOD);
        wdt_csr.rmwf(utra::wdt::PERIOD_PERIOD, bar);
        let mut baz = wdt_csr.zf(utra::wdt::PERIOD_PERIOD, bar);
        baz |= wdt_csr.ms(utra::wdt::PERIOD_PERIOD, 1);
        wdt_csr.wfo(utra::wdt::PERIOD_PERIOD, baz);

        let foo = wdt_csr.r(utra::wdt::STATE);
        wdt_csr.wo(utra::wdt::STATE, foo);
        let bar = wdt_csr.rf(utra::wdt::STATE_ENABLED);
        wdt_csr.rmwf(utra::wdt::STATE_ENABLED, bar);
        let mut baz = wdt_csr.zf(utra::wdt::STATE_ENABLED, bar);
        baz |= wdt_csr.ms(utra::wdt::STATE_ENABLED, 1);
        wdt_csr.wfo(utra::wdt::STATE_ENABLED, baz);
        let bar = wdt_csr.rf(utra::wdt::STATE_ARMED1);
        wdt_csr.rmwf(utra::wdt::STATE_ARMED1, bar);
        let mut baz = wdt_csr.zf(utra::wdt::STATE_ARMED1, bar);
        baz |= wdt_csr.ms(utra::wdt::STATE_ARMED1, 1);
        wdt_csr.wfo(utra::wdt::STATE_ARMED1, baz);
        let bar = wdt_csr.rf(utra::wdt::STATE_ARMED2);
        wdt_csr.rmwf(utra::wdt::STATE_ARMED2, bar);
        let mut baz = wdt_csr.zf(utra::wdt::STATE_ARMED2, bar);
        baz |= wdt_csr.ms(utra::wdt::STATE_ARMED2, 1);
        wdt_csr.wfo(utra::wdt::STATE_ARMED2, baz);
        let bar = wdt_csr.rf(utra::wdt::STATE_DISARMED);
        wdt_csr.rmwf(utra::wdt::STATE_DISARMED, bar);
        let mut baz = wdt_csr.zf(utra::wdt::STATE_DISARMED, bar);
        baz |= wdt_csr.ms(utra::wdt::STATE_DISARMED, 1);
        wdt_csr.wfo(utra::wdt::STATE_DISARMED, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_usbdev_csr() {
        use super::*;
        let mut usbdev_csr = CSR::new(HW_USBDEV_BASE as *mut u32);

        let foo = usbdev_csr.r(utra::usbdev::USBDISABLE);
        usbdev_csr.wo(utra::usbdev::USBDISABLE, foo);
        let bar = usbdev_csr.rf(utra::usbdev::USBDISABLE_USBDISABLE);
        usbdev_csr.rmwf(utra::usbdev::USBDISABLE_USBDISABLE, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::USBDISABLE_USBDISABLE, bar);
        baz |= usbdev_csr.ms(utra::usbdev::USBDISABLE_USBDISABLE, 1);
        usbdev_csr.wfo(utra::usbdev::USBDISABLE_USBDISABLE, baz);

        let foo = usbdev_csr.r(utra::usbdev::USBSELECT);
        usbdev_csr.wo(utra::usbdev::USBSELECT, foo);
        let bar = usbdev_csr.rf(utra::usbdev::USBSELECT_SELECT_DEVICE);
        usbdev_csr.rmwf(utra::usbdev::USBSELECT_SELECT_DEVICE, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::USBSELECT_SELECT_DEVICE, bar);
        baz |= usbdev_csr.ms(utra::usbdev::USBSELECT_SELECT_DEVICE, 1);
        usbdev_csr.wfo(utra::usbdev::USBSELECT_SELECT_DEVICE, baz);
        let bar = usbdev_csr.rf(utra::usbdev::USBSELECT_FORCE_RESET);
        usbdev_csr.rmwf(utra::usbdev::USBSELECT_FORCE_RESET, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::USBSELECT_FORCE_RESET, bar);
        baz |= usbdev_csr.ms(utra::usbdev::USBSELECT_FORCE_RESET, 1);
        usbdev_csr.wfo(utra::usbdev::USBSELECT_FORCE_RESET, baz);

        let foo = usbdev_csr.r(utra::usbdev::EV_STATUS);
        usbdev_csr.wo(utra::usbdev::EV_STATUS, foo);
        let bar = usbdev_csr.rf(utra::usbdev::EV_STATUS_USB);
        usbdev_csr.rmwf(utra::usbdev::EV_STATUS_USB, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::EV_STATUS_USB, bar);
        baz |= usbdev_csr.ms(utra::usbdev::EV_STATUS_USB, 1);
        usbdev_csr.wfo(utra::usbdev::EV_STATUS_USB, baz);

        let foo = usbdev_csr.r(utra::usbdev::EV_PENDING);
        usbdev_csr.wo(utra::usbdev::EV_PENDING, foo);
        let bar = usbdev_csr.rf(utra::usbdev::EV_PENDING_USB);
        usbdev_csr.rmwf(utra::usbdev::EV_PENDING_USB, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::EV_PENDING_USB, bar);
        baz |= usbdev_csr.ms(utra::usbdev::EV_PENDING_USB, 1);
        usbdev_csr.wfo(utra::usbdev::EV_PENDING_USB, baz);

        let foo = usbdev_csr.r(utra::usbdev::EV_ENABLE);
        usbdev_csr.wo(utra::usbdev::EV_ENABLE, foo);
        let bar = usbdev_csr.rf(utra::usbdev::EV_ENABLE_USB);
        usbdev_csr.rmwf(utra::usbdev::EV_ENABLE_USB, bar);
        let mut baz = usbdev_csr.zf(utra::usbdev::EV_ENABLE_USB, bar);
        baz |= usbdev_csr.ms(utra::usbdev::EV_ENABLE_USB, 1);
        usbdev_csr.wfo(utra::usbdev::EV_ENABLE_USB, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_d11ctime_csr() {
        use super::*;
        let mut d11ctime_csr = CSR::new(HW_D11CTIME_BASE as *mut u32);

        let foo = d11ctime_csr.r(utra::d11ctime::CONTROL);
        d11ctime_csr.wo(utra::d11ctime::CONTROL, foo);
        let bar = d11ctime_csr.rf(utra::d11ctime::CONTROL_COUNT);
        d11ctime_csr.rmwf(utra::d11ctime::CONTROL_COUNT, bar);
        let mut baz = d11ctime_csr.zf(utra::d11ctime::CONTROL_COUNT, bar);
        baz |= d11ctime_csr.ms(utra::d11ctime::CONTROL_COUNT, 1);
        d11ctime_csr.wfo(utra::d11ctime::CONTROL_COUNT, baz);

        let foo = d11ctime_csr.r(utra::d11ctime::HEARTBEAT);
        d11ctime_csr.wo(utra::d11ctime::HEARTBEAT, foo);
        let bar = d11ctime_csr.rf(utra::d11ctime::HEARTBEAT_BEAT);
        d11ctime_csr.rmwf(utra::d11ctime::HEARTBEAT_BEAT, bar);
        let mut baz = d11ctime_csr.zf(utra::d11ctime::HEARTBEAT_BEAT, bar);
        baz |= d11ctime_csr.ms(utra::d11ctime::HEARTBEAT_BEAT, 1);
        d11ctime_csr.wfo(utra::d11ctime::HEARTBEAT_BEAT, baz);
  }

    #[test]
    #[ignore]
    fn compile_check_wfi_csr() {
        use super::*;
        let mut wfi_csr = CSR::new(HW_WFI_BASE as *mut u32);

        let foo = wfi_csr.r(utra::wfi::WFI);
        wfi_csr.wo(utra::wfi::WFI, foo);
        let bar = wfi_csr.rf(utra::wfi::WFI_WFI);
        wfi_csr.rmwf(utra::wfi::WFI_WFI, bar);
        let mut baz = wfi_csr.zf(utra::wfi::WFI_WFI, bar);
        baz |= wfi_csr.ms(utra::wfi::WFI_WFI, 1);
        wfi_csr.wfo(utra::wfi::WFI_WFI, baz);

        let foo = wfi_csr.r(utra::wfi::IGNORE_LOCKED);
        wfi_csr.wo(utra::wfi::IGNORE_LOCKED, foo);
        let bar = wfi_csr.rf(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED);
        wfi_csr.rmwf(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, bar);
        let mut baz = wfi_csr.zf(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, bar);
        baz |= wfi_csr.ms(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, 1);
        wfi_csr.wfo(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, baz);
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
}
