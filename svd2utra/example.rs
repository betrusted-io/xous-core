
use core::convert::TryInto;
pub struct Register {
    /// Offset of this register within this CSR
    offset: usize,
}
impl Register {
    pub const fn new(offset: usize) -> Register {
        Register { offset }
    }
}
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
            _ => 0,
        };
        Field {
            mask,
            offset,
            register,
        }
    }
}
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
    /// Read the contents of this register
    pub fn r(&mut self, reg: Register) -> T {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        unsafe { usize_base.add(reg.offset).read_volatile() }
            .try_into()
            .unwrap_or_default()
    }
    /// Read a field from this CSR
    pub fn rf(&mut self, field: Field) -> T {
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
            unsafe { usize_base.add(field.register.offset).read_volatile() } & !field.mask;
        unsafe {
            usize_base
                .add(field.register.offset)
                .write_volatile(previous | value_as_usize)
        };
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
    }
    /// Write the entire contents of a register without reading it first
    pub fn wo(&mut self, reg: Register, value: T) {
        let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
    }
    /// Zero a field from a provided value
    pub fn zf(&mut self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        (value_as_usize & !(field.mask << field.offset))
            .try_into()
            .unwrap_or_default()
    }
    /// Shift & mask a value to its final field position
    pub fn ms(&mut self, field: Field, value: T) -> T {
        let value_as_usize: usize = value.try_into().unwrap_or_default();
        ((value_as_usize & field.mask) << field.offset)
            .try_into()
            .unwrap_or_default()
    }
}
// Physical base addresses of memory regions
pub const HW_ROM_MEM:     usize = 0x00000000;
pub const HW_ROM_MEM_LEN: usize = 32768;
pub const HW_SRAM_MEM:     usize = 0x10000000;
pub const HW_SRAM_MEM_LEN: usize = 131072;
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
pub const HW_SHA2_MEM:     usize = 0xe0001000;
pub const HW_SHA2_MEM_LEN: usize = 4;
pub const HW_SHA512_MEM:     usize = 0xe0002000;
pub const HW_SHA512_MEM_LEN: usize = 8;
pub const HW_ENGINE_MEM:     usize = 0xe0020000;
pub const HW_ENGINE_MEM_LEN: usize = 131072;
pub const HW_CSR_MEM:     usize = 0xf0000000;
pub const HW_CSR_MEM_LEN: usize = 262144;

// Physical base addresses of registers
pub const HW_CTRL_BASE :   usize = 4026531840;
pub const HW_IDENTIFIER_MEM_BASE :   usize = 4026540032;
pub const HW_UART_PHY_BASE :   usize = 4026544128;
pub const HW_UART_BASE :   usize = 4026548224;
pub const HW_TIMER0_BASE :   usize = 4026552320;
pub const HW_REBOOT_BASE :   usize = 4026556416;
pub const HW_CRG_BASE :   usize = 4026560512;
pub const HW_INFO_BASE :   usize = 4026564608;
pub const HW_SRAM_EXT_BASE :   usize = 4026568704;
pub const HW_MEMLCD_BASE :   usize = 4026572800;
pub const HW_COM_BASE :   usize = 4026576896;
pub const HW_I2C_BASE :   usize = 4026580992;
pub const HW_BTEVENTS_BASE :   usize = 4026585088;
pub const HW_MESSIBLE_BASE :   usize = 4026589184;
pub const HW_TICKTIMER_BASE :   usize = 4026593280;
pub const HW_POWER_BASE :   usize = 4026597376;
pub const HW_SPINOR_BASE :   usize = 4026601472;
pub const HW_KEYBOARD_BASE :   usize = 4026605568;
pub const HW_GPIO_BASE :   usize = 4026609664;
pub const HW_SEED_BASE :   usize = 4026613760;
pub const HW_ROMTEST_BASE :   usize = 4026617856;
pub const HW_AUDIO_BASE :   usize = 4026621952;
pub const HW_TRNG_OSC_BASE :   usize = 4026626048;
pub const HW_AES_BASE :   usize = 4026630144;
pub const HW_SHA2_BASE :   usize = 4026634240;
pub const HW_SHA512_BASE :   usize = 4026638336;
pub const HW_ENGINE_BASE :   usize = 4026642432;
pub const HW_JTAG_BASE :   usize = 4026646528;

pub mod utra {

    pub mod ctrl {

        pub const RESET: crate::Register = crate::Register::new(0);
        pub const RESET_RESET: crate::Field = crate::Field::new(1, 0, RESET);

        pub const SCRATCH: crate::Register = crate::Register::new(4);
        pub const SCRATCH_SCRATCH: crate::Field = crate::Field::new(32, 0, SCRATCH);

        pub const BUS_ERRORS: crate::Register = crate::Register::new(8);
        pub const BUS_ERRORS_BUS_ERRORS: crate::Field = crate::Field::new(32, 0, BUS_ERRORS);

    }

    pub mod identifier_mem {

        pub const IDENTIFIER_MEM: crate::Register = crate::Register::new(0);
        pub const IDENTIFIER_MEM_IDENTIFIER_MEM: crate::Field = crate::Field::new(8, 0, IDENTIFIER_MEM);

    }

    pub mod uart_phy {

        pub const TUNING_WORD: crate::Register = crate::Register::new(0);
        pub const TUNING_WORD_TUNING_WORD: crate::Field = crate::Field::new(32, 0, TUNING_WORD);

    }

    pub mod uart {

        pub const RXTX: crate::Register = crate::Register::new(0);
        pub const RXTX_RXTX: crate::Field = crate::Field::new(8, 0, RXTX);

        pub const TXFULL: crate::Register = crate::Register::new(4);
        pub const TXFULL_TXFULL: crate::Field = crate::Field::new(1, 0, TXFULL);

        pub const RXEMPTY: crate::Register = crate::Register::new(8);
        pub const RXEMPTY_RXEMPTY: crate::Field = crate::Field::new(1, 0, RXEMPTY);

        pub const EV_STATUS: crate::Register = crate::Register::new(12);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(2, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(16);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(2, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(20);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(2, 0, EV_ENABLE);

        pub const TXEMPTY: crate::Register = crate::Register::new(24);
        pub const TXEMPTY_TXEMPTY: crate::Field = crate::Field::new(1, 0, TXEMPTY);

        pub const RXFULL: crate::Register = crate::Register::new(28);
        pub const RXFULL_RXFULL: crate::Field = crate::Field::new(1, 0, RXFULL);

        pub const UART_IRQ: usize = 0;
    }

    pub mod timer0 {

        pub const LOAD: crate::Register = crate::Register::new(0);
        pub const LOAD_LOAD: crate::Field = crate::Field::new(32, 0, LOAD);

        pub const RELOAD: crate::Register = crate::Register::new(4);
        pub const RELOAD_RELOAD: crate::Field = crate::Field::new(32, 0, RELOAD);

        pub const EN: crate::Register = crate::Register::new(8);
        pub const EN_EN: crate::Field = crate::Field::new(1, 0, EN);

        pub const UPDATE_VALUE: crate::Register = crate::Register::new(12);
        pub const UPDATE_VALUE_UPDATE_VALUE: crate::Field = crate::Field::new(1, 0, UPDATE_VALUE);

        pub const VALUE: crate::Register = crate::Register::new(16);
        pub const VALUE_VALUE: crate::Field = crate::Field::new(32, 0, VALUE);

        pub const EV_STATUS: crate::Register = crate::Register::new(20);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(24);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(28);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const TIMER0_IRQ: usize = 1;
    }

    pub mod reboot {

        pub const CTRL: crate::Register = crate::Register::new(0);
        pub const CTRL_CTRL: crate::Field = crate::Field::new(8, 0, CTRL);

        pub const ADDR: crate::Register = crate::Register::new(4);
        pub const ADDR_ADDR: crate::Field = crate::Field::new(32, 0, ADDR);

    }

    pub mod crg {

        pub const MMCM_DRP_RESET: crate::Register = crate::Register::new(0);
        pub const MMCM_DRP_RESET_MMCM_DRP_RESET: crate::Field = crate::Field::new(1, 0, MMCM_DRP_RESET);

        pub const MMCM_DRP_LOCKED: crate::Register = crate::Register::new(4);
        pub const MMCM_DRP_LOCKED_MMCM_DRP_LOCKED: crate::Field = crate::Field::new(1, 0, MMCM_DRP_LOCKED);

        pub const MMCM_DRP_READ: crate::Register = crate::Register::new(8);
        pub const MMCM_DRP_READ_MMCM_DRP_READ: crate::Field = crate::Field::new(1, 0, MMCM_DRP_READ);

        pub const MMCM_DRP_WRITE: crate::Register = crate::Register::new(12);
        pub const MMCM_DRP_WRITE_MMCM_DRP_WRITE: crate::Field = crate::Field::new(1, 0, MMCM_DRP_WRITE);

        pub const MMCM_DRP_DRDY: crate::Register = crate::Register::new(16);
        pub const MMCM_DRP_DRDY_MMCM_DRP_DRDY: crate::Field = crate::Field::new(1, 0, MMCM_DRP_DRDY);

        pub const MMCM_DRP_ADR: crate::Register = crate::Register::new(20);
        pub const MMCM_DRP_ADR_MMCM_DRP_ADR: crate::Field = crate::Field::new(7, 0, MMCM_DRP_ADR);

        pub const MMCM_DRP_DAT_W: crate::Register = crate::Register::new(24);
        pub const MMCM_DRP_DAT_W_MMCM_DRP_DAT_W: crate::Field = crate::Field::new(16, 0, MMCM_DRP_DAT_W);

        pub const MMCM_DRP_DAT_R: crate::Register = crate::Register::new(28);
        pub const MMCM_DRP_DAT_R_MMCM_DRP_DAT_R: crate::Field = crate::Field::new(16, 0, MMCM_DRP_DAT_R);

    }

    pub mod info {

        pub const DNA_ID1: crate::Register = crate::Register::new(0);
        pub const DNA_ID1_DNA_ID: crate::Field = crate::Field::new(32, 0, DNA_ID1);

        pub const DNA_ID0: crate::Register = crate::Register::new(4);
        pub const DNA_ID0_DNA_ID: crate::Field = crate::Field::new(32, 0, DNA_ID0);

        pub const GIT_MAJOR: crate::Register = crate::Register::new(8);
        pub const GIT_MAJOR_GIT_MAJOR: crate::Field = crate::Field::new(8, 0, GIT_MAJOR);

        pub const GIT_MINOR: crate::Register = crate::Register::new(12);
        pub const GIT_MINOR_GIT_MINOR: crate::Field = crate::Field::new(8, 0, GIT_MINOR);

        pub const GIT_REVISION: crate::Register = crate::Register::new(16);
        pub const GIT_REVISION_GIT_REVISION: crate::Field = crate::Field::new(8, 0, GIT_REVISION);

        pub const GIT_GITREV: crate::Register = crate::Register::new(20);
        pub const GIT_GITREV_GIT_GITREV: crate::Field = crate::Field::new(32, 0, GIT_GITREV);

        pub const GIT_GITEXTRA: crate::Register = crate::Register::new(24);
        pub const GIT_GITEXTRA_GIT_GITEXTRA: crate::Field = crate::Field::new(10, 0, GIT_GITEXTRA);

        pub const GIT_DIRTY: crate::Register = crate::Register::new(28);
        pub const GIT_DIRTY_DIRTY: crate::Field = crate::Field::new(1, 0, GIT_DIRTY);

        pub const PLATFORM_PLATFORM1: crate::Register = crate::Register::new(32);
        pub const PLATFORM_PLATFORM1_PLATFORM_PLATFORM: crate::Field = crate::Field::new(32, 0, PLATFORM_PLATFORM1);

        pub const PLATFORM_PLATFORM0: crate::Register = crate::Register::new(36);
        pub const PLATFORM_PLATFORM0_PLATFORM_PLATFORM: crate::Field = crate::Field::new(32, 0, PLATFORM_PLATFORM0);

        pub const PLATFORM_TARGET1: crate::Register = crate::Register::new(40);
        pub const PLATFORM_TARGET1_PLATFORM_TARGET: crate::Field = crate::Field::new(32, 0, PLATFORM_TARGET1);

        pub const PLATFORM_TARGET0: crate::Register = crate::Register::new(44);
        pub const PLATFORM_TARGET0_PLATFORM_TARGET: crate::Field = crate::Field::new(32, 0, PLATFORM_TARGET0);

        pub const XADC_TEMPERATURE: crate::Register = crate::Register::new(48);
        pub const XADC_TEMPERATURE_XADC_TEMPERATURE: crate::Field = crate::Field::new(12, 0, XADC_TEMPERATURE);

        pub const XADC_VCCINT: crate::Register = crate::Register::new(52);
        pub const XADC_VCCINT_XADC_VCCINT: crate::Field = crate::Field::new(12, 0, XADC_VCCINT);

        pub const XADC_VCCAUX: crate::Register = crate::Register::new(56);
        pub const XADC_VCCAUX_XADC_VCCAUX: crate::Field = crate::Field::new(12, 0, XADC_VCCAUX);

        pub const XADC_VCCBRAM: crate::Register = crate::Register::new(60);
        pub const XADC_VCCBRAM_XADC_VCCBRAM: crate::Field = crate::Field::new(12, 0, XADC_VCCBRAM);

        pub const XADC_EOC: crate::Register = crate::Register::new(64);
        pub const XADC_EOC_XADC_EOC: crate::Field = crate::Field::new(1, 0, XADC_EOC);

        pub const XADC_EOS: crate::Register = crate::Register::new(68);
        pub const XADC_EOS_XADC_EOS: crate::Field = crate::Field::new(1, 0, XADC_EOS);

        pub const XADC_DRP_ENABLE: crate::Register = crate::Register::new(72);
        pub const XADC_DRP_ENABLE_XADC_DRP_ENABLE: crate::Field = crate::Field::new(1, 0, XADC_DRP_ENABLE);

        pub const XADC_DRP_READ: crate::Register = crate::Register::new(76);
        pub const XADC_DRP_READ_XADC_DRP_READ: crate::Field = crate::Field::new(1, 0, XADC_DRP_READ);

        pub const XADC_DRP_WRITE: crate::Register = crate::Register::new(80);
        pub const XADC_DRP_WRITE_XADC_DRP_WRITE: crate::Field = crate::Field::new(1, 0, XADC_DRP_WRITE);

        pub const XADC_DRP_DRDY: crate::Register = crate::Register::new(84);
        pub const XADC_DRP_DRDY_XADC_DRP_DRDY: crate::Field = crate::Field::new(1, 0, XADC_DRP_DRDY);

        pub const XADC_DRP_ADR: crate::Register = crate::Register::new(88);
        pub const XADC_DRP_ADR_XADC_DRP_ADR: crate::Field = crate::Field::new(7, 0, XADC_DRP_ADR);

        pub const XADC_DRP_DAT_W: crate::Register = crate::Register::new(92);
        pub const XADC_DRP_DAT_W_XADC_DRP_DAT_W: crate::Field = crate::Field::new(16, 0, XADC_DRP_DAT_W);

        pub const XADC_DRP_DAT_R: crate::Register = crate::Register::new(96);
        pub const XADC_DRP_DAT_R_XADC_DRP_DAT_R: crate::Field = crate::Field::new(16, 0, XADC_DRP_DAT_R);

    }

    pub mod sram_ext {

        pub const CONFIG_STATUS: crate::Register = crate::Register::new(0);
        pub const CONFIG_STATUS_MODE: crate::Field = crate::Field::new(32, 0, CONFIG_STATUS);

        pub const READ_CONFIG: crate::Register = crate::Register::new(4);
        pub const READ_CONFIG_TRIGGER: crate::Field = crate::Field::new(1, 0, READ_CONFIG);

    }

    pub mod memlcd {

        pub const COMMAND: crate::Register = crate::Register::new(0);
        pub const COMMAND_UPDATEDIRTY: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_UPDATEALL: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const BUSY: crate::Register = crate::Register::new(4);
        pub const BUSY_BUSY: crate::Field = crate::Field::new(1, 0, BUSY);

        pub const PRESCALER: crate::Register = crate::Register::new(8);
        pub const PRESCALER_PRESCALER: crate::Field = crate::Field::new(8, 0, PRESCALER);

        pub const EV_STATUS: crate::Register = crate::Register::new(12);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(16);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(20);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

    }

    pub mod com {

        pub const TX: crate::Register = crate::Register::new(0);
        pub const TX_TX: crate::Field = crate::Field::new(16, 0, TX);

        pub const RX: crate::Register = crate::Register::new(4);
        pub const RX_RX: crate::Field = crate::Field::new(16, 0, RX);

        pub const CONTROL: crate::Register = crate::Register::new(8);
        pub const CONTROL_INTENA: crate::Field = crate::Field::new(1, 0, CONTROL);

        pub const STATUS: crate::Register = crate::Register::new(12);
        pub const STATUS_TIP: crate::Field = crate::Field::new(1, 0, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(16);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(20);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(24);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

    }

    pub mod i2c {

        pub const PRESCALE: crate::Register = crate::Register::new(0);
        pub const PRESCALE_PRESCALE: crate::Field = crate::Field::new(16, 0, PRESCALE);

        pub const CONTROL: crate::Register = crate::Register::new(4);
        pub const CONTROL_RESVD: crate::Field = crate::Field::new(6, 0, CONTROL);
        pub const CONTROL_IEN: crate::Field = crate::Field::new(1, 6, CONTROL);
        pub const CONTROL_EN: crate::Field = crate::Field::new(1, 7, CONTROL);

        pub const TXR: crate::Register = crate::Register::new(8);
        pub const TXR_TXR: crate::Field = crate::Field::new(8, 0, TXR);

        pub const RXR: crate::Register = crate::Register::new(12);
        pub const RXR_RXR: crate::Field = crate::Field::new(8, 0, RXR);

        pub const COMMAND: crate::Register = crate::Register::new(16);
        pub const COMMAND_IACK: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_RESVD: crate::Field = crate::Field::new(2, 1, COMMAND);
        pub const COMMAND_ACK: crate::Field = crate::Field::new(1, 3, COMMAND);
        pub const COMMAND_WR: crate::Field = crate::Field::new(1, 4, COMMAND);
        pub const COMMAND_RD: crate::Field = crate::Field::new(1, 5, COMMAND);
        pub const COMMAND_STO: crate::Field = crate::Field::new(1, 6, COMMAND);
        pub const COMMAND_STA: crate::Field = crate::Field::new(1, 7, COMMAND);

        pub const STATUS: crate::Register = crate::Register::new(20);
        pub const STATUS_IF: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_TIP: crate::Field = crate::Field::new(1, 1, STATUS);
        pub const STATUS_RESVD: crate::Field = crate::Field::new(3, 2, STATUS);
        pub const STATUS_ARBLOST: crate::Field = crate::Field::new(1, 5, STATUS);
        pub const STATUS_BUSY: crate::Field = crate::Field::new(1, 6, STATUS);
        pub const STATUS_RXACK: crate::Field = crate::Field::new(1, 7, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(24);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(28);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(32);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const I2C_IRQ: usize = 2;
    }

    pub mod btevents {

        pub const EV_STATUS: crate::Register = crate::Register::new(0);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(2, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(4);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(2, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(8);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(2, 0, EV_ENABLE);

        pub const BTEVENTS_IRQ: usize = 3;
    }

    pub mod messible {

        pub const IN: crate::Register = crate::Register::new(0);
        pub const IN_IN: crate::Field = crate::Field::new(8, 0, IN);

        pub const OUT: crate::Register = crate::Register::new(4);
        pub const OUT_OUT: crate::Field = crate::Field::new(8, 0, OUT);

        pub const STATUS: crate::Register = crate::Register::new(8);
        pub const STATUS_FULL: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_HAVE: crate::Field = crate::Field::new(1, 1, STATUS);

    }

    pub mod ticktimer {

        pub const CONTROL: crate::Register = crate::Register::new(0);
        pub const CONTROL_RESET: crate::Field = crate::Field::new(1, 0, CONTROL);
        pub const CONTROL_PAUSE: crate::Field = crate::Field::new(1, 1, CONTROL);

        pub const TIME1: crate::Register = crate::Register::new(4);
        pub const TIME1_TIME: crate::Field = crate::Field::new(32, 0, TIME1);

        pub const TIME0: crate::Register = crate::Register::new(8);
        pub const TIME0_TIME: crate::Field = crate::Field::new(32, 0, TIME0);

    }

    pub mod power {

        pub const POWER: crate::Register = crate::Register::new(0);
        pub const POWER_AUDIO: crate::Field = crate::Field::new(1, 0, POWER);
        pub const POWER_SELF: crate::Field = crate::Field::new(1, 1, POWER);
        pub const POWER_EC_SNOOP: crate::Field = crate::Field::new(1, 2, POWER);
        pub const POWER_STATE: crate::Field = crate::Field::new(2, 3, POWER);
        pub const POWER_NOISEBIAS: crate::Field = crate::Field::new(1, 5, POWER);
        pub const POWER_NOISE: crate::Field = crate::Field::new(2, 6, POWER);
        pub const POWER_RESET_EC: crate::Field = crate::Field::new(1, 8, POWER);
        pub const POWER_UP5K_ON: crate::Field = crate::Field::new(1, 9, POWER);
        pub const POWER_BOOSTMODE: crate::Field = crate::Field::new(1, 10, POWER);
        pub const POWER_SELFDESTRUCT: crate::Field = crate::Field::new(1, 11, POWER);

        pub const VIBE: crate::Register = crate::Register::new(4);
        pub const VIBE_VIBE: crate::Field = crate::Field::new(1, 0, VIBE);

        pub const EV_STATUS: crate::Register = crate::Register::new(8);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(12);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(16);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

    }

    pub mod spinor {

        pub const CONFIG: crate::Register = crate::Register::new(0);
        pub const CONFIG_DUMMY: crate::Field = crate::Field::new(5, 0, CONFIG);

        pub const DELAY_CONFIG: crate::Register = crate::Register::new(4);
        pub const DELAY_CONFIG_D: crate::Field = crate::Field::new(5, 0, DELAY_CONFIG);
        pub const DELAY_CONFIG_LOAD: crate::Field = crate::Field::new(1, 5, DELAY_CONFIG);

        pub const DELAY_STATUS: crate::Register = crate::Register::new(8);
        pub const DELAY_STATUS_Q: crate::Field = crate::Field::new(5, 0, DELAY_STATUS);

        pub const COMMAND: crate::Register = crate::Register::new(12);
        pub const COMMAND_WAKEUP: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_SECTOR_ERASE: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const SECTOR: crate::Register = crate::Register::new(16);
        pub const SECTOR_SECTOR: crate::Field = crate::Field::new(32, 0, SECTOR);

        pub const STATUS: crate::Register = crate::Register::new(20);
        pub const STATUS_WIP: crate::Field = crate::Field::new(1, 0, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(24);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(28);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(32);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const ECC_ADDRESS: crate::Register = crate::Register::new(36);
        pub const ECC_ADDRESS_ECC_ADDRESS: crate::Field = crate::Field::new(32, 0, ECC_ADDRESS);

        pub const ECC_STATUS: crate::Register = crate::Register::new(40);
        pub const ECC_STATUS_ECC_ERROR: crate::Field = crate::Field::new(1, 0, ECC_STATUS);
        pub const ECC_STATUS_ECC_OVERFLOW: crate::Field = crate::Field::new(1, 1, ECC_STATUS);

    }

    pub mod keyboard {

        pub const ROW0DAT: crate::Register = crate::Register::new(0);
        pub const ROW0DAT_ROW0DAT: crate::Field = crate::Field::new(10, 0, ROW0DAT);

        pub const ROW1DAT: crate::Register = crate::Register::new(4);
        pub const ROW1DAT_ROW1DAT: crate::Field = crate::Field::new(10, 0, ROW1DAT);

        pub const ROW2DAT: crate::Register = crate::Register::new(8);
        pub const ROW2DAT_ROW2DAT: crate::Field = crate::Field::new(10, 0, ROW2DAT);

        pub const ROW3DAT: crate::Register = crate::Register::new(12);
        pub const ROW3DAT_ROW3DAT: crate::Field = crate::Field::new(10, 0, ROW3DAT);

        pub const ROW4DAT: crate::Register = crate::Register::new(16);
        pub const ROW4DAT_ROW4DAT: crate::Field = crate::Field::new(10, 0, ROW4DAT);

        pub const ROW5DAT: crate::Register = crate::Register::new(20);
        pub const ROW5DAT_ROW5DAT: crate::Field = crate::Field::new(10, 0, ROW5DAT);

        pub const ROW6DAT: crate::Register = crate::Register::new(24);
        pub const ROW6DAT_ROW6DAT: crate::Field = crate::Field::new(10, 0, ROW6DAT);

        pub const ROW7DAT: crate::Register = crate::Register::new(28);
        pub const ROW7DAT_ROW7DAT: crate::Field = crate::Field::new(10, 0, ROW7DAT);

        pub const ROW8DAT: crate::Register = crate::Register::new(32);
        pub const ROW8DAT_ROW8DAT: crate::Field = crate::Field::new(10, 0, ROW8DAT);

        pub const EV_STATUS: crate::Register = crate::Register::new(36);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(1, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(40);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(1, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(44);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(1, 0, EV_ENABLE);

        pub const ROWCHANGE: crate::Register = crate::Register::new(48);
        pub const ROWCHANGE_ROWCHANGE: crate::Field = crate::Field::new(9, 0, ROWCHANGE);

        pub const KEYBOARD_IRQ: usize = 4;
    }

    pub mod gpio {

        pub const OUTPUT: crate::Register = crate::Register::new(0);
        pub const OUTPUT_OUTPUT: crate::Field = crate::Field::new(6, 0, OUTPUT);

        pub const INPUT: crate::Register = crate::Register::new(4);
        pub const INPUT_INPUT: crate::Field = crate::Field::new(6, 0, INPUT);

        pub const DRIVE: crate::Register = crate::Register::new(8);
        pub const DRIVE_DRIVE: crate::Field = crate::Field::new(6, 0, DRIVE);

        pub const INTENA: crate::Register = crate::Register::new(12);
        pub const INTENA_INTENA: crate::Field = crate::Field::new(6, 0, INTENA);

        pub const INTPOL: crate::Register = crate::Register::new(16);
        pub const INTPOL_INTPOL: crate::Field = crate::Field::new(6, 0, INTPOL);

        pub const EV_STATUS: crate::Register = crate::Register::new(20);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(6, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(24);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(6, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(28);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(6, 0, EV_ENABLE);

        pub const GPIO_IRQ: usize = 5;
    }

    pub mod seed {

        pub const SEED1: crate::Register = crate::Register::new(0);
        pub const SEED1_SEED: crate::Field = crate::Field::new(32, 0, SEED1);

        pub const SEED0: crate::Register = crate::Register::new(4);
        pub const SEED0_SEED: crate::Field = crate::Field::new(32, 0, SEED0);

    }

    pub mod romtest {

        pub const ADDRESS: crate::Register = crate::Register::new(0);
        pub const ADDRESS_ADDRESS: crate::Field = crate::Field::new(8, 0, ADDRESS);

        pub const DATA: crate::Register = crate::Register::new(4);
        pub const DATA_DATA: crate::Field = crate::Field::new(32, 0, DATA);

    }

    pub mod audio {

        pub const EV_STATUS: crate::Register = crate::Register::new(0);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(4, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(4);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(4, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(8);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(4, 0, EV_ENABLE);

        pub const RX_CTL: crate::Register = crate::Register::new(12);
        pub const RX_CTL_ENABLE: crate::Field = crate::Field::new(1, 0, RX_CTL);
        pub const RX_CTL_RESET: crate::Field = crate::Field::new(1, 1, RX_CTL);

        pub const RX_STAT: crate::Register = crate::Register::new(16);
        pub const RX_STAT_OVERFLOW: crate::Field = crate::Field::new(1, 0, RX_STAT);
        pub const RX_STAT_UNDERFLOW: crate::Field = crate::Field::new(1, 1, RX_STAT);
        pub const RX_STAT_DATAREADY: crate::Field = crate::Field::new(1, 2, RX_STAT);
        pub const RX_STAT_EMPTY: crate::Field = crate::Field::new(1, 3, RX_STAT);
        pub const RX_STAT_WRCOUNT: crate::Field = crate::Field::new(9, 4, RX_STAT);
        pub const RX_STAT_RDCOUNT: crate::Field = crate::Field::new(9, 13, RX_STAT);
        pub const RX_STAT_FIFO_DEPTH: crate::Field = crate::Field::new(9, 22, RX_STAT);
        pub const RX_STAT_CONCATENATE_CHANNELS: crate::Field = crate::Field::new(1, 31, RX_STAT);

        pub const RX_CONF: crate::Register = crate::Register::new(20);
        pub const RX_CONF_FORMAT: crate::Field = crate::Field::new(2, 0, RX_CONF);
        pub const RX_CONF_SAMPLE_WIDTH: crate::Field = crate::Field::new(6, 2, RX_CONF);
        pub const RX_CONF_LRCK_FREQ: crate::Field = crate::Field::new(24, 8, RX_CONF);

        pub const TX_CTL: crate::Register = crate::Register::new(24);
        pub const TX_CTL_ENABLE: crate::Field = crate::Field::new(1, 0, TX_CTL);
        pub const TX_CTL_RESET: crate::Field = crate::Field::new(1, 1, TX_CTL);

        pub const TX_STAT: crate::Register = crate::Register::new(28);
        pub const TX_STAT_OVERFLOW: crate::Field = crate::Field::new(1, 0, TX_STAT);
        pub const TX_STAT_UNDERFLOW: crate::Field = crate::Field::new(1, 1, TX_STAT);
        pub const TX_STAT_FREE: crate::Field = crate::Field::new(1, 2, TX_STAT);
        pub const TX_STAT_ALMOSTFULL: crate::Field = crate::Field::new(1, 3, TX_STAT);
        pub const TX_STAT_FULL: crate::Field = crate::Field::new(1, 4, TX_STAT);
        pub const TX_STAT_EMPTY: crate::Field = crate::Field::new(1, 5, TX_STAT);
        pub const TX_STAT_WRCOUNT: crate::Field = crate::Field::new(9, 6, TX_STAT);
        pub const TX_STAT_RDCOUNT: crate::Field = crate::Field::new(9, 15, TX_STAT);
        pub const TX_STAT_CONCATENATE_CHANNELS: crate::Field = crate::Field::new(1, 24, TX_STAT);

        pub const TX_CONF: crate::Register = crate::Register::new(32);
        pub const TX_CONF_FORMAT: crate::Field = crate::Field::new(2, 0, TX_CONF);
        pub const TX_CONF_SAMPLE_WIDTH: crate::Field = crate::Field::new(6, 2, TX_CONF);
        pub const TX_CONF_LRCK_FREQ: crate::Field = crate::Field::new(24, 8, TX_CONF);

        pub const AUDIO_IRQ: usize = 6;
    }

    pub mod trng_osc {

        pub const CTL: crate::Register = crate::Register::new(0);
        pub const CTL_ENA: crate::Field = crate::Field::new(1, 0, CTL);
        pub const CTL_GANG: crate::Field = crate::Field::new(1, 1, CTL);
        pub const CTL_DWELL: crate::Field = crate::Field::new(20, 2, CTL);
        pub const CTL_DELAY: crate::Field = crate::Field::new(10, 22, CTL);

        pub const RAND: crate::Register = crate::Register::new(4);
        pub const RAND_RAND: crate::Field = crate::Field::new(32, 0, RAND);

        pub const STATUS: crate::Register = crate::Register::new(8);
        pub const STATUS_FRESH: crate::Field = crate::Field::new(1, 0, STATUS);

    }

    pub mod aes {

        pub const KEY_0_Q: crate::Register = crate::Register::new(0);
        pub const KEY_0_Q_KEY_0: crate::Field = crate::Field::new(32, 0, KEY_0_Q);

        pub const KEY_1_Q: crate::Register = crate::Register::new(4);
        pub const KEY_1_Q_KEY_1: crate::Field = crate::Field::new(32, 0, KEY_1_Q);

        pub const KEY_2_Q: crate::Register = crate::Register::new(8);
        pub const KEY_2_Q_KEY_2: crate::Field = crate::Field::new(32, 0, KEY_2_Q);

        pub const KEY_3_Q: crate::Register = crate::Register::new(12);
        pub const KEY_3_Q_KEY_3: crate::Field = crate::Field::new(32, 0, KEY_3_Q);

        pub const KEY_4_Q: crate::Register = crate::Register::new(16);
        pub const KEY_4_Q_KEY_4: crate::Field = crate::Field::new(32, 0, KEY_4_Q);

        pub const KEY_5_Q: crate::Register = crate::Register::new(20);
        pub const KEY_5_Q_KEY_5: crate::Field = crate::Field::new(32, 0, KEY_5_Q);

        pub const KEY_6_Q: crate::Register = crate::Register::new(24);
        pub const KEY_6_Q_KEY_6: crate::Field = crate::Field::new(32, 0, KEY_6_Q);

        pub const KEY_7_Q: crate::Register = crate::Register::new(28);
        pub const KEY_7_Q_KEY_7: crate::Field = crate::Field::new(32, 0, KEY_7_Q);

        pub const DATAOUT_0: crate::Register = crate::Register::new(32);
        pub const DATAOUT_0_DATA_0: crate::Field = crate::Field::new(32, 0, DATAOUT_0);

        pub const DATAOUT_1: crate::Register = crate::Register::new(36);
        pub const DATAOUT_1_DATA_1: crate::Field = crate::Field::new(32, 0, DATAOUT_1);

        pub const DATAOUT_2: crate::Register = crate::Register::new(40);
        pub const DATAOUT_2_DATA_2: crate::Field = crate::Field::new(32, 0, DATAOUT_2);

        pub const DATAOUT_3: crate::Register = crate::Register::new(44);
        pub const DATAOUT_3_DATA_3: crate::Field = crate::Field::new(32, 0, DATAOUT_3);

        pub const DATAIN_0: crate::Register = crate::Register::new(48);
        pub const DATAIN_0_DATA_0: crate::Field = crate::Field::new(32, 0, DATAIN_0);

        pub const DATAIN_1: crate::Register = crate::Register::new(52);
        pub const DATAIN_1_DATA_1: crate::Field = crate::Field::new(32, 0, DATAIN_1);

        pub const DATAIN_2: crate::Register = crate::Register::new(56);
        pub const DATAIN_2_DATA_2: crate::Field = crate::Field::new(32, 0, DATAIN_2);

        pub const DATAIN_3: crate::Register = crate::Register::new(60);
        pub const DATAIN_3_DATA_3: crate::Field = crate::Field::new(32, 0, DATAIN_3);

        pub const IV_0: crate::Register = crate::Register::new(64);
        pub const IV_0_IV_0: crate::Field = crate::Field::new(32, 0, IV_0);

        pub const IV_1: crate::Register = crate::Register::new(68);
        pub const IV_1_IV_1: crate::Field = crate::Field::new(32, 0, IV_1);

        pub const IV_2: crate::Register = crate::Register::new(72);
        pub const IV_2_IV_2: crate::Field = crate::Field::new(32, 0, IV_2);

        pub const IV_3: crate::Register = crate::Register::new(76);
        pub const IV_3_IV_3: crate::Field = crate::Field::new(32, 0, IV_3);

        pub const CTRL: crate::Register = crate::Register::new(80);
        pub const CTRL_MODE: crate::Field = crate::Field::new(3, 0, CTRL);
        pub const CTRL_KEY_LEN: crate::Field = crate::Field::new(3, 3, CTRL);
        pub const CTRL_MANUAL_OPERATION: crate::Field = crate::Field::new(1, 6, CTRL);
        pub const CTRL_OPERATION: crate::Field = crate::Field::new(1, 7, CTRL);

        pub const STATUS: crate::Register = crate::Register::new(84);
        pub const STATUS_IDLE: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_STALL: crate::Field = crate::Field::new(1, 1, STATUS);
        pub const STATUS_OUTPUT_VALID: crate::Field = crate::Field::new(1, 2, STATUS);
        pub const STATUS_INPUT_READY: crate::Field = crate::Field::new(1, 3, STATUS);
        pub const STATUS_OPERATION_RBK: crate::Field = crate::Field::new(1, 4, STATUS);
        pub const STATUS_MODE_RBK: crate::Field = crate::Field::new(3, 5, STATUS);
        pub const STATUS_KEY_LEN_RBK: crate::Field = crate::Field::new(3, 8, STATUS);
        pub const STATUS_MANUAL_OPERATION_RBK: crate::Field = crate::Field::new(1, 11, STATUS);

        pub const TRIGGER: crate::Register = crate::Register::new(88);
        pub const TRIGGER_START: crate::Field = crate::Field::new(1, 0, TRIGGER);
        pub const TRIGGER_KEY_CLEAR: crate::Field = crate::Field::new(1, 1, TRIGGER);
        pub const TRIGGER_IV_CLEAR: crate::Field = crate::Field::new(1, 2, TRIGGER);
        pub const TRIGGER_DATA_IN_CLEAR: crate::Field = crate::Field::new(1, 3, TRIGGER);
        pub const TRIGGER_DATA_OUT_CLEAR: crate::Field = crate::Field::new(1, 4, TRIGGER);
        pub const TRIGGER_PRNG_RESEED: crate::Field = crate::Field::new(1, 5, TRIGGER);

    }

    pub mod sha2 {

        pub const KEY0: crate::Register = crate::Register::new(0);
        pub const KEY0_KEY0: crate::Field = crate::Field::new(32, 0, KEY0);

        pub const KEY1: crate::Register = crate::Register::new(4);
        pub const KEY1_KEY1: crate::Field = crate::Field::new(32, 0, KEY1);

        pub const KEY2: crate::Register = crate::Register::new(8);
        pub const KEY2_KEY2: crate::Field = crate::Field::new(32, 0, KEY2);

        pub const KEY3: crate::Register = crate::Register::new(12);
        pub const KEY3_KEY3: crate::Field = crate::Field::new(32, 0, KEY3);

        pub const KEY4: crate::Register = crate::Register::new(16);
        pub const KEY4_KEY4: crate::Field = crate::Field::new(32, 0, KEY4);

        pub const KEY5: crate::Register = crate::Register::new(20);
        pub const KEY5_KEY5: crate::Field = crate::Field::new(32, 0, KEY5);

        pub const KEY6: crate::Register = crate::Register::new(24);
        pub const KEY6_KEY6: crate::Field = crate::Field::new(32, 0, KEY6);

        pub const KEY7: crate::Register = crate::Register::new(28);
        pub const KEY7_KEY7: crate::Field = crate::Field::new(32, 0, KEY7);

        pub const CONFIG: crate::Register = crate::Register::new(32);
        pub const CONFIG_SHA_EN: crate::Field = crate::Field::new(1, 0, CONFIG);
        pub const CONFIG_ENDIAN_SWAP: crate::Field = crate::Field::new(1, 1, CONFIG);
        pub const CONFIG_DIGEST_SWAP: crate::Field = crate::Field::new(1, 2, CONFIG);
        pub const CONFIG_HMAC_EN: crate::Field = crate::Field::new(1, 3, CONFIG);

        pub const COMMAND: crate::Register = crate::Register::new(36);
        pub const COMMAND_HASH_START: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_HASH_PROCESS: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const WIPE: crate::Register = crate::Register::new(40);
        pub const WIPE_WIPE: crate::Field = crate::Field::new(32, 0, WIPE);

        pub const DIGEST0: crate::Register = crate::Register::new(44);
        pub const DIGEST0_DIGEST0: crate::Field = crate::Field::new(32, 0, DIGEST0);

        pub const DIGEST1: crate::Register = crate::Register::new(48);
        pub const DIGEST1_DIGEST1: crate::Field = crate::Field::new(32, 0, DIGEST1);

        pub const DIGEST2: crate::Register = crate::Register::new(52);
        pub const DIGEST2_DIGEST2: crate::Field = crate::Field::new(32, 0, DIGEST2);

        pub const DIGEST3: crate::Register = crate::Register::new(56);
        pub const DIGEST3_DIGEST3: crate::Field = crate::Field::new(32, 0, DIGEST3);

        pub const DIGEST4: crate::Register = crate::Register::new(60);
        pub const DIGEST4_DIGEST4: crate::Field = crate::Field::new(32, 0, DIGEST4);

        pub const DIGEST5: crate::Register = crate::Register::new(64);
        pub const DIGEST5_DIGEST5: crate::Field = crate::Field::new(32, 0, DIGEST5);

        pub const DIGEST6: crate::Register = crate::Register::new(68);
        pub const DIGEST6_DIGEST6: crate::Field = crate::Field::new(32, 0, DIGEST6);

        pub const DIGEST7: crate::Register = crate::Register::new(72);
        pub const DIGEST7_DIGEST7: crate::Field = crate::Field::new(32, 0, DIGEST7);

        pub const MSG_LENGTH1: crate::Register = crate::Register::new(76);
        pub const MSG_LENGTH1_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH1);

        pub const MSG_LENGTH0: crate::Register = crate::Register::new(80);
        pub const MSG_LENGTH0_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH0);

        pub const ERROR_CODE: crate::Register = crate::Register::new(84);
        pub const ERROR_CODE_ERROR_CODE: crate::Field = crate::Field::new(32, 0, ERROR_CODE);

        pub const EV_STATUS: crate::Register = crate::Register::new(88);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(4, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(92);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(4, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(96);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(4, 0, EV_ENABLE);

        pub const FIFO: crate::Register = crate::Register::new(100);
        pub const FIFO_READ_COUNT: crate::Field = crate::Field::new(10, 0, FIFO);
        pub const FIFO_WRITE_COUNT: crate::Field = crate::Field::new(10, 10, FIFO);
        pub const FIFO_READ_ERROR: crate::Field = crate::Field::new(1, 20, FIFO);
        pub const FIFO_WRITE_ERROR: crate::Field = crate::Field::new(1, 21, FIFO);
        pub const FIFO_ALMOST_FULL: crate::Field = crate::Field::new(1, 22, FIFO);
        pub const FIFO_ALMOST_EMPTY: crate::Field = crate::Field::new(1, 23, FIFO);

        pub const SHA2_IRQ: usize = 7;
    }

    pub mod sha512 {

        pub const CONFIG: crate::Register = crate::Register::new(0);
        pub const CONFIG_SHA_EN: crate::Field = crate::Field::new(1, 0, CONFIG);
        pub const CONFIG_ENDIAN_SWAP: crate::Field = crate::Field::new(1, 1, CONFIG);
        pub const CONFIG_DIGEST_SWAP: crate::Field = crate::Field::new(1, 2, CONFIG);
        pub const CONFIG_SELECT_256: crate::Field = crate::Field::new(1, 3, CONFIG);

        pub const COMMAND: crate::Register = crate::Register::new(4);
        pub const COMMAND_HASH_START: crate::Field = crate::Field::new(1, 0, COMMAND);
        pub const COMMAND_HASH_PROCESS: crate::Field = crate::Field::new(1, 1, COMMAND);

        pub const DIGEST01: crate::Register = crate::Register::new(8);
        pub const DIGEST01_DIGEST0: crate::Field = crate::Field::new(32, 0, DIGEST01);

        pub const DIGEST00: crate::Register = crate::Register::new(12);
        pub const DIGEST00_DIGEST0: crate::Field = crate::Field::new(32, 0, DIGEST00);

        pub const DIGEST11: crate::Register = crate::Register::new(16);
        pub const DIGEST11_DIGEST1: crate::Field = crate::Field::new(32, 0, DIGEST11);

        pub const DIGEST10: crate::Register = crate::Register::new(20);
        pub const DIGEST10_DIGEST1: crate::Field = crate::Field::new(32, 0, DIGEST10);

        pub const DIGEST21: crate::Register = crate::Register::new(24);
        pub const DIGEST21_DIGEST2: crate::Field = crate::Field::new(32, 0, DIGEST21);

        pub const DIGEST20: crate::Register = crate::Register::new(28);
        pub const DIGEST20_DIGEST2: crate::Field = crate::Field::new(32, 0, DIGEST20);

        pub const DIGEST31: crate::Register = crate::Register::new(32);
        pub const DIGEST31_DIGEST3: crate::Field = crate::Field::new(32, 0, DIGEST31);

        pub const DIGEST30: crate::Register = crate::Register::new(36);
        pub const DIGEST30_DIGEST3: crate::Field = crate::Field::new(32, 0, DIGEST30);

        pub const DIGEST41: crate::Register = crate::Register::new(40);
        pub const DIGEST41_DIGEST4: crate::Field = crate::Field::new(32, 0, DIGEST41);

        pub const DIGEST40: crate::Register = crate::Register::new(44);
        pub const DIGEST40_DIGEST4: crate::Field = crate::Field::new(32, 0, DIGEST40);

        pub const DIGEST51: crate::Register = crate::Register::new(48);
        pub const DIGEST51_DIGEST5: crate::Field = crate::Field::new(32, 0, DIGEST51);

        pub const DIGEST50: crate::Register = crate::Register::new(52);
        pub const DIGEST50_DIGEST5: crate::Field = crate::Field::new(32, 0, DIGEST50);

        pub const DIGEST61: crate::Register = crate::Register::new(56);
        pub const DIGEST61_DIGEST6: crate::Field = crate::Field::new(32, 0, DIGEST61);

        pub const DIGEST60: crate::Register = crate::Register::new(60);
        pub const DIGEST60_DIGEST6: crate::Field = crate::Field::new(32, 0, DIGEST60);

        pub const DIGEST71: crate::Register = crate::Register::new(64);
        pub const DIGEST71_DIGEST7: crate::Field = crate::Field::new(32, 0, DIGEST71);

        pub const DIGEST70: crate::Register = crate::Register::new(68);
        pub const DIGEST70_DIGEST7: crate::Field = crate::Field::new(32, 0, DIGEST70);

        pub const MSG_LENGTH1: crate::Register = crate::Register::new(72);
        pub const MSG_LENGTH1_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH1);

        pub const MSG_LENGTH0: crate::Register = crate::Register::new(76);
        pub const MSG_LENGTH0_MSG_LENGTH: crate::Field = crate::Field::new(32, 0, MSG_LENGTH0);

        pub const EV_STATUS: crate::Register = crate::Register::new(80);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(3, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(84);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(3, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(88);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(3, 0, EV_ENABLE);

        pub const FIFO: crate::Register = crate::Register::new(92);
        pub const FIFO_READ_COUNT: crate::Field = crate::Field::new(9, 0, FIFO);
        pub const FIFO_WRITE_COUNT: crate::Field = crate::Field::new(9, 9, FIFO);
        pub const FIFO_READ_ERROR: crate::Field = crate::Field::new(1, 18, FIFO);
        pub const FIFO_WRITE_ERROR: crate::Field = crate::Field::new(1, 19, FIFO);
        pub const FIFO_ALMOST_FULL: crate::Field = crate::Field::new(1, 20, FIFO);
        pub const FIFO_ALMOST_EMPTY: crate::Field = crate::Field::new(1, 21, FIFO);
        pub const FIFO_RUNNING: crate::Field = crate::Field::new(1, 22, FIFO);

        pub const SHA512_IRQ: usize = 8;
    }

    pub mod engine {

        pub const WINDOW: crate::Register = crate::Register::new(0);
        pub const WINDOW_WINDOW: crate::Field = crate::Field::new(4, 0, WINDOW);

        pub const MPSTART: crate::Register = crate::Register::new(4);
        pub const MPSTART_MPSTART: crate::Field = crate::Field::new(10, 0, MPSTART);

        pub const MPLEN: crate::Register = crate::Register::new(8);
        pub const MPLEN_MPLEN: crate::Field = crate::Field::new(10, 0, MPLEN);

        pub const CONTROL: crate::Register = crate::Register::new(12);
        pub const CONTROL_GO: crate::Field = crate::Field::new(1, 0, CONTROL);

        pub const STATUS: crate::Register = crate::Register::new(16);
        pub const STATUS_RUNNING: crate::Field = crate::Field::new(1, 0, STATUS);
        pub const STATUS_MPC: crate::Field = crate::Field::new(10, 1, STATUS);

        pub const EV_STATUS: crate::Register = crate::Register::new(20);
        pub const EV_STATUS_STATUS: crate::Field = crate::Field::new(2, 0, EV_STATUS);

        pub const EV_PENDING: crate::Register = crate::Register::new(24);
        pub const EV_PENDING_PENDING: crate::Field = crate::Field::new(2, 0, EV_PENDING);

        pub const EV_ENABLE: crate::Register = crate::Register::new(28);
        pub const EV_ENABLE_ENABLE: crate::Field = crate::Field::new(2, 0, EV_ENABLE);

        pub const INSTRUCTION: crate::Register = crate::Register::new(32);
        pub const INSTRUCTION_OPCODE: crate::Field = crate::Field::new(6, 0, INSTRUCTION);
        pub const INSTRUCTION_RA: crate::Field = crate::Field::new(5, 6, INSTRUCTION);
        pub const INSTRUCTION_CA: crate::Field = crate::Field::new(1, 11, INSTRUCTION);
        pub const INSTRUCTION_RB: crate::Field = crate::Field::new(5, 12, INSTRUCTION);
        pub const INSTRUCTION_CB: crate::Field = crate::Field::new(1, 17, INSTRUCTION);
        pub const INSTRUCTION_WD: crate::Field = crate::Field::new(5, 18, INSTRUCTION);
        pub const INSTRUCTION_IMMEDIATE: crate::Field = crate::Field::new(9, 23, INSTRUCTION);

        pub const ENGINE_IRQ: usize = 9;
    }

    pub mod jtag {

        pub const NEXT: crate::Register = crate::Register::new(0);
        pub const NEXT_TDI: crate::Field = crate::Field::new(1, 0, NEXT);
        pub const NEXT_TMS: crate::Field = crate::Field::new(1, 1, NEXT);

        pub const TDO: crate::Register = crate::Register::new(4);
        pub const TDO_TDO: crate::Field = crate::Field::new(1, 0, TDO);
        pub const TDO_READY: crate::Field = crate::Field::new(1, 1, TDO);

    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn compile_check() {
        use super::*;
        let mut ctrl_csr = CSR::new(HW_CTRL_BASE as *mut u32);

        let foo = ctrl_csr.r(utra::ctrl::RESET);
        ctrl_csr.wo(utra::ctrl::RESET, foo);
        let bar = ctrl_csr.rf(utra::ctrl::RESET_RESET);
        ctrl_csr.rmwf(utra::ctrl::RESET_RESET, bar);
        let mut baz = ctrl_csr.zf(utra::ctrl::RESET_RESET, bar);
        baz |= ctrl_csr.ms(utra::ctrl::RESET_RESET, 1);
        ctrl_csr.wfo(utra::ctrl::RESET_RESET, baz);

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
        let mut identifier_mem_csr = CSR::new(HW_IDENTIFIER_MEM_BASE as *mut u32);

        let foo = identifier_mem_csr.r(utra::identifier_mem::IDENTIFIER_MEM);
        identifier_mem_csr.wo(utra::identifier_mem::IDENTIFIER_MEM, foo);
        let bar = identifier_mem_csr.rf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM);
        identifier_mem_csr.rmwf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, bar);
        let mut baz = identifier_mem_csr.zf(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, bar);
        baz |= identifier_mem_csr.ms(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, 1);
        identifier_mem_csr.wfo(utra::identifier_mem::IDENTIFIER_MEM_IDENTIFIER_MEM, baz);
        let mut uart_phy_csr = CSR::new(HW_UART_PHY_BASE as *mut u32);

        let foo = uart_phy_csr.r(utra::uart_phy::TUNING_WORD);
        uart_phy_csr.wo(utra::uart_phy::TUNING_WORD, foo);
        let bar = uart_phy_csr.rf(utra::uart_phy::TUNING_WORD_TUNING_WORD);
        uart_phy_csr.rmwf(utra::uart_phy::TUNING_WORD_TUNING_WORD, bar);
        let mut baz = uart_phy_csr.zf(utra::uart_phy::TUNING_WORD_TUNING_WORD, bar);
        baz |= uart_phy_csr.ms(utra::uart_phy::TUNING_WORD_TUNING_WORD, 1);
        uart_phy_csr.wfo(utra::uart_phy::TUNING_WORD_TUNING_WORD, baz);
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
        let bar = uart_csr.rf(utra::uart::EV_STATUS_STATUS);
        uart_csr.rmwf(utra::uart::EV_STATUS_STATUS, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_STATUS_STATUS, bar);
        baz |= uart_csr.ms(utra::uart::EV_STATUS_STATUS, 1);
        uart_csr.wfo(utra::uart::EV_STATUS_STATUS, baz);

        let foo = uart_csr.r(utra::uart::EV_PENDING);
        uart_csr.wo(utra::uart::EV_PENDING, foo);
        let bar = uart_csr.rf(utra::uart::EV_PENDING_PENDING);
        uart_csr.rmwf(utra::uart::EV_PENDING_PENDING, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_PENDING_PENDING, bar);
        baz |= uart_csr.ms(utra::uart::EV_PENDING_PENDING, 1);
        uart_csr.wfo(utra::uart::EV_PENDING_PENDING, baz);

        let foo = uart_csr.r(utra::uart::EV_ENABLE);
        uart_csr.wo(utra::uart::EV_ENABLE, foo);
        let bar = uart_csr.rf(utra::uart::EV_ENABLE_ENABLE);
        uart_csr.rmwf(utra::uart::EV_ENABLE_ENABLE, bar);
        let mut baz = uart_csr.zf(utra::uart::EV_ENABLE_ENABLE, bar);
        baz |= uart_csr.ms(utra::uart::EV_ENABLE_ENABLE, 1);
        uart_csr.wfo(utra::uart::EV_ENABLE_ENABLE, baz);

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
        let bar = timer0_csr.rf(utra::timer0::EV_STATUS_STATUS);
        timer0_csr.rmwf(utra::timer0::EV_STATUS_STATUS, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_STATUS_STATUS, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_STATUS_STATUS, 1);
        timer0_csr.wfo(utra::timer0::EV_STATUS_STATUS, baz);

        let foo = timer0_csr.r(utra::timer0::EV_PENDING);
        timer0_csr.wo(utra::timer0::EV_PENDING, foo);
        let bar = timer0_csr.rf(utra::timer0::EV_PENDING_PENDING);
        timer0_csr.rmwf(utra::timer0::EV_PENDING_PENDING, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_PENDING_PENDING, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_PENDING_PENDING, 1);
        timer0_csr.wfo(utra::timer0::EV_PENDING_PENDING, baz);

        let foo = timer0_csr.r(utra::timer0::EV_ENABLE);
        timer0_csr.wo(utra::timer0::EV_ENABLE, foo);
        let bar = timer0_csr.rf(utra::timer0::EV_ENABLE_ENABLE);
        timer0_csr.rmwf(utra::timer0::EV_ENABLE_ENABLE, bar);
        let mut baz = timer0_csr.zf(utra::timer0::EV_ENABLE_ENABLE, bar);
        baz |= timer0_csr.ms(utra::timer0::EV_ENABLE_ENABLE, 1);
        timer0_csr.wfo(utra::timer0::EV_ENABLE_ENABLE, baz);
        let mut reboot_csr = CSR::new(HW_REBOOT_BASE as *mut u32);

        let foo = reboot_csr.r(utra::reboot::CTRL);
        reboot_csr.wo(utra::reboot::CTRL, foo);
        let bar = reboot_csr.rf(utra::reboot::CTRL_CTRL);
        reboot_csr.rmwf(utra::reboot::CTRL_CTRL, bar);
        let mut baz = reboot_csr.zf(utra::reboot::CTRL_CTRL, bar);
        baz |= reboot_csr.ms(utra::reboot::CTRL_CTRL, 1);
        reboot_csr.wfo(utra::reboot::CTRL_CTRL, baz);

        let foo = reboot_csr.r(utra::reboot::ADDR);
        reboot_csr.wo(utra::reboot::ADDR, foo);
        let bar = reboot_csr.rf(utra::reboot::ADDR_ADDR);
        reboot_csr.rmwf(utra::reboot::ADDR_ADDR, bar);
        let mut baz = reboot_csr.zf(utra::reboot::ADDR_ADDR, bar);
        baz |= reboot_csr.ms(utra::reboot::ADDR_ADDR, 1);
        reboot_csr.wfo(utra::reboot::ADDR_ADDR, baz);
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

        let foo = info_csr.r(utra::info::XADC_TEMPERATURE);
        info_csr.wo(utra::info::XADC_TEMPERATURE, foo);
        let bar = info_csr.rf(utra::info::XADC_TEMPERATURE_XADC_TEMPERATURE);
        info_csr.rmwf(utra::info::XADC_TEMPERATURE_XADC_TEMPERATURE, bar);
        let mut baz = info_csr.zf(utra::info::XADC_TEMPERATURE_XADC_TEMPERATURE, bar);
        baz |= info_csr.ms(utra::info::XADC_TEMPERATURE_XADC_TEMPERATURE, 1);
        info_csr.wfo(utra::info::XADC_TEMPERATURE_XADC_TEMPERATURE, baz);

        let foo = info_csr.r(utra::info::XADC_VCCINT);
        info_csr.wo(utra::info::XADC_VCCINT, foo);
        let bar = info_csr.rf(utra::info::XADC_VCCINT_XADC_VCCINT);
        info_csr.rmwf(utra::info::XADC_VCCINT_XADC_VCCINT, bar);
        let mut baz = info_csr.zf(utra::info::XADC_VCCINT_XADC_VCCINT, bar);
        baz |= info_csr.ms(utra::info::XADC_VCCINT_XADC_VCCINT, 1);
        info_csr.wfo(utra::info::XADC_VCCINT_XADC_VCCINT, baz);

        let foo = info_csr.r(utra::info::XADC_VCCAUX);
        info_csr.wo(utra::info::XADC_VCCAUX, foo);
        let bar = info_csr.rf(utra::info::XADC_VCCAUX_XADC_VCCAUX);
        info_csr.rmwf(utra::info::XADC_VCCAUX_XADC_VCCAUX, bar);
        let mut baz = info_csr.zf(utra::info::XADC_VCCAUX_XADC_VCCAUX, bar);
        baz |= info_csr.ms(utra::info::XADC_VCCAUX_XADC_VCCAUX, 1);
        info_csr.wfo(utra::info::XADC_VCCAUX_XADC_VCCAUX, baz);

        let foo = info_csr.r(utra::info::XADC_VCCBRAM);
        info_csr.wo(utra::info::XADC_VCCBRAM, foo);
        let bar = info_csr.rf(utra::info::XADC_VCCBRAM_XADC_VCCBRAM);
        info_csr.rmwf(utra::info::XADC_VCCBRAM_XADC_VCCBRAM, bar);
        let mut baz = info_csr.zf(utra::info::XADC_VCCBRAM_XADC_VCCBRAM, bar);
        baz |= info_csr.ms(utra::info::XADC_VCCBRAM_XADC_VCCBRAM, 1);
        info_csr.wfo(utra::info::XADC_VCCBRAM_XADC_VCCBRAM, baz);

        let foo = info_csr.r(utra::info::XADC_EOC);
        info_csr.wo(utra::info::XADC_EOC, foo);
        let bar = info_csr.rf(utra::info::XADC_EOC_XADC_EOC);
        info_csr.rmwf(utra::info::XADC_EOC_XADC_EOC, bar);
        let mut baz = info_csr.zf(utra::info::XADC_EOC_XADC_EOC, bar);
        baz |= info_csr.ms(utra::info::XADC_EOC_XADC_EOC, 1);
        info_csr.wfo(utra::info::XADC_EOC_XADC_EOC, baz);

        let foo = info_csr.r(utra::info::XADC_EOS);
        info_csr.wo(utra::info::XADC_EOS, foo);
        let bar = info_csr.rf(utra::info::XADC_EOS_XADC_EOS);
        info_csr.rmwf(utra::info::XADC_EOS_XADC_EOS, bar);
        let mut baz = info_csr.zf(utra::info::XADC_EOS_XADC_EOS, bar);
        baz |= info_csr.ms(utra::info::XADC_EOS_XADC_EOS, 1);
        info_csr.wfo(utra::info::XADC_EOS_XADC_EOS, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_ENABLE);
        info_csr.wo(utra::info::XADC_DRP_ENABLE, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_ENABLE_XADC_DRP_ENABLE);
        info_csr.rmwf(utra::info::XADC_DRP_ENABLE_XADC_DRP_ENABLE, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_ENABLE_XADC_DRP_ENABLE, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_ENABLE_XADC_DRP_ENABLE, 1);
        info_csr.wfo(utra::info::XADC_DRP_ENABLE_XADC_DRP_ENABLE, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_READ);
        info_csr.wo(utra::info::XADC_DRP_READ, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_READ_XADC_DRP_READ);
        info_csr.rmwf(utra::info::XADC_DRP_READ_XADC_DRP_READ, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_READ_XADC_DRP_READ, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_READ_XADC_DRP_READ, 1);
        info_csr.wfo(utra::info::XADC_DRP_READ_XADC_DRP_READ, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_WRITE);
        info_csr.wo(utra::info::XADC_DRP_WRITE, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_WRITE_XADC_DRP_WRITE);
        info_csr.rmwf(utra::info::XADC_DRP_WRITE_XADC_DRP_WRITE, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_WRITE_XADC_DRP_WRITE, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_WRITE_XADC_DRP_WRITE, 1);
        info_csr.wfo(utra::info::XADC_DRP_WRITE_XADC_DRP_WRITE, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_DRDY);
        info_csr.wo(utra::info::XADC_DRP_DRDY, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_DRDY_XADC_DRP_DRDY);
        info_csr.rmwf(utra::info::XADC_DRP_DRDY_XADC_DRP_DRDY, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_DRDY_XADC_DRP_DRDY, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_DRDY_XADC_DRP_DRDY, 1);
        info_csr.wfo(utra::info::XADC_DRP_DRDY_XADC_DRP_DRDY, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_ADR);
        info_csr.wo(utra::info::XADC_DRP_ADR, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_ADR_XADC_DRP_ADR);
        info_csr.rmwf(utra::info::XADC_DRP_ADR_XADC_DRP_ADR, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_ADR_XADC_DRP_ADR, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_ADR_XADC_DRP_ADR, 1);
        info_csr.wfo(utra::info::XADC_DRP_ADR_XADC_DRP_ADR, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_DAT_W);
        info_csr.wo(utra::info::XADC_DRP_DAT_W, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_DAT_W_XADC_DRP_DAT_W);
        info_csr.rmwf(utra::info::XADC_DRP_DAT_W_XADC_DRP_DAT_W, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_DAT_W_XADC_DRP_DAT_W, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_DAT_W_XADC_DRP_DAT_W, 1);
        info_csr.wfo(utra::info::XADC_DRP_DAT_W_XADC_DRP_DAT_W, baz);

        let foo = info_csr.r(utra::info::XADC_DRP_DAT_R);
        info_csr.wo(utra::info::XADC_DRP_DAT_R, foo);
        let bar = info_csr.rf(utra::info::XADC_DRP_DAT_R_XADC_DRP_DAT_R);
        info_csr.rmwf(utra::info::XADC_DRP_DAT_R_XADC_DRP_DAT_R, bar);
        let mut baz = info_csr.zf(utra::info::XADC_DRP_DAT_R_XADC_DRP_DAT_R, bar);
        baz |= info_csr.ms(utra::info::XADC_DRP_DAT_R_XADC_DRP_DAT_R, 1);
        info_csr.wfo(utra::info::XADC_DRP_DAT_R_XADC_DRP_DAT_R, baz);
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
        let bar = memlcd_csr.rf(utra::memlcd::EV_STATUS_STATUS);
        memlcd_csr.rmwf(utra::memlcd::EV_STATUS_STATUS, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_STATUS_STATUS, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_STATUS_STATUS, 1);
        memlcd_csr.wfo(utra::memlcd::EV_STATUS_STATUS, baz);

        let foo = memlcd_csr.r(utra::memlcd::EV_PENDING);
        memlcd_csr.wo(utra::memlcd::EV_PENDING, foo);
        let bar = memlcd_csr.rf(utra::memlcd::EV_PENDING_PENDING);
        memlcd_csr.rmwf(utra::memlcd::EV_PENDING_PENDING, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_PENDING_PENDING, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_PENDING_PENDING, 1);
        memlcd_csr.wfo(utra::memlcd::EV_PENDING_PENDING, baz);

        let foo = memlcd_csr.r(utra::memlcd::EV_ENABLE);
        memlcd_csr.wo(utra::memlcd::EV_ENABLE, foo);
        let bar = memlcd_csr.rf(utra::memlcd::EV_ENABLE_ENABLE);
        memlcd_csr.rmwf(utra::memlcd::EV_ENABLE_ENABLE, bar);
        let mut baz = memlcd_csr.zf(utra::memlcd::EV_ENABLE_ENABLE, bar);
        baz |= memlcd_csr.ms(utra::memlcd::EV_ENABLE_ENABLE, 1);
        memlcd_csr.wfo(utra::memlcd::EV_ENABLE_ENABLE, baz);
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

        let foo = com_csr.r(utra::com::STATUS);
        com_csr.wo(utra::com::STATUS, foo);
        let bar = com_csr.rf(utra::com::STATUS_TIP);
        com_csr.rmwf(utra::com::STATUS_TIP, bar);
        let mut baz = com_csr.zf(utra::com::STATUS_TIP, bar);
        baz |= com_csr.ms(utra::com::STATUS_TIP, 1);
        com_csr.wfo(utra::com::STATUS_TIP, baz);

        let foo = com_csr.r(utra::com::EV_STATUS);
        com_csr.wo(utra::com::EV_STATUS, foo);
        let bar = com_csr.rf(utra::com::EV_STATUS_STATUS);
        com_csr.rmwf(utra::com::EV_STATUS_STATUS, bar);
        let mut baz = com_csr.zf(utra::com::EV_STATUS_STATUS, bar);
        baz |= com_csr.ms(utra::com::EV_STATUS_STATUS, 1);
        com_csr.wfo(utra::com::EV_STATUS_STATUS, baz);

        let foo = com_csr.r(utra::com::EV_PENDING);
        com_csr.wo(utra::com::EV_PENDING, foo);
        let bar = com_csr.rf(utra::com::EV_PENDING_PENDING);
        com_csr.rmwf(utra::com::EV_PENDING_PENDING, bar);
        let mut baz = com_csr.zf(utra::com::EV_PENDING_PENDING, bar);
        baz |= com_csr.ms(utra::com::EV_PENDING_PENDING, 1);
        com_csr.wfo(utra::com::EV_PENDING_PENDING, baz);

        let foo = com_csr.r(utra::com::EV_ENABLE);
        com_csr.wo(utra::com::EV_ENABLE, foo);
        let bar = com_csr.rf(utra::com::EV_ENABLE_ENABLE);
        com_csr.rmwf(utra::com::EV_ENABLE_ENABLE, bar);
        let mut baz = com_csr.zf(utra::com::EV_ENABLE_ENABLE, bar);
        baz |= com_csr.ms(utra::com::EV_ENABLE_ENABLE, 1);
        com_csr.wfo(utra::com::EV_ENABLE_ENABLE, baz);
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

        let foo = i2c_csr.r(utra::i2c::EV_STATUS);
        i2c_csr.wo(utra::i2c::EV_STATUS, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_STATUS_STATUS);
        i2c_csr.rmwf(utra::i2c::EV_STATUS_STATUS, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_STATUS_STATUS, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_STATUS_STATUS, 1);
        i2c_csr.wfo(utra::i2c::EV_STATUS_STATUS, baz);

        let foo = i2c_csr.r(utra::i2c::EV_PENDING);
        i2c_csr.wo(utra::i2c::EV_PENDING, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_PENDING_PENDING);
        i2c_csr.rmwf(utra::i2c::EV_PENDING_PENDING, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_PENDING_PENDING, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_PENDING_PENDING, 1);
        i2c_csr.wfo(utra::i2c::EV_PENDING_PENDING, baz);

        let foo = i2c_csr.r(utra::i2c::EV_ENABLE);
        i2c_csr.wo(utra::i2c::EV_ENABLE, foo);
        let bar = i2c_csr.rf(utra::i2c::EV_ENABLE_ENABLE);
        i2c_csr.rmwf(utra::i2c::EV_ENABLE_ENABLE, bar);
        let mut baz = i2c_csr.zf(utra::i2c::EV_ENABLE_ENABLE, bar);
        baz |= i2c_csr.ms(utra::i2c::EV_ENABLE_ENABLE, 1);
        i2c_csr.wfo(utra::i2c::EV_ENABLE_ENABLE, baz);
        let mut btevents_csr = CSR::new(HW_BTEVENTS_BASE as *mut u32);

        let foo = btevents_csr.r(utra::btevents::EV_STATUS);
        btevents_csr.wo(utra::btevents::EV_STATUS, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_STATUS_STATUS);
        btevents_csr.rmwf(utra::btevents::EV_STATUS_STATUS, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_STATUS_STATUS, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_STATUS_STATUS, 1);
        btevents_csr.wfo(utra::btevents::EV_STATUS_STATUS, baz);

        let foo = btevents_csr.r(utra::btevents::EV_PENDING);
        btevents_csr.wo(utra::btevents::EV_PENDING, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_PENDING_PENDING);
        btevents_csr.rmwf(utra::btevents::EV_PENDING_PENDING, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_PENDING_PENDING, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_PENDING_PENDING, 1);
        btevents_csr.wfo(utra::btevents::EV_PENDING_PENDING, baz);

        let foo = btevents_csr.r(utra::btevents::EV_ENABLE);
        btevents_csr.wo(utra::btevents::EV_ENABLE, foo);
        let bar = btevents_csr.rf(utra::btevents::EV_ENABLE_ENABLE);
        btevents_csr.rmwf(utra::btevents::EV_ENABLE_ENABLE, bar);
        let mut baz = btevents_csr.zf(utra::btevents::EV_ENABLE_ENABLE, bar);
        baz |= btevents_csr.ms(utra::btevents::EV_ENABLE_ENABLE, 1);
        btevents_csr.wfo(utra::btevents::EV_ENABLE_ENABLE, baz);
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
        let mut ticktimer_csr = CSR::new(HW_TICKTIMER_BASE as *mut u32);

        let foo = ticktimer_csr.r(utra::ticktimer::CONTROL);
        ticktimer_csr.wo(utra::ticktimer::CONTROL, foo);
        let bar = ticktimer_csr.rf(utra::ticktimer::CONTROL_RESET);
        ticktimer_csr.rmwf(utra::ticktimer::CONTROL_RESET, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::CONTROL_RESET, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::CONTROL_RESET, 1);
        ticktimer_csr.wfo(utra::ticktimer::CONTROL_RESET, baz);
        let bar = ticktimer_csr.rf(utra::ticktimer::CONTROL_PAUSE);
        ticktimer_csr.rmwf(utra::ticktimer::CONTROL_PAUSE, bar);
        let mut baz = ticktimer_csr.zf(utra::ticktimer::CONTROL_PAUSE, bar);
        baz |= ticktimer_csr.ms(utra::ticktimer::CONTROL_PAUSE, 1);
        ticktimer_csr.wfo(utra::ticktimer::CONTROL_PAUSE, baz);

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
        let bar = power_csr.rf(utra::power::POWER_NOISEBIAS);
        power_csr.rmwf(utra::power::POWER_NOISEBIAS, bar);
        let mut baz = power_csr.zf(utra::power::POWER_NOISEBIAS, bar);
        baz |= power_csr.ms(utra::power::POWER_NOISEBIAS, 1);
        power_csr.wfo(utra::power::POWER_NOISEBIAS, baz);
        let bar = power_csr.rf(utra::power::POWER_NOISE);
        power_csr.rmwf(utra::power::POWER_NOISE, bar);
        let mut baz = power_csr.zf(utra::power::POWER_NOISE, bar);
        baz |= power_csr.ms(utra::power::POWER_NOISE, 1);
        power_csr.wfo(utra::power::POWER_NOISE, baz);
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

        let foo = power_csr.r(utra::power::VIBE);
        power_csr.wo(utra::power::VIBE, foo);
        let bar = power_csr.rf(utra::power::VIBE_VIBE);
        power_csr.rmwf(utra::power::VIBE_VIBE, bar);
        let mut baz = power_csr.zf(utra::power::VIBE_VIBE, bar);
        baz |= power_csr.ms(utra::power::VIBE_VIBE, 1);
        power_csr.wfo(utra::power::VIBE_VIBE, baz);

        let foo = power_csr.r(utra::power::EV_STATUS);
        power_csr.wo(utra::power::EV_STATUS, foo);
        let bar = power_csr.rf(utra::power::EV_STATUS_STATUS);
        power_csr.rmwf(utra::power::EV_STATUS_STATUS, bar);
        let mut baz = power_csr.zf(utra::power::EV_STATUS_STATUS, bar);
        baz |= power_csr.ms(utra::power::EV_STATUS_STATUS, 1);
        power_csr.wfo(utra::power::EV_STATUS_STATUS, baz);

        let foo = power_csr.r(utra::power::EV_PENDING);
        power_csr.wo(utra::power::EV_PENDING, foo);
        let bar = power_csr.rf(utra::power::EV_PENDING_PENDING);
        power_csr.rmwf(utra::power::EV_PENDING_PENDING, bar);
        let mut baz = power_csr.zf(utra::power::EV_PENDING_PENDING, bar);
        baz |= power_csr.ms(utra::power::EV_PENDING_PENDING, 1);
        power_csr.wfo(utra::power::EV_PENDING_PENDING, baz);

        let foo = power_csr.r(utra::power::EV_ENABLE);
        power_csr.wo(utra::power::EV_ENABLE, foo);
        let bar = power_csr.rf(utra::power::EV_ENABLE_ENABLE);
        power_csr.rmwf(utra::power::EV_ENABLE_ENABLE, bar);
        let mut baz = power_csr.zf(utra::power::EV_ENABLE_ENABLE, bar);
        baz |= power_csr.ms(utra::power::EV_ENABLE_ENABLE, 1);
        power_csr.wfo(utra::power::EV_ENABLE_ENABLE, baz);
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
        let bar = spinor_csr.rf(utra::spinor::COMMAND_SECTOR_ERASE);
        spinor_csr.rmwf(utra::spinor::COMMAND_SECTOR_ERASE, bar);
        let mut baz = spinor_csr.zf(utra::spinor::COMMAND_SECTOR_ERASE, bar);
        baz |= spinor_csr.ms(utra::spinor::COMMAND_SECTOR_ERASE, 1);
        spinor_csr.wfo(utra::spinor::COMMAND_SECTOR_ERASE, baz);

        let foo = spinor_csr.r(utra::spinor::SECTOR);
        spinor_csr.wo(utra::spinor::SECTOR, foo);
        let bar = spinor_csr.rf(utra::spinor::SECTOR_SECTOR);
        spinor_csr.rmwf(utra::spinor::SECTOR_SECTOR, bar);
        let mut baz = spinor_csr.zf(utra::spinor::SECTOR_SECTOR, bar);
        baz |= spinor_csr.ms(utra::spinor::SECTOR_SECTOR, 1);
        spinor_csr.wfo(utra::spinor::SECTOR_SECTOR, baz);

        let foo = spinor_csr.r(utra::spinor::STATUS);
        spinor_csr.wo(utra::spinor::STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::STATUS_WIP);
        spinor_csr.rmwf(utra::spinor::STATUS_WIP, bar);
        let mut baz = spinor_csr.zf(utra::spinor::STATUS_WIP, bar);
        baz |= spinor_csr.ms(utra::spinor::STATUS_WIP, 1);
        spinor_csr.wfo(utra::spinor::STATUS_WIP, baz);

        let foo = spinor_csr.r(utra::spinor::EV_STATUS);
        spinor_csr.wo(utra::spinor::EV_STATUS, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_STATUS_STATUS);
        spinor_csr.rmwf(utra::spinor::EV_STATUS_STATUS, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_STATUS_STATUS, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_STATUS_STATUS, 1);
        spinor_csr.wfo(utra::spinor::EV_STATUS_STATUS, baz);

        let foo = spinor_csr.r(utra::spinor::EV_PENDING);
        spinor_csr.wo(utra::spinor::EV_PENDING, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_PENDING_PENDING);
        spinor_csr.rmwf(utra::spinor::EV_PENDING_PENDING, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_PENDING_PENDING, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_PENDING_PENDING, 1);
        spinor_csr.wfo(utra::spinor::EV_PENDING_PENDING, baz);

        let foo = spinor_csr.r(utra::spinor::EV_ENABLE);
        spinor_csr.wo(utra::spinor::EV_ENABLE, foo);
        let bar = spinor_csr.rf(utra::spinor::EV_ENABLE_ENABLE);
        spinor_csr.rmwf(utra::spinor::EV_ENABLE_ENABLE, bar);
        let mut baz = spinor_csr.zf(utra::spinor::EV_ENABLE_ENABLE, bar);
        baz |= spinor_csr.ms(utra::spinor::EV_ENABLE_ENABLE, 1);
        spinor_csr.wfo(utra::spinor::EV_ENABLE_ENABLE, baz);

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
        let mut keyboard_csr = CSR::new(HW_KEYBOARD_BASE as *mut u32);

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
        let bar = keyboard_csr.rf(utra::keyboard::EV_STATUS_STATUS);
        keyboard_csr.rmwf(utra::keyboard::EV_STATUS_STATUS, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_STATUS_STATUS, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_STATUS_STATUS, 1);
        keyboard_csr.wfo(utra::keyboard::EV_STATUS_STATUS, baz);

        let foo = keyboard_csr.r(utra::keyboard::EV_PENDING);
        keyboard_csr.wo(utra::keyboard::EV_PENDING, foo);
        let bar = keyboard_csr.rf(utra::keyboard::EV_PENDING_PENDING);
        keyboard_csr.rmwf(utra::keyboard::EV_PENDING_PENDING, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_PENDING_PENDING, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_PENDING_PENDING, 1);
        keyboard_csr.wfo(utra::keyboard::EV_PENDING_PENDING, baz);

        let foo = keyboard_csr.r(utra::keyboard::EV_ENABLE);
        keyboard_csr.wo(utra::keyboard::EV_ENABLE, foo);
        let bar = keyboard_csr.rf(utra::keyboard::EV_ENABLE_ENABLE);
        keyboard_csr.rmwf(utra::keyboard::EV_ENABLE_ENABLE, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::EV_ENABLE_ENABLE, bar);
        baz |= keyboard_csr.ms(utra::keyboard::EV_ENABLE_ENABLE, 1);
        keyboard_csr.wfo(utra::keyboard::EV_ENABLE_ENABLE, baz);

        let foo = keyboard_csr.r(utra::keyboard::ROWCHANGE);
        keyboard_csr.wo(utra::keyboard::ROWCHANGE, foo);
        let bar = keyboard_csr.rf(utra::keyboard::ROWCHANGE_ROWCHANGE);
        keyboard_csr.rmwf(utra::keyboard::ROWCHANGE_ROWCHANGE, bar);
        let mut baz = keyboard_csr.zf(utra::keyboard::ROWCHANGE_ROWCHANGE, bar);
        baz |= keyboard_csr.ms(utra::keyboard::ROWCHANGE_ROWCHANGE, 1);
        keyboard_csr.wfo(utra::keyboard::ROWCHANGE_ROWCHANGE, baz);
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

        let foo = gpio_csr.r(utra::gpio::EV_STATUS);
        gpio_csr.wo(utra::gpio::EV_STATUS, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_STATUS_STATUS);
        gpio_csr.rmwf(utra::gpio::EV_STATUS_STATUS, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_STATUS_STATUS, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_STATUS_STATUS, 1);
        gpio_csr.wfo(utra::gpio::EV_STATUS_STATUS, baz);

        let foo = gpio_csr.r(utra::gpio::EV_PENDING);
        gpio_csr.wo(utra::gpio::EV_PENDING, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_PENDING_PENDING);
        gpio_csr.rmwf(utra::gpio::EV_PENDING_PENDING, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_PENDING_PENDING, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_PENDING_PENDING, 1);
        gpio_csr.wfo(utra::gpio::EV_PENDING_PENDING, baz);

        let foo = gpio_csr.r(utra::gpio::EV_ENABLE);
        gpio_csr.wo(utra::gpio::EV_ENABLE, foo);
        let bar = gpio_csr.rf(utra::gpio::EV_ENABLE_ENABLE);
        gpio_csr.rmwf(utra::gpio::EV_ENABLE_ENABLE, bar);
        let mut baz = gpio_csr.zf(utra::gpio::EV_ENABLE_ENABLE, bar);
        baz |= gpio_csr.ms(utra::gpio::EV_ENABLE_ENABLE, 1);
        gpio_csr.wfo(utra::gpio::EV_ENABLE_ENABLE, baz);
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
        let mut romtest_csr = CSR::new(HW_ROMTEST_BASE as *mut u32);

        let foo = romtest_csr.r(utra::romtest::ADDRESS);
        romtest_csr.wo(utra::romtest::ADDRESS, foo);
        let bar = romtest_csr.rf(utra::romtest::ADDRESS_ADDRESS);
        romtest_csr.rmwf(utra::romtest::ADDRESS_ADDRESS, bar);
        let mut baz = romtest_csr.zf(utra::romtest::ADDRESS_ADDRESS, bar);
        baz |= romtest_csr.ms(utra::romtest::ADDRESS_ADDRESS, 1);
        romtest_csr.wfo(utra::romtest::ADDRESS_ADDRESS, baz);

        let foo = romtest_csr.r(utra::romtest::DATA);
        romtest_csr.wo(utra::romtest::DATA, foo);
        let bar = romtest_csr.rf(utra::romtest::DATA_DATA);
        romtest_csr.rmwf(utra::romtest::DATA_DATA, bar);
        let mut baz = romtest_csr.zf(utra::romtest::DATA_DATA, bar);
        baz |= romtest_csr.ms(utra::romtest::DATA_DATA, 1);
        romtest_csr.wfo(utra::romtest::DATA_DATA, baz);
        let mut audio_csr = CSR::new(HW_AUDIO_BASE as *mut u32);

        let foo = audio_csr.r(utra::audio::EV_STATUS);
        audio_csr.wo(utra::audio::EV_STATUS, foo);
        let bar = audio_csr.rf(utra::audio::EV_STATUS_STATUS);
        audio_csr.rmwf(utra::audio::EV_STATUS_STATUS, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_STATUS_STATUS, bar);
        baz |= audio_csr.ms(utra::audio::EV_STATUS_STATUS, 1);
        audio_csr.wfo(utra::audio::EV_STATUS_STATUS, baz);

        let foo = audio_csr.r(utra::audio::EV_PENDING);
        audio_csr.wo(utra::audio::EV_PENDING, foo);
        let bar = audio_csr.rf(utra::audio::EV_PENDING_PENDING);
        audio_csr.rmwf(utra::audio::EV_PENDING_PENDING, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_PENDING_PENDING, bar);
        baz |= audio_csr.ms(utra::audio::EV_PENDING_PENDING, 1);
        audio_csr.wfo(utra::audio::EV_PENDING_PENDING, baz);

        let foo = audio_csr.r(utra::audio::EV_ENABLE);
        audio_csr.wo(utra::audio::EV_ENABLE, foo);
        let bar = audio_csr.rf(utra::audio::EV_ENABLE_ENABLE);
        audio_csr.rmwf(utra::audio::EV_ENABLE_ENABLE, bar);
        let mut baz = audio_csr.zf(utra::audio::EV_ENABLE_ENABLE, bar);
        baz |= audio_csr.ms(utra::audio::EV_ENABLE_ENABLE, 1);
        audio_csr.wfo(utra::audio::EV_ENABLE_ENABLE, baz);

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
        let mut trng_osc_csr = CSR::new(HW_TRNG_OSC_BASE as *mut u32);

        let foo = trng_osc_csr.r(utra::trng_osc::CTL);
        trng_osc_csr.wo(utra::trng_osc::CTL, foo);
        let bar = trng_osc_csr.rf(utra::trng_osc::CTL_ENA);
        trng_osc_csr.rmwf(utra::trng_osc::CTL_ENA, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::CTL_ENA, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::CTL_ENA, 1);
        trng_osc_csr.wfo(utra::trng_osc::CTL_ENA, baz);
        let bar = trng_osc_csr.rf(utra::trng_osc::CTL_GANG);
        trng_osc_csr.rmwf(utra::trng_osc::CTL_GANG, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::CTL_GANG, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::CTL_GANG, 1);
        trng_osc_csr.wfo(utra::trng_osc::CTL_GANG, baz);
        let bar = trng_osc_csr.rf(utra::trng_osc::CTL_DWELL);
        trng_osc_csr.rmwf(utra::trng_osc::CTL_DWELL, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::CTL_DWELL, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::CTL_DWELL, 1);
        trng_osc_csr.wfo(utra::trng_osc::CTL_DWELL, baz);
        let bar = trng_osc_csr.rf(utra::trng_osc::CTL_DELAY);
        trng_osc_csr.rmwf(utra::trng_osc::CTL_DELAY, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::CTL_DELAY, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::CTL_DELAY, 1);
        trng_osc_csr.wfo(utra::trng_osc::CTL_DELAY, baz);

        let foo = trng_osc_csr.r(utra::trng_osc::RAND);
        trng_osc_csr.wo(utra::trng_osc::RAND, foo);
        let bar = trng_osc_csr.rf(utra::trng_osc::RAND_RAND);
        trng_osc_csr.rmwf(utra::trng_osc::RAND_RAND, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::RAND_RAND, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::RAND_RAND, 1);
        trng_osc_csr.wfo(utra::trng_osc::RAND_RAND, baz);

        let foo = trng_osc_csr.r(utra::trng_osc::STATUS);
        trng_osc_csr.wo(utra::trng_osc::STATUS, foo);
        let bar = trng_osc_csr.rf(utra::trng_osc::STATUS_FRESH);
        trng_osc_csr.rmwf(utra::trng_osc::STATUS_FRESH, bar);
        let mut baz = trng_osc_csr.zf(utra::trng_osc::STATUS_FRESH, bar);
        baz |= trng_osc_csr.ms(utra::trng_osc::STATUS_FRESH, 1);
        trng_osc_csr.wfo(utra::trng_osc::STATUS_FRESH, baz);
        let mut aes_csr = CSR::new(HW_AES_BASE as *mut u32);

        let foo = aes_csr.r(utra::aes::KEY_0_Q);
        aes_csr.wo(utra::aes::KEY_0_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_0_Q_KEY_0);
        aes_csr.rmwf(utra::aes::KEY_0_Q_KEY_0, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_0_Q_KEY_0, bar);
        baz |= aes_csr.ms(utra::aes::KEY_0_Q_KEY_0, 1);
        aes_csr.wfo(utra::aes::KEY_0_Q_KEY_0, baz);

        let foo = aes_csr.r(utra::aes::KEY_1_Q);
        aes_csr.wo(utra::aes::KEY_1_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_1_Q_KEY_1);
        aes_csr.rmwf(utra::aes::KEY_1_Q_KEY_1, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_1_Q_KEY_1, bar);
        baz |= aes_csr.ms(utra::aes::KEY_1_Q_KEY_1, 1);
        aes_csr.wfo(utra::aes::KEY_1_Q_KEY_1, baz);

        let foo = aes_csr.r(utra::aes::KEY_2_Q);
        aes_csr.wo(utra::aes::KEY_2_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_2_Q_KEY_2);
        aes_csr.rmwf(utra::aes::KEY_2_Q_KEY_2, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_2_Q_KEY_2, bar);
        baz |= aes_csr.ms(utra::aes::KEY_2_Q_KEY_2, 1);
        aes_csr.wfo(utra::aes::KEY_2_Q_KEY_2, baz);

        let foo = aes_csr.r(utra::aes::KEY_3_Q);
        aes_csr.wo(utra::aes::KEY_3_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_3_Q_KEY_3);
        aes_csr.rmwf(utra::aes::KEY_3_Q_KEY_3, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_3_Q_KEY_3, bar);
        baz |= aes_csr.ms(utra::aes::KEY_3_Q_KEY_3, 1);
        aes_csr.wfo(utra::aes::KEY_3_Q_KEY_3, baz);

        let foo = aes_csr.r(utra::aes::KEY_4_Q);
        aes_csr.wo(utra::aes::KEY_4_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_4_Q_KEY_4);
        aes_csr.rmwf(utra::aes::KEY_4_Q_KEY_4, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_4_Q_KEY_4, bar);
        baz |= aes_csr.ms(utra::aes::KEY_4_Q_KEY_4, 1);
        aes_csr.wfo(utra::aes::KEY_4_Q_KEY_4, baz);

        let foo = aes_csr.r(utra::aes::KEY_5_Q);
        aes_csr.wo(utra::aes::KEY_5_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_5_Q_KEY_5);
        aes_csr.rmwf(utra::aes::KEY_5_Q_KEY_5, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_5_Q_KEY_5, bar);
        baz |= aes_csr.ms(utra::aes::KEY_5_Q_KEY_5, 1);
        aes_csr.wfo(utra::aes::KEY_5_Q_KEY_5, baz);

        let foo = aes_csr.r(utra::aes::KEY_6_Q);
        aes_csr.wo(utra::aes::KEY_6_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_6_Q_KEY_6);
        aes_csr.rmwf(utra::aes::KEY_6_Q_KEY_6, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_6_Q_KEY_6, bar);
        baz |= aes_csr.ms(utra::aes::KEY_6_Q_KEY_6, 1);
        aes_csr.wfo(utra::aes::KEY_6_Q_KEY_6, baz);

        let foo = aes_csr.r(utra::aes::KEY_7_Q);
        aes_csr.wo(utra::aes::KEY_7_Q, foo);
        let bar = aes_csr.rf(utra::aes::KEY_7_Q_KEY_7);
        aes_csr.rmwf(utra::aes::KEY_7_Q_KEY_7, bar);
        let mut baz = aes_csr.zf(utra::aes::KEY_7_Q_KEY_7, bar);
        baz |= aes_csr.ms(utra::aes::KEY_7_Q_KEY_7, 1);
        aes_csr.wfo(utra::aes::KEY_7_Q_KEY_7, baz);

        let foo = aes_csr.r(utra::aes::DATAOUT_0);
        aes_csr.wo(utra::aes::DATAOUT_0, foo);
        let bar = aes_csr.rf(utra::aes::DATAOUT_0_DATA_0);
        aes_csr.rmwf(utra::aes::DATAOUT_0_DATA_0, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAOUT_0_DATA_0, bar);
        baz |= aes_csr.ms(utra::aes::DATAOUT_0_DATA_0, 1);
        aes_csr.wfo(utra::aes::DATAOUT_0_DATA_0, baz);

        let foo = aes_csr.r(utra::aes::DATAOUT_1);
        aes_csr.wo(utra::aes::DATAOUT_1, foo);
        let bar = aes_csr.rf(utra::aes::DATAOUT_1_DATA_1);
        aes_csr.rmwf(utra::aes::DATAOUT_1_DATA_1, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAOUT_1_DATA_1, bar);
        baz |= aes_csr.ms(utra::aes::DATAOUT_1_DATA_1, 1);
        aes_csr.wfo(utra::aes::DATAOUT_1_DATA_1, baz);

        let foo = aes_csr.r(utra::aes::DATAOUT_2);
        aes_csr.wo(utra::aes::DATAOUT_2, foo);
        let bar = aes_csr.rf(utra::aes::DATAOUT_2_DATA_2);
        aes_csr.rmwf(utra::aes::DATAOUT_2_DATA_2, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAOUT_2_DATA_2, bar);
        baz |= aes_csr.ms(utra::aes::DATAOUT_2_DATA_2, 1);
        aes_csr.wfo(utra::aes::DATAOUT_2_DATA_2, baz);

        let foo = aes_csr.r(utra::aes::DATAOUT_3);
        aes_csr.wo(utra::aes::DATAOUT_3, foo);
        let bar = aes_csr.rf(utra::aes::DATAOUT_3_DATA_3);
        aes_csr.rmwf(utra::aes::DATAOUT_3_DATA_3, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAOUT_3_DATA_3, bar);
        baz |= aes_csr.ms(utra::aes::DATAOUT_3_DATA_3, 1);
        aes_csr.wfo(utra::aes::DATAOUT_3_DATA_3, baz);

        let foo = aes_csr.r(utra::aes::DATAIN_0);
        aes_csr.wo(utra::aes::DATAIN_0, foo);
        let bar = aes_csr.rf(utra::aes::DATAIN_0_DATA_0);
        aes_csr.rmwf(utra::aes::DATAIN_0_DATA_0, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAIN_0_DATA_0, bar);
        baz |= aes_csr.ms(utra::aes::DATAIN_0_DATA_0, 1);
        aes_csr.wfo(utra::aes::DATAIN_0_DATA_0, baz);

        let foo = aes_csr.r(utra::aes::DATAIN_1);
        aes_csr.wo(utra::aes::DATAIN_1, foo);
        let bar = aes_csr.rf(utra::aes::DATAIN_1_DATA_1);
        aes_csr.rmwf(utra::aes::DATAIN_1_DATA_1, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAIN_1_DATA_1, bar);
        baz |= aes_csr.ms(utra::aes::DATAIN_1_DATA_1, 1);
        aes_csr.wfo(utra::aes::DATAIN_1_DATA_1, baz);

        let foo = aes_csr.r(utra::aes::DATAIN_2);
        aes_csr.wo(utra::aes::DATAIN_2, foo);
        let bar = aes_csr.rf(utra::aes::DATAIN_2_DATA_2);
        aes_csr.rmwf(utra::aes::DATAIN_2_DATA_2, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAIN_2_DATA_2, bar);
        baz |= aes_csr.ms(utra::aes::DATAIN_2_DATA_2, 1);
        aes_csr.wfo(utra::aes::DATAIN_2_DATA_2, baz);

        let foo = aes_csr.r(utra::aes::DATAIN_3);
        aes_csr.wo(utra::aes::DATAIN_3, foo);
        let bar = aes_csr.rf(utra::aes::DATAIN_3_DATA_3);
        aes_csr.rmwf(utra::aes::DATAIN_3_DATA_3, bar);
        let mut baz = aes_csr.zf(utra::aes::DATAIN_3_DATA_3, bar);
        baz |= aes_csr.ms(utra::aes::DATAIN_3_DATA_3, 1);
        aes_csr.wfo(utra::aes::DATAIN_3_DATA_3, baz);

        let foo = aes_csr.r(utra::aes::IV_0);
        aes_csr.wo(utra::aes::IV_0, foo);
        let bar = aes_csr.rf(utra::aes::IV_0_IV_0);
        aes_csr.rmwf(utra::aes::IV_0_IV_0, bar);
        let mut baz = aes_csr.zf(utra::aes::IV_0_IV_0, bar);
        baz |= aes_csr.ms(utra::aes::IV_0_IV_0, 1);
        aes_csr.wfo(utra::aes::IV_0_IV_0, baz);

        let foo = aes_csr.r(utra::aes::IV_1);
        aes_csr.wo(utra::aes::IV_1, foo);
        let bar = aes_csr.rf(utra::aes::IV_1_IV_1);
        aes_csr.rmwf(utra::aes::IV_1_IV_1, bar);
        let mut baz = aes_csr.zf(utra::aes::IV_1_IV_1, bar);
        baz |= aes_csr.ms(utra::aes::IV_1_IV_1, 1);
        aes_csr.wfo(utra::aes::IV_1_IV_1, baz);

        let foo = aes_csr.r(utra::aes::IV_2);
        aes_csr.wo(utra::aes::IV_2, foo);
        let bar = aes_csr.rf(utra::aes::IV_2_IV_2);
        aes_csr.rmwf(utra::aes::IV_2_IV_2, bar);
        let mut baz = aes_csr.zf(utra::aes::IV_2_IV_2, bar);
        baz |= aes_csr.ms(utra::aes::IV_2_IV_2, 1);
        aes_csr.wfo(utra::aes::IV_2_IV_2, baz);

        let foo = aes_csr.r(utra::aes::IV_3);
        aes_csr.wo(utra::aes::IV_3, foo);
        let bar = aes_csr.rf(utra::aes::IV_3_IV_3);
        aes_csr.rmwf(utra::aes::IV_3_IV_3, bar);
        let mut baz = aes_csr.zf(utra::aes::IV_3_IV_3, bar);
        baz |= aes_csr.ms(utra::aes::IV_3_IV_3, 1);
        aes_csr.wfo(utra::aes::IV_3_IV_3, baz);

        let foo = aes_csr.r(utra::aes::CTRL);
        aes_csr.wo(utra::aes::CTRL, foo);
        let bar = aes_csr.rf(utra::aes::CTRL_MODE);
        aes_csr.rmwf(utra::aes::CTRL_MODE, bar);
        let mut baz = aes_csr.zf(utra::aes::CTRL_MODE, bar);
        baz |= aes_csr.ms(utra::aes::CTRL_MODE, 1);
        aes_csr.wfo(utra::aes::CTRL_MODE, baz);
        let bar = aes_csr.rf(utra::aes::CTRL_KEY_LEN);
        aes_csr.rmwf(utra::aes::CTRL_KEY_LEN, bar);
        let mut baz = aes_csr.zf(utra::aes::CTRL_KEY_LEN, bar);
        baz |= aes_csr.ms(utra::aes::CTRL_KEY_LEN, 1);
        aes_csr.wfo(utra::aes::CTRL_KEY_LEN, baz);
        let bar = aes_csr.rf(utra::aes::CTRL_MANUAL_OPERATION);
        aes_csr.rmwf(utra::aes::CTRL_MANUAL_OPERATION, bar);
        let mut baz = aes_csr.zf(utra::aes::CTRL_MANUAL_OPERATION, bar);
        baz |= aes_csr.ms(utra::aes::CTRL_MANUAL_OPERATION, 1);
        aes_csr.wfo(utra::aes::CTRL_MANUAL_OPERATION, baz);
        let bar = aes_csr.rf(utra::aes::CTRL_OPERATION);
        aes_csr.rmwf(utra::aes::CTRL_OPERATION, bar);
        let mut baz = aes_csr.zf(utra::aes::CTRL_OPERATION, bar);
        baz |= aes_csr.ms(utra::aes::CTRL_OPERATION, 1);
        aes_csr.wfo(utra::aes::CTRL_OPERATION, baz);

        let foo = aes_csr.r(utra::aes::STATUS);
        aes_csr.wo(utra::aes::STATUS, foo);
        let bar = aes_csr.rf(utra::aes::STATUS_IDLE);
        aes_csr.rmwf(utra::aes::STATUS_IDLE, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_IDLE, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_IDLE, 1);
        aes_csr.wfo(utra::aes::STATUS_IDLE, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_STALL);
        aes_csr.rmwf(utra::aes::STATUS_STALL, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_STALL, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_STALL, 1);
        aes_csr.wfo(utra::aes::STATUS_STALL, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_OUTPUT_VALID);
        aes_csr.rmwf(utra::aes::STATUS_OUTPUT_VALID, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_OUTPUT_VALID, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_OUTPUT_VALID, 1);
        aes_csr.wfo(utra::aes::STATUS_OUTPUT_VALID, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_INPUT_READY);
        aes_csr.rmwf(utra::aes::STATUS_INPUT_READY, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_INPUT_READY, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_INPUT_READY, 1);
        aes_csr.wfo(utra::aes::STATUS_INPUT_READY, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_OPERATION_RBK);
        aes_csr.rmwf(utra::aes::STATUS_OPERATION_RBK, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_OPERATION_RBK, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_OPERATION_RBK, 1);
        aes_csr.wfo(utra::aes::STATUS_OPERATION_RBK, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_MODE_RBK);
        aes_csr.rmwf(utra::aes::STATUS_MODE_RBK, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_MODE_RBK, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_MODE_RBK, 1);
        aes_csr.wfo(utra::aes::STATUS_MODE_RBK, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_KEY_LEN_RBK);
        aes_csr.rmwf(utra::aes::STATUS_KEY_LEN_RBK, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_KEY_LEN_RBK, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_KEY_LEN_RBK, 1);
        aes_csr.wfo(utra::aes::STATUS_KEY_LEN_RBK, baz);
        let bar = aes_csr.rf(utra::aes::STATUS_MANUAL_OPERATION_RBK);
        aes_csr.rmwf(utra::aes::STATUS_MANUAL_OPERATION_RBK, bar);
        let mut baz = aes_csr.zf(utra::aes::STATUS_MANUAL_OPERATION_RBK, bar);
        baz |= aes_csr.ms(utra::aes::STATUS_MANUAL_OPERATION_RBK, 1);
        aes_csr.wfo(utra::aes::STATUS_MANUAL_OPERATION_RBK, baz);

        let foo = aes_csr.r(utra::aes::TRIGGER);
        aes_csr.wo(utra::aes::TRIGGER, foo);
        let bar = aes_csr.rf(utra::aes::TRIGGER_START);
        aes_csr.rmwf(utra::aes::TRIGGER_START, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_START, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_START, 1);
        aes_csr.wfo(utra::aes::TRIGGER_START, baz);
        let bar = aes_csr.rf(utra::aes::TRIGGER_KEY_CLEAR);
        aes_csr.rmwf(utra::aes::TRIGGER_KEY_CLEAR, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_KEY_CLEAR, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_KEY_CLEAR, 1);
        aes_csr.wfo(utra::aes::TRIGGER_KEY_CLEAR, baz);
        let bar = aes_csr.rf(utra::aes::TRIGGER_IV_CLEAR);
        aes_csr.rmwf(utra::aes::TRIGGER_IV_CLEAR, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_IV_CLEAR, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_IV_CLEAR, 1);
        aes_csr.wfo(utra::aes::TRIGGER_IV_CLEAR, baz);
        let bar = aes_csr.rf(utra::aes::TRIGGER_DATA_IN_CLEAR);
        aes_csr.rmwf(utra::aes::TRIGGER_DATA_IN_CLEAR, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_DATA_IN_CLEAR, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_DATA_IN_CLEAR, 1);
        aes_csr.wfo(utra::aes::TRIGGER_DATA_IN_CLEAR, baz);
        let bar = aes_csr.rf(utra::aes::TRIGGER_DATA_OUT_CLEAR);
        aes_csr.rmwf(utra::aes::TRIGGER_DATA_OUT_CLEAR, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_DATA_OUT_CLEAR, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_DATA_OUT_CLEAR, 1);
        aes_csr.wfo(utra::aes::TRIGGER_DATA_OUT_CLEAR, baz);
        let bar = aes_csr.rf(utra::aes::TRIGGER_PRNG_RESEED);
        aes_csr.rmwf(utra::aes::TRIGGER_PRNG_RESEED, bar);
        let mut baz = aes_csr.zf(utra::aes::TRIGGER_PRNG_RESEED, bar);
        baz |= aes_csr.ms(utra::aes::TRIGGER_PRNG_RESEED, 1);
        aes_csr.wfo(utra::aes::TRIGGER_PRNG_RESEED, baz);
        let mut sha2_csr = CSR::new(HW_SHA2_BASE as *mut u32);

        let foo = sha2_csr.r(utra::sha2::KEY0);
        sha2_csr.wo(utra::sha2::KEY0, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY0_KEY0);
        sha2_csr.rmwf(utra::sha2::KEY0_KEY0, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY0_KEY0, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY0_KEY0, 1);
        sha2_csr.wfo(utra::sha2::KEY0_KEY0, baz);

        let foo = sha2_csr.r(utra::sha2::KEY1);
        sha2_csr.wo(utra::sha2::KEY1, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY1_KEY1);
        sha2_csr.rmwf(utra::sha2::KEY1_KEY1, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY1_KEY1, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY1_KEY1, 1);
        sha2_csr.wfo(utra::sha2::KEY1_KEY1, baz);

        let foo = sha2_csr.r(utra::sha2::KEY2);
        sha2_csr.wo(utra::sha2::KEY2, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY2_KEY2);
        sha2_csr.rmwf(utra::sha2::KEY2_KEY2, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY2_KEY2, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY2_KEY2, 1);
        sha2_csr.wfo(utra::sha2::KEY2_KEY2, baz);

        let foo = sha2_csr.r(utra::sha2::KEY3);
        sha2_csr.wo(utra::sha2::KEY3, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY3_KEY3);
        sha2_csr.rmwf(utra::sha2::KEY3_KEY3, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY3_KEY3, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY3_KEY3, 1);
        sha2_csr.wfo(utra::sha2::KEY3_KEY3, baz);

        let foo = sha2_csr.r(utra::sha2::KEY4);
        sha2_csr.wo(utra::sha2::KEY4, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY4_KEY4);
        sha2_csr.rmwf(utra::sha2::KEY4_KEY4, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY4_KEY4, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY4_KEY4, 1);
        sha2_csr.wfo(utra::sha2::KEY4_KEY4, baz);

        let foo = sha2_csr.r(utra::sha2::KEY5);
        sha2_csr.wo(utra::sha2::KEY5, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY5_KEY5);
        sha2_csr.rmwf(utra::sha2::KEY5_KEY5, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY5_KEY5, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY5_KEY5, 1);
        sha2_csr.wfo(utra::sha2::KEY5_KEY5, baz);

        let foo = sha2_csr.r(utra::sha2::KEY6);
        sha2_csr.wo(utra::sha2::KEY6, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY6_KEY6);
        sha2_csr.rmwf(utra::sha2::KEY6_KEY6, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY6_KEY6, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY6_KEY6, 1);
        sha2_csr.wfo(utra::sha2::KEY6_KEY6, baz);

        let foo = sha2_csr.r(utra::sha2::KEY7);
        sha2_csr.wo(utra::sha2::KEY7, foo);
        let bar = sha2_csr.rf(utra::sha2::KEY7_KEY7);
        sha2_csr.rmwf(utra::sha2::KEY7_KEY7, bar);
        let mut baz = sha2_csr.zf(utra::sha2::KEY7_KEY7, bar);
        baz |= sha2_csr.ms(utra::sha2::KEY7_KEY7, 1);
        sha2_csr.wfo(utra::sha2::KEY7_KEY7, baz);

        let foo = sha2_csr.r(utra::sha2::CONFIG);
        sha2_csr.wo(utra::sha2::CONFIG, foo);
        let bar = sha2_csr.rf(utra::sha2::CONFIG_SHA_EN);
        sha2_csr.rmwf(utra::sha2::CONFIG_SHA_EN, bar);
        let mut baz = sha2_csr.zf(utra::sha2::CONFIG_SHA_EN, bar);
        baz |= sha2_csr.ms(utra::sha2::CONFIG_SHA_EN, 1);
        sha2_csr.wfo(utra::sha2::CONFIG_SHA_EN, baz);
        let bar = sha2_csr.rf(utra::sha2::CONFIG_ENDIAN_SWAP);
        sha2_csr.rmwf(utra::sha2::CONFIG_ENDIAN_SWAP, bar);
        let mut baz = sha2_csr.zf(utra::sha2::CONFIG_ENDIAN_SWAP, bar);
        baz |= sha2_csr.ms(utra::sha2::CONFIG_ENDIAN_SWAP, 1);
        sha2_csr.wfo(utra::sha2::CONFIG_ENDIAN_SWAP, baz);
        let bar = sha2_csr.rf(utra::sha2::CONFIG_DIGEST_SWAP);
        sha2_csr.rmwf(utra::sha2::CONFIG_DIGEST_SWAP, bar);
        let mut baz = sha2_csr.zf(utra::sha2::CONFIG_DIGEST_SWAP, bar);
        baz |= sha2_csr.ms(utra::sha2::CONFIG_DIGEST_SWAP, 1);
        sha2_csr.wfo(utra::sha2::CONFIG_DIGEST_SWAP, baz);
        let bar = sha2_csr.rf(utra::sha2::CONFIG_HMAC_EN);
        sha2_csr.rmwf(utra::sha2::CONFIG_HMAC_EN, bar);
        let mut baz = sha2_csr.zf(utra::sha2::CONFIG_HMAC_EN, bar);
        baz |= sha2_csr.ms(utra::sha2::CONFIG_HMAC_EN, 1);
        sha2_csr.wfo(utra::sha2::CONFIG_HMAC_EN, baz);

        let foo = sha2_csr.r(utra::sha2::COMMAND);
        sha2_csr.wo(utra::sha2::COMMAND, foo);
        let bar = sha2_csr.rf(utra::sha2::COMMAND_HASH_START);
        sha2_csr.rmwf(utra::sha2::COMMAND_HASH_START, bar);
        let mut baz = sha2_csr.zf(utra::sha2::COMMAND_HASH_START, bar);
        baz |= sha2_csr.ms(utra::sha2::COMMAND_HASH_START, 1);
        sha2_csr.wfo(utra::sha2::COMMAND_HASH_START, baz);
        let bar = sha2_csr.rf(utra::sha2::COMMAND_HASH_PROCESS);
        sha2_csr.rmwf(utra::sha2::COMMAND_HASH_PROCESS, bar);
        let mut baz = sha2_csr.zf(utra::sha2::COMMAND_HASH_PROCESS, bar);
        baz |= sha2_csr.ms(utra::sha2::COMMAND_HASH_PROCESS, 1);
        sha2_csr.wfo(utra::sha2::COMMAND_HASH_PROCESS, baz);

        let foo = sha2_csr.r(utra::sha2::WIPE);
        sha2_csr.wo(utra::sha2::WIPE, foo);
        let bar = sha2_csr.rf(utra::sha2::WIPE_WIPE);
        sha2_csr.rmwf(utra::sha2::WIPE_WIPE, bar);
        let mut baz = sha2_csr.zf(utra::sha2::WIPE_WIPE, bar);
        baz |= sha2_csr.ms(utra::sha2::WIPE_WIPE, 1);
        sha2_csr.wfo(utra::sha2::WIPE_WIPE, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST0);
        sha2_csr.wo(utra::sha2::DIGEST0, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST0_DIGEST0);
        sha2_csr.rmwf(utra::sha2::DIGEST0_DIGEST0, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST0_DIGEST0, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST0_DIGEST0, 1);
        sha2_csr.wfo(utra::sha2::DIGEST0_DIGEST0, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST1);
        sha2_csr.wo(utra::sha2::DIGEST1, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST1_DIGEST1);
        sha2_csr.rmwf(utra::sha2::DIGEST1_DIGEST1, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST1_DIGEST1, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST1_DIGEST1, 1);
        sha2_csr.wfo(utra::sha2::DIGEST1_DIGEST1, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST2);
        sha2_csr.wo(utra::sha2::DIGEST2, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST2_DIGEST2);
        sha2_csr.rmwf(utra::sha2::DIGEST2_DIGEST2, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST2_DIGEST2, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST2_DIGEST2, 1);
        sha2_csr.wfo(utra::sha2::DIGEST2_DIGEST2, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST3);
        sha2_csr.wo(utra::sha2::DIGEST3, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST3_DIGEST3);
        sha2_csr.rmwf(utra::sha2::DIGEST3_DIGEST3, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST3_DIGEST3, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST3_DIGEST3, 1);
        sha2_csr.wfo(utra::sha2::DIGEST3_DIGEST3, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST4);
        sha2_csr.wo(utra::sha2::DIGEST4, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST4_DIGEST4);
        sha2_csr.rmwf(utra::sha2::DIGEST4_DIGEST4, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST4_DIGEST4, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST4_DIGEST4, 1);
        sha2_csr.wfo(utra::sha2::DIGEST4_DIGEST4, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST5);
        sha2_csr.wo(utra::sha2::DIGEST5, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST5_DIGEST5);
        sha2_csr.rmwf(utra::sha2::DIGEST5_DIGEST5, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST5_DIGEST5, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST5_DIGEST5, 1);
        sha2_csr.wfo(utra::sha2::DIGEST5_DIGEST5, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST6);
        sha2_csr.wo(utra::sha2::DIGEST6, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST6_DIGEST6);
        sha2_csr.rmwf(utra::sha2::DIGEST6_DIGEST6, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST6_DIGEST6, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST6_DIGEST6, 1);
        sha2_csr.wfo(utra::sha2::DIGEST6_DIGEST6, baz);

        let foo = sha2_csr.r(utra::sha2::DIGEST7);
        sha2_csr.wo(utra::sha2::DIGEST7, foo);
        let bar = sha2_csr.rf(utra::sha2::DIGEST7_DIGEST7);
        sha2_csr.rmwf(utra::sha2::DIGEST7_DIGEST7, bar);
        let mut baz = sha2_csr.zf(utra::sha2::DIGEST7_DIGEST7, bar);
        baz |= sha2_csr.ms(utra::sha2::DIGEST7_DIGEST7, 1);
        sha2_csr.wfo(utra::sha2::DIGEST7_DIGEST7, baz);

        let foo = sha2_csr.r(utra::sha2::MSG_LENGTH1);
        sha2_csr.wo(utra::sha2::MSG_LENGTH1, foo);
        let bar = sha2_csr.rf(utra::sha2::MSG_LENGTH1_MSG_LENGTH);
        sha2_csr.rmwf(utra::sha2::MSG_LENGTH1_MSG_LENGTH, bar);
        let mut baz = sha2_csr.zf(utra::sha2::MSG_LENGTH1_MSG_LENGTH, bar);
        baz |= sha2_csr.ms(utra::sha2::MSG_LENGTH1_MSG_LENGTH, 1);
        sha2_csr.wfo(utra::sha2::MSG_LENGTH1_MSG_LENGTH, baz);

        let foo = sha2_csr.r(utra::sha2::MSG_LENGTH0);
        sha2_csr.wo(utra::sha2::MSG_LENGTH0, foo);
        let bar = sha2_csr.rf(utra::sha2::MSG_LENGTH0_MSG_LENGTH);
        sha2_csr.rmwf(utra::sha2::MSG_LENGTH0_MSG_LENGTH, bar);
        let mut baz = sha2_csr.zf(utra::sha2::MSG_LENGTH0_MSG_LENGTH, bar);
        baz |= sha2_csr.ms(utra::sha2::MSG_LENGTH0_MSG_LENGTH, 1);
        sha2_csr.wfo(utra::sha2::MSG_LENGTH0_MSG_LENGTH, baz);

        let foo = sha2_csr.r(utra::sha2::ERROR_CODE);
        sha2_csr.wo(utra::sha2::ERROR_CODE, foo);
        let bar = sha2_csr.rf(utra::sha2::ERROR_CODE_ERROR_CODE);
        sha2_csr.rmwf(utra::sha2::ERROR_CODE_ERROR_CODE, bar);
        let mut baz = sha2_csr.zf(utra::sha2::ERROR_CODE_ERROR_CODE, bar);
        baz |= sha2_csr.ms(utra::sha2::ERROR_CODE_ERROR_CODE, 1);
        sha2_csr.wfo(utra::sha2::ERROR_CODE_ERROR_CODE, baz);

        let foo = sha2_csr.r(utra::sha2::EV_STATUS);
        sha2_csr.wo(utra::sha2::EV_STATUS, foo);
        let bar = sha2_csr.rf(utra::sha2::EV_STATUS_STATUS);
        sha2_csr.rmwf(utra::sha2::EV_STATUS_STATUS, bar);
        let mut baz = sha2_csr.zf(utra::sha2::EV_STATUS_STATUS, bar);
        baz |= sha2_csr.ms(utra::sha2::EV_STATUS_STATUS, 1);
        sha2_csr.wfo(utra::sha2::EV_STATUS_STATUS, baz);

        let foo = sha2_csr.r(utra::sha2::EV_PENDING);
        sha2_csr.wo(utra::sha2::EV_PENDING, foo);
        let bar = sha2_csr.rf(utra::sha2::EV_PENDING_PENDING);
        sha2_csr.rmwf(utra::sha2::EV_PENDING_PENDING, bar);
        let mut baz = sha2_csr.zf(utra::sha2::EV_PENDING_PENDING, bar);
        baz |= sha2_csr.ms(utra::sha2::EV_PENDING_PENDING, 1);
        sha2_csr.wfo(utra::sha2::EV_PENDING_PENDING, baz);

        let foo = sha2_csr.r(utra::sha2::EV_ENABLE);
        sha2_csr.wo(utra::sha2::EV_ENABLE, foo);
        let bar = sha2_csr.rf(utra::sha2::EV_ENABLE_ENABLE);
        sha2_csr.rmwf(utra::sha2::EV_ENABLE_ENABLE, bar);
        let mut baz = sha2_csr.zf(utra::sha2::EV_ENABLE_ENABLE, bar);
        baz |= sha2_csr.ms(utra::sha2::EV_ENABLE_ENABLE, 1);
        sha2_csr.wfo(utra::sha2::EV_ENABLE_ENABLE, baz);

        let foo = sha2_csr.r(utra::sha2::FIFO);
        sha2_csr.wo(utra::sha2::FIFO, foo);
        let bar = sha2_csr.rf(utra::sha2::FIFO_READ_COUNT);
        sha2_csr.rmwf(utra::sha2::FIFO_READ_COUNT, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_READ_COUNT, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_READ_COUNT, 1);
        sha2_csr.wfo(utra::sha2::FIFO_READ_COUNT, baz);
        let bar = sha2_csr.rf(utra::sha2::FIFO_WRITE_COUNT);
        sha2_csr.rmwf(utra::sha2::FIFO_WRITE_COUNT, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_WRITE_COUNT, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_WRITE_COUNT, 1);
        sha2_csr.wfo(utra::sha2::FIFO_WRITE_COUNT, baz);
        let bar = sha2_csr.rf(utra::sha2::FIFO_READ_ERROR);
        sha2_csr.rmwf(utra::sha2::FIFO_READ_ERROR, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_READ_ERROR, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_READ_ERROR, 1);
        sha2_csr.wfo(utra::sha2::FIFO_READ_ERROR, baz);
        let bar = sha2_csr.rf(utra::sha2::FIFO_WRITE_ERROR);
        sha2_csr.rmwf(utra::sha2::FIFO_WRITE_ERROR, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_WRITE_ERROR, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_WRITE_ERROR, 1);
        sha2_csr.wfo(utra::sha2::FIFO_WRITE_ERROR, baz);
        let bar = sha2_csr.rf(utra::sha2::FIFO_ALMOST_FULL);
        sha2_csr.rmwf(utra::sha2::FIFO_ALMOST_FULL, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_ALMOST_FULL, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_ALMOST_FULL, 1);
        sha2_csr.wfo(utra::sha2::FIFO_ALMOST_FULL, baz);
        let bar = sha2_csr.rf(utra::sha2::FIFO_ALMOST_EMPTY);
        sha2_csr.rmwf(utra::sha2::FIFO_ALMOST_EMPTY, bar);
        let mut baz = sha2_csr.zf(utra::sha2::FIFO_ALMOST_EMPTY, bar);
        baz |= sha2_csr.ms(utra::sha2::FIFO_ALMOST_EMPTY, 1);
        sha2_csr.wfo(utra::sha2::FIFO_ALMOST_EMPTY, baz);
        let mut sha512_csr = CSR::new(HW_SHA512_BASE as *mut u32);

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
        let bar = sha512_csr.rf(utra::sha512::EV_STATUS_STATUS);
        sha512_csr.rmwf(utra::sha512::EV_STATUS_STATUS, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_STATUS_STATUS, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_STATUS_STATUS, 1);
        sha512_csr.wfo(utra::sha512::EV_STATUS_STATUS, baz);

        let foo = sha512_csr.r(utra::sha512::EV_PENDING);
        sha512_csr.wo(utra::sha512::EV_PENDING, foo);
        let bar = sha512_csr.rf(utra::sha512::EV_PENDING_PENDING);
        sha512_csr.rmwf(utra::sha512::EV_PENDING_PENDING, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_PENDING_PENDING, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_PENDING_PENDING, 1);
        sha512_csr.wfo(utra::sha512::EV_PENDING_PENDING, baz);

        let foo = sha512_csr.r(utra::sha512::EV_ENABLE);
        sha512_csr.wo(utra::sha512::EV_ENABLE, foo);
        let bar = sha512_csr.rf(utra::sha512::EV_ENABLE_ENABLE);
        sha512_csr.rmwf(utra::sha512::EV_ENABLE_ENABLE, bar);
        let mut baz = sha512_csr.zf(utra::sha512::EV_ENABLE_ENABLE, bar);
        baz |= sha512_csr.ms(utra::sha512::EV_ENABLE_ENABLE, 1);
        sha512_csr.wfo(utra::sha512::EV_ENABLE_ENABLE, baz);

        let foo = sha512_csr.r(utra::sha512::FIFO);
        sha512_csr.wo(utra::sha512::FIFO, foo);
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

        let foo = engine_csr.r(utra::engine::EV_STATUS);
        engine_csr.wo(utra::engine::EV_STATUS, foo);
        let bar = engine_csr.rf(utra::engine::EV_STATUS_STATUS);
        engine_csr.rmwf(utra::engine::EV_STATUS_STATUS, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_STATUS_STATUS, bar);
        baz |= engine_csr.ms(utra::engine::EV_STATUS_STATUS, 1);
        engine_csr.wfo(utra::engine::EV_STATUS_STATUS, baz);

        let foo = engine_csr.r(utra::engine::EV_PENDING);
        engine_csr.wo(utra::engine::EV_PENDING, foo);
        let bar = engine_csr.rf(utra::engine::EV_PENDING_PENDING);
        engine_csr.rmwf(utra::engine::EV_PENDING_PENDING, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_PENDING_PENDING, bar);
        baz |= engine_csr.ms(utra::engine::EV_PENDING_PENDING, 1);
        engine_csr.wfo(utra::engine::EV_PENDING_PENDING, baz);

        let foo = engine_csr.r(utra::engine::EV_ENABLE);
        engine_csr.wo(utra::engine::EV_ENABLE, foo);
        let bar = engine_csr.rf(utra::engine::EV_ENABLE_ENABLE);
        engine_csr.rmwf(utra::engine::EV_ENABLE_ENABLE, bar);
        let mut baz = engine_csr.zf(utra::engine::EV_ENABLE_ENABLE, bar);
        baz |= engine_csr.ms(utra::engine::EV_ENABLE_ENABLE, 1);
        engine_csr.wfo(utra::engine::EV_ENABLE_ENABLE, baz);

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
}
