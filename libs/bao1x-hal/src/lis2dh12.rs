//! LIS2DH12 3-axis accelerometer driver for Xous
//!
//! This driver provides an interface to the ST LIS2DH12TR ultra-low-power
//! high-performance 3-axis accelerometer over I2C.
//!
//! # Features
//! - Orientation detection (face up/down)
//! - Motion/tap interrupt configuration
//! - Configurable full-scale range (±2g to ±16g)
//! - Multiple operating modes (low-power, normal, high-resolution)

use std::thread;
use std::time::Duration;

use bao1x_api::{I2cApi, I2cResult};

/// I2C device address with SA0 tied high (0b0011001)
pub const LIS2DH12_ADDR: u8 = 0x19;

/// Expected WHO_AM_I register value
pub const WHO_AM_I_VALUE: u8 = 0x33;

// =============================================================================
// I2C Result Extension Trait
// =============================================================================

/// Extension trait for clean I2C result handling with the `?` operator
pub trait I2cResultExt {
    /// Convert I2C result to a simple Result, discarding the byte count on success
    fn check(self) -> Result<(), xous::Error>;
}

impl I2cResultExt for Result<I2cResult, xous::Error> {
    fn check(self) -> Result<(), xous::Error> {
        match self? {
            I2cResult::Ack(_) => Ok(()),
            I2cResult::Nack | I2cResult::Pending => Err(xous::Error::Timeout),
            I2cResult::InternalError => Err(xous::Error::InternalError),
        }
    }
}

// =============================================================================
// Register Addresses
// =============================================================================

/// Register addresses for LIS2DH12
#[allow(dead_code)]
pub mod regs {
    pub const STATUS_REG_AUX: u8 = 0x07;
    pub const OUT_TEMP_L: u8 = 0x0C;
    pub const OUT_TEMP_H: u8 = 0x0D;
    pub const WHO_AM_I: u8 = 0x0F;
    pub const CTRL_REG0: u8 = 0x1E;
    pub const TEMP_CFG_REG: u8 = 0x1F;
    pub const CTRL_REG1: u8 = 0x20;
    pub const CTRL_REG2: u8 = 0x21;
    pub const CTRL_REG3: u8 = 0x22;
    pub const CTRL_REG4: u8 = 0x23;
    pub const CTRL_REG5: u8 = 0x24;
    pub const CTRL_REG6: u8 = 0x25;
    pub const REFERENCE: u8 = 0x26;
    pub const STATUS_REG: u8 = 0x27;
    pub const OUT_X_L: u8 = 0x28;
    pub const OUT_X_H: u8 = 0x29;
    pub const OUT_Y_L: u8 = 0x2A;
    pub const OUT_Y_H: u8 = 0x2B;
    pub const OUT_Z_L: u8 = 0x2C;
    pub const OUT_Z_H: u8 = 0x2D;
    pub const FIFO_CTRL_REG: u8 = 0x2E;
    pub const FIFO_SRC_REG: u8 = 0x2F;
    pub const INT1_CFG: u8 = 0x30;
    pub const INT1_SRC: u8 = 0x31;
    pub const INT1_THS: u8 = 0x32;
    pub const INT1_DURATION: u8 = 0x33;
    pub const INT2_CFG: u8 = 0x34;
    pub const INT2_SRC: u8 = 0x35;
    pub const INT2_THS: u8 = 0x36;
    pub const INT2_DURATION: u8 = 0x37;
    pub const CLICK_CFG: u8 = 0x38;
    pub const CLICK_SRC: u8 = 0x39;
    pub const CLICK_THS: u8 = 0x3A;
    pub const TIME_LIMIT: u8 = 0x3B;
    pub const TIME_LATENCY: u8 = 0x3C;
    pub const TIME_WINDOW: u8 = 0x3D;
    pub const ACT_THS: u8 = 0x3E;
    pub const ACT_DUR: u8 = 0x3F;

    /// Flag to set MSB for auto-increment in multi-byte reads
    pub const AUTO_INCREMENT: u8 = 0x80;
}

// =============================================================================
// Configuration Enums
// =============================================================================

/// Output data rate selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DataRate {
    /// Power-down mode
    #[default]
    PowerDown = 0b0000,
    /// 1 Hz
    Hz1 = 0b0001,
    /// 10 Hz
    Hz10 = 0b0010,
    /// 25 Hz
    Hz25 = 0b0011,
    /// 50 Hz
    Hz50 = 0b0100,
    /// 100 Hz
    Hz100 = 0b0101,
    /// 200 Hz
    Hz200 = 0b0110,
    /// 400 Hz
    Hz400 = 0b0111,
    /// 1.620 kHz (low-power mode only)
    Hz1620LowPower = 0b1000,
    /// 1.344 kHz (HR/Normal) or 5.376 kHz (low-power)
    Hz1344OrHz5376 = 0b1001,
}

impl From<DataRate> for u8 {
    fn from(dr: DataRate) -> u8 { dr as u8 }
}

impl TryFrom<u8> for DataRate {
    type Error = xous::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x0F {
            0b0000 => Ok(DataRate::PowerDown),
            0b0001 => Ok(DataRate::Hz1),
            0b0010 => Ok(DataRate::Hz10),
            0b0011 => Ok(DataRate::Hz25),
            0b0100 => Ok(DataRate::Hz50),
            0b0101 => Ok(DataRate::Hz100),
            0b0110 => Ok(DataRate::Hz200),
            0b0111 => Ok(DataRate::Hz400),
            0b1000 => Ok(DataRate::Hz1620LowPower),
            0b1001 => Ok(DataRate::Hz1344OrHz5376),
            _ => Err(xous::Error::InvalidCoding),
        }
    }
}

/// Full-scale selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FullScale {
    /// ±2g
    #[default]
    G2 = 0b00,
    /// ±4g
    G4 = 0b01,
    /// ±8g
    G8 = 0b10,
    /// ±16g
    G16 = 0b11,
}

impl From<FullScale> for u8 {
    fn from(fs: FullScale) -> u8 { fs as u8 }
}

impl TryFrom<u8> for FullScale {
    type Error = xous::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0b00 => Ok(FullScale::G2),
            0b01 => Ok(FullScale::G4),
            0b10 => Ok(FullScale::G8),
            0b11 => Ok(FullScale::G16),
            _ => Err(xous::Error::InvalidCoding),
        }
    }
}

impl FullScale {
    /// Get the sensitivity in mg per digit for the given operating mode
    pub fn sensitivity_mg(&self, mode: OperatingMode) -> u16 {
        match (self, mode) {
            (FullScale::G2, OperatingMode::HighResolution) => 1,
            (FullScale::G2, OperatingMode::Normal) => 4,
            (FullScale::G2, OperatingMode::LowPower) => 16,
            (FullScale::G4, OperatingMode::HighResolution) => 2,
            (FullScale::G4, OperatingMode::Normal) => 8,
            (FullScale::G4, OperatingMode::LowPower) => 32,
            (FullScale::G8, OperatingMode::HighResolution) => 4,
            (FullScale::G8, OperatingMode::Normal) => 16,
            (FullScale::G8, OperatingMode::LowPower) => 64,
            (FullScale::G16, OperatingMode::HighResolution) => 12,
            (FullScale::G16, OperatingMode::Normal) => 48,
            (FullScale::G16, OperatingMode::LowPower) => 192,
        }
    }

    /// Get the threshold LSB value in mg for interrupt thresholds
    pub fn threshold_mg_per_lsb(&self) -> u16 {
        match self {
            FullScale::G2 => 16,
            FullScale::G4 => 32,
            FullScale::G8 => 62,
            FullScale::G16 => 186,
        }
    }
}

/// Operating mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperatingMode {
    /// Low-power mode (8-bit data output)
    LowPower,
    /// Normal mode (10-bit data output)
    #[default]
    Normal,
    /// High-resolution mode (12-bit data output)
    HighResolution,
}

impl OperatingMode {
    /// Returns (LPen bit, HR bit) values for CTRL_REG1 and CTRL_REG4
    pub fn to_bits(self) -> (bool, bool) {
        match self {
            OperatingMode::LowPower => (true, false),
            OperatingMode::Normal => (false, false),
            OperatingMode::HighResolution => (false, true),
        }
    }

    /// Create from LPen and HR bit values
    pub fn from_bits(lpen: bool, hr: bool) -> Option<Self> {
        match (lpen, hr) {
            (true, false) => Some(OperatingMode::LowPower),
            (false, false) => Some(OperatingMode::Normal),
            (false, true) => Some(OperatingMode::HighResolution),
            (true, true) => None, // Invalid combination
        }
    }

    /// Number of valid bits in the acceleration data
    pub fn data_bits(self) -> u8 {
        match self {
            OperatingMode::LowPower => 8,
            OperatingMode::Normal => 10,
            OperatingMode::HighResolution => 12,
        }
    }
}

/// High-pass filter mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum HighPassFilterMode {
    /// Normal mode (reset by reading REFERENCE register)
    #[default]
    NormalWithReset = 0b00,
    /// Reference signal for filtering
    Reference = 0b01,
    /// Normal mode
    Normal = 0b10,
    /// Autoreset on interrupt event
    AutoresetOnInterrupt = 0b11,
}

impl From<HighPassFilterMode> for u8 {
    fn from(mode: HighPassFilterMode) -> u8 { mode as u8 }
}

impl TryFrom<u8> for HighPassFilterMode {
    type Error = xous::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0b00 => Ok(HighPassFilterMode::NormalWithReset),
            0b01 => Ok(HighPassFilterMode::Reference),
            0b10 => Ok(HighPassFilterMode::Normal),
            0b11 => Ok(HighPassFilterMode::AutoresetOnInterrupt),
            _ => Err(xous::Error::InvalidCoding),
        }
    }
}

/// FIFO mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FifoMode {
    /// Bypass mode (FIFO disabled)
    #[default]
    Bypass = 0b00,
    /// FIFO mode
    Fifo = 0b01,
    /// Stream mode
    Stream = 0b10,
    /// Stream-to-FIFO mode
    StreamToFifo = 0b11,
}

impl From<FifoMode> for u8 {
    fn from(mode: FifoMode) -> u8 { mode as u8 }
}

impl TryFrom<u8> for FifoMode {
    type Error = xous::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0b00 => Ok(FifoMode::Bypass),
            0b01 => Ok(FifoMode::Fifo),
            0b10 => Ok(FifoMode::Stream),
            0b11 => Ok(FifoMode::StreamToFifo),
            _ => Err(xous::Error::InvalidCoding),
        }
    }
}

/// Interrupt mode for INT1/INT2 configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterruptMode {
    /// OR combination of interrupt events
    #[default]
    OrCombination,
    /// 6-direction movement recognition
    Movement6D,
    /// AND combination of interrupt events
    AndCombination,
    /// 6-direction position recognition
    Position6D,
}

impl InterruptMode {
    /// Returns (AOI bit, 6D bit) values
    pub fn to_bits(self) -> (bool, bool) {
        match self {
            InterruptMode::OrCombination => (false, false),
            InterruptMode::Movement6D => (false, true),
            InterruptMode::AndCombination => (true, false),
            InterruptMode::Position6D => (true, true),
        }
    }

    /// Create from AOI and 6D bit values
    pub fn from_bits(aoi: bool, d6: bool) -> Self {
        match (aoi, d6) {
            (false, false) => InterruptMode::OrCombination,
            (false, true) => InterruptMode::Movement6D,
            (true, false) => InterruptMode::AndCombination,
            (true, true) => InterruptMode::Position6D,
        }
    }
}

/// Interrupt polarity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterruptPolarity {
    /// Active high
    #[default]
    ActiveHigh,
    /// Active low
    ActiveLow,
}

/// Self-test mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SelfTestMode {
    /// Normal mode (self-test disabled)
    #[default]
    Disabled = 0b00,
    /// Self-test 0
    SelfTest0 = 0b01,
    /// Self-test 1
    SelfTest1 = 0b10,
}

impl From<SelfTestMode> for u8 {
    fn from(mode: SelfTestMode) -> u8 { mode as u8 }
}

impl TryFrom<u8> for SelfTestMode {
    type Error = xous::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0b00 => Ok(SelfTestMode::Disabled),
            0b01 => Ok(SelfTestMode::SelfTest0),
            0b10 => Ok(SelfTestMode::SelfTest1),
            _ => Err(xous::Error::InvalidCoding),
        }
    }
}

/// Debug dump of interrupt-related registers
#[derive(Debug, Clone, Copy)]
pub struct IntDebugDump {
    pub ctrl_reg1: u8,
    pub ctrl_reg2: u8,
    pub ctrl_reg3: u8,
    pub ctrl_reg4: u8,
    pub ctrl_reg5: u8,
    pub ctrl_reg6: u8,
    pub int1_cfg: u8,
    pub int1_ths: u8,
    pub int1_duration: u8,
    pub int1_src: u8,
    pub status_reg: u8,
}

impl IntDebugDump {
    /// Print a human-readable interpretation of the register values
    pub fn interpret(&self) {
        log::info!("=== LIS2DH12 Interrupt Debug Dump ===");

        // CTRL_REG1: ODR, LPen, axis enables
        let odr = (self.ctrl_reg1 >> 4) & 0x0F;
        let lpen = (self.ctrl_reg1 & 0x08) != 0;
        let zen = (self.ctrl_reg1 & 0x04) != 0;
        let yen = (self.ctrl_reg1 & 0x02) != 0;
        let xen = (self.ctrl_reg1 & 0x01) != 0;
        log::info!(
            "CTRL_REG1 (0x{:02X}): ODR={}, LPen={}, Z={}, Y={}, X={}",
            self.ctrl_reg1,
            odr,
            lpen,
            zen,
            yen,
            xen
        );

        // CTRL_REG2: High-pass filter settings
        let hpm = (self.ctrl_reg2 >> 6) & 0x03;
        let hpcf = (self.ctrl_reg2 >> 4) & 0x03;
        let fds = (self.ctrl_reg2 & 0x08) != 0;
        let hp_ia1 = (self.ctrl_reg2 & 0x01) != 0;
        log::info!(
            "CTRL_REG2 (0x{:02X}): HPM={}, HPCF={}, FDS={}, HP_IA1={}",
            self.ctrl_reg2,
            hpm,
            hpcf,
            fds,
            hp_ia1
        );

        // CTRL_REG3: Interrupt enables on INT1
        let i1_click = (self.ctrl_reg3 & 0x80) != 0;
        let i1_ia1 = (self.ctrl_reg3 & 0x40) != 0;
        let i1_ia2 = (self.ctrl_reg3 & 0x20) != 0;
        let i1_zyxda = (self.ctrl_reg3 & 0x10) != 0;
        log::info!(
            "CTRL_REG3 (0x{:02X}): I1_CLICK={}, I1_IA1={}, I1_IA2={}, I1_ZYXDA={}",
            self.ctrl_reg3,
            i1_click,
            i1_ia1,
            i1_ia2,
            i1_zyxda
        );
        if !i1_ia1 {
            log::info!("  WARNING: I1_IA1 not set - INT1 generator not routed to INT1 pin!");
        }

        // CTRL_REG4: BDU, FS, HR
        let bdu = (self.ctrl_reg4 & 0x80) != 0;
        let fs = (self.ctrl_reg4 >> 4) & 0x03;
        let hr = (self.ctrl_reg4 & 0x08) != 0;
        log::info!("CTRL_REG4 (0x{:02X}): BDU={}, FS={}, HR={}", self.ctrl_reg4, bdu, fs, hr);

        // CTRL_REG5: Latch settings
        let lir_int1 = (self.ctrl_reg5 & 0x08) != 0;
        let d4d_int1 = (self.ctrl_reg5 & 0x04) != 0;
        log::info!("CTRL_REG5 (0x{:02X}): LIR_INT1={}, D4D_INT1={}", self.ctrl_reg5, lir_int1, d4d_int1);

        // CTRL_REG6: INT polarity
        let int_polarity = (self.ctrl_reg6 & 0x02) != 0;
        log::info!(
            "CTRL_REG6 (0x{:02X}): INT_POLARITY={} ({})",
            self.ctrl_reg6,
            int_polarity,
            if int_polarity { "active-low" } else { "active-high" }
        );

        // INT1_CFG: Interrupt configuration
        let aoi = (self.int1_cfg & 0x80) != 0;
        let d6 = (self.int1_cfg & 0x40) != 0;
        let zhie = (self.int1_cfg & 0x20) != 0;
        let zlie = (self.int1_cfg & 0x10) != 0;
        let yhie = (self.int1_cfg & 0x08) != 0;
        let ylie = (self.int1_cfg & 0x04) != 0;
        let xhie = (self.int1_cfg & 0x02) != 0;
        let xlie = (self.int1_cfg & 0x01) != 0;
        log::info!(
            "INT1_CFG (0x{:02X}): AOI={}, 6D={}, ZH={}, ZL={}, YH={}, YL={}, XH={}, XL={}",
            self.int1_cfg,
            aoi,
            d6,
            zhie,
            zlie,
            yhie,
            ylie,
            xhie,
            xlie
        );
        if self.int1_cfg == 0 {
            log::info!("  WARNING: INT1_CFG is 0 - no interrupt events enabled!");
        }

        // INT1_THS: Threshold
        let ths = self.int1_ths & 0x7F;
        log::info!("INT1_THS (0x{:02X}): threshold={} ({}mg at ±2g)", self.int1_ths, ths, ths as u16 * 16);

        // INT1_DURATION
        let dur = self.int1_duration & 0x7F;
        log::info!("INT1_DURATION (0x{:02X}): duration={} samples", dur, dur);

        // INT1_SRC: Current interrupt status (reading this clears latched interrupt!)
        let ia = (self.int1_src & 0x40) != 0;
        let zh = (self.int1_src & 0x20) != 0;
        let zl = (self.int1_src & 0x10) != 0;
        let yh = (self.int1_src & 0x08) != 0;
        let yl = (self.int1_src & 0x04) != 0;
        let xh = (self.int1_src & 0x02) != 0;
        let xl = (self.int1_src & 0x01) != 0;
        log::info!(
            "INT1_SRC (0x{:02X}): IA={}, ZH={}, ZL={}, YH={}, YL={}, XH={}, XL={}",
            self.int1_src,
            ia,
            zh,
            zl,
            yh,
            yl,
            xh,
            xl
        );

        // STATUS_REG
        let zyxda = (self.status_reg & 0x08) != 0;
        log::info!("STATUS_REG (0x{:02X}): ZYXDA={}", self.status_reg, zyxda);

        log::info!("=====================================");
    }
}

// =============================================================================
// Data Structures
// =============================================================================

/// Raw acceleration data from all three axes
#[derive(Debug, Clone, Copy, Default)]
pub struct AccelData {
    /// X-axis raw value (left-justified two's complement)
    pub x: i16,
    /// Y-axis raw value (left-justified two's complement)
    pub y: i16,
    /// Z-axis raw value (left-justified two's complement)
    pub z: i16,
}

impl AccelData {
    /// Convert raw data to milli-g values based on full scale and operating mode
    pub fn to_mg(&self, full_scale: FullScale, mode: OperatingMode) -> (i32, i32, i32) {
        let sensitivity = full_scale.sensitivity_mg(mode) as i32;
        let shift = 16 - mode.data_bits();

        // Data is left-justified, so we need to shift right to get the actual value
        let x = ((self.x >> shift) as i32) * sensitivity;
        let y = ((self.y >> shift) as i32) * sensitivity;
        let z = ((self.z >> shift) as i32) * sensitivity;

        (x, y, z)
    }
}

/// Device orientation based on acceleration readings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Face up (Z positive, towards sky)
    FaceUp,
    /// Face down (Z negative, towards ground)
    FaceDown,
    /// Unknown or transitional orientation
    Unknown,
}

/// Interrupt source flags
#[derive(Debug, Clone, Copy, Default)]
pub struct InterruptSource {
    /// Interrupt active
    pub active: bool,
    /// X high event occurred
    pub x_high: bool,
    /// X low event occurred
    pub x_low: bool,
    /// Y high event occurred
    pub y_high: bool,
    /// Y low event occurred
    pub y_low: bool,
    /// Z high event occurred
    pub z_high: bool,
    /// Z low event occurred
    pub z_low: bool,
}

impl From<u8> for InterruptSource {
    fn from(value: u8) -> Self {
        InterruptSource {
            active: (value & 0x40) != 0,
            z_high: (value & 0x20) != 0,
            z_low: (value & 0x10) != 0,
            y_high: (value & 0x08) != 0,
            y_low: (value & 0x04) != 0,
            x_high: (value & 0x02) != 0,
            x_low: (value & 0x01) != 0,
        }
    }
}

/// Click source flags
#[derive(Debug, Clone, Copy, Default)]
pub struct ClickSource {
    /// Interrupt active
    pub active: bool,
    /// Double-click detected
    pub double_click: bool,
    /// Single-click detected
    pub single_click: bool,
    /// Click sign (true = negative)
    pub negative: bool,
    /// Z-axis click detected
    pub z: bool,
    /// Y-axis click detected
    pub y: bool,
    /// X-axis click detected
    pub x: bool,
}

impl From<u8> for ClickSource {
    fn from(value: u8) -> Self {
        ClickSource {
            active: (value & 0x40) != 0,
            double_click: (value & 0x20) != 0,
            single_click: (value & 0x10) != 0,
            negative: (value & 0x08) != 0,
            z: (value & 0x04) != 0,
            y: (value & 0x02) != 0,
            x: (value & 0x01) != 0,
        }
    }
}

/// Status register flags
#[derive(Debug, Clone, Copy, Default)]
pub struct Status {
    /// X, Y, Z data overrun
    pub xyz_overrun: bool,
    /// Z data overrun
    pub z_overrun: bool,
    /// Y data overrun
    pub y_overrun: bool,
    /// X data overrun
    pub x_overrun: bool,
    /// X, Y, Z new data available
    pub xyz_data_available: bool,
    /// Z new data available
    pub z_data_available: bool,
    /// Y new data available
    pub y_data_available: bool,
    /// X new data available
    pub x_data_available: bool,
}

impl From<u8> for Status {
    fn from(value: u8) -> Self {
        Status {
            xyz_overrun: (value & 0x80) != 0,
            z_overrun: (value & 0x40) != 0,
            y_overrun: (value & 0x20) != 0,
            x_overrun: (value & 0x10) != 0,
            xyz_data_available: (value & 0x08) != 0,
            z_data_available: (value & 0x04) != 0,
            y_data_available: (value & 0x02) != 0,
            x_data_available: (value & 0x01) != 0,
        }
    }
}

/// FIFO status
#[derive(Debug, Clone, Copy, Default)]
pub struct FifoStatus {
    /// Watermark level exceeded
    pub watermark: bool,
    /// FIFO overrun
    pub overrun: bool,
    /// FIFO empty
    pub empty: bool,
    /// Number of unread samples (0-32)
    pub samples: u8,
}

impl From<u8> for FifoStatus {
    fn from(value: u8) -> Self {
        FifoStatus {
            watermark: (value & 0x80) != 0,
            overrun: (value & 0x40) != 0,
            empty: (value & 0x20) != 0,
            samples: value & 0x1F,
        }
    }
}

// =============================================================================
// Interrupt Configuration Builders
// =============================================================================

/// Configuration for interrupt thresholds and axes
#[derive(Debug, Clone, Copy, Default)]
pub struct InterruptConfig {
    /// Interrupt mode (OR, AND, 6D movement, 6D position)
    pub mode: InterruptMode,
    /// Enable X high event
    pub x_high: bool,
    /// Enable X low event
    pub x_low: bool,
    /// Enable Y high event
    pub y_high: bool,
    /// Enable Y low event
    pub y_low: bool,
    /// Enable Z high event
    pub z_high: bool,
    /// Enable Z low event
    pub z_low: bool,
    /// Threshold value (7-bit, 0-127)
    pub threshold: u8,
    /// Duration value (7-bit, 0-127) in 1/ODR units
    pub duration: u8,
}

impl InterruptConfig {
    /// Create a new interrupt configuration
    pub fn new() -> Self { Self::default() }

    /// Configure for motion detection on any axis
    pub fn motion_any_axis(threshold: u8, duration: u8) -> Self {
        InterruptConfig {
            mode: InterruptMode::Movement6D,
            x_high: true,
            x_low: true,
            y_high: true,
            y_low: true,
            z_high: true,
            z_low: true,
            threshold: threshold & 0x7F,
            duration: duration & 0x7F,
        }
    }

    /// Configure for 6D position detection
    pub fn position_6d(threshold: u8) -> Self {
        InterruptConfig {
            mode: InterruptMode::Position6D,
            x_high: true,
            x_low: true,
            y_high: true,
            y_low: true,
            z_high: true,
            z_low: true,
            threshold: threshold & 0x7F,
            duration: 0,
        }
    }

    /// Convert to INT_CFG register value
    pub fn to_cfg_byte(&self) -> u8 {
        let (aoi, d6) = self.mode.to_bits();
        let mut value = 0u8;
        if aoi {
            value |= 0x80;
        }
        if d6 {
            value |= 0x40;
        }
        if self.z_high {
            value |= 0x20;
        }
        if self.z_low {
            value |= 0x10;
        }
        if self.y_high {
            value |= 0x08;
        }
        if self.y_low {
            value |= 0x04;
        }
        if self.x_high {
            value |= 0x02;
        }
        if self.x_low {
            value |= 0x01;
        }
        value
    }
}

/// Click/tap detection configuration
#[derive(Debug, Clone, Copy, Default)]
pub struct ClickConfig {
    /// Enable double-click on X axis
    pub x_double: bool,
    /// Enable single-click on X axis
    pub x_single: bool,
    /// Enable double-click on Y axis
    pub y_double: bool,
    /// Enable single-click on Y axis
    pub y_single: bool,
    /// Enable double-click on Z axis
    pub z_double: bool,
    /// Enable single-click on Z axis
    pub z_single: bool,
    /// Click threshold (7-bit)
    pub threshold: u8,
    /// Time limit (7-bit) - max time for click
    pub time_limit: u8,
    /// Time latency - time between clicks for double-click
    pub time_latency: u8,
    /// Time window - max time for second click in double-click
    pub time_window: u8,
    /// Latch interrupt until CLICK_SRC is read
    pub latch: bool,
}

impl ClickConfig {
    /// Create a new click configuration
    pub fn new() -> Self { Self::default() }

    /// Configure for single tap detection on any axis
    pub fn single_tap_any(threshold: u8, time_limit: u8) -> Self {
        ClickConfig {
            x_single: true,
            y_single: true,
            z_single: true,
            threshold: threshold & 0x7F,
            time_limit: time_limit & 0x7F,
            ..Default::default()
        }
    }

    /// Configure for double tap detection on any axis
    pub fn double_tap_any(threshold: u8, time_limit: u8, latency: u8, window: u8) -> Self {
        ClickConfig {
            x_double: true,
            y_double: true,
            z_double: true,
            threshold: threshold & 0x7F,
            time_limit: time_limit & 0x7F,
            time_latency: latency,
            time_window: window,
            latch: true,
            ..Default::default()
        }
    }

    /// Convert to CLICK_CFG register value
    pub fn to_cfg_byte(&self) -> u8 {
        let mut value = 0u8;
        if self.z_double {
            value |= 0x20;
        }
        if self.z_single {
            value |= 0x10;
        }
        if self.y_double {
            value |= 0x08;
        }
        if self.y_single {
            value |= 0x04;
        }
        if self.x_double {
            value |= 0x02;
        }
        if self.x_single {
            value |= 0x01;
        }
        value
    }

    /// Convert to CLICK_THS register value (includes latch bit)
    pub fn to_ths_byte(&self) -> u8 {
        let mut value = self.threshold & 0x7F;
        if self.latch {
            value |= 0x80;
        }
        value
    }
}

// =============================================================================
// Main Driver
// =============================================================================

/// LIS2DH12 accelerometer driver
pub struct Lis2dh12 {
    /// Current full-scale setting (cached for conversion calculations)
    full_scale: FullScale,
    /// Current operating mode (cached for conversion calculations)
    operating_mode: OperatingMode,
}

impl Lis2dh12 {
    /// Create a new LIS2DH12 driver and initialize the device
    ///
    /// This performs a basic initialization:
    /// - Waits for boot completion (5ms)
    /// - Verifies the WHO_AM_I register
    /// - Sets default operating mode (Normal, 10 Hz, all axes enabled)
    /// - Sets default full scale (±2g)
    pub fn new(i2c: &mut dyn I2cApi) -> Result<Self, xous::Error> {
        let mut driver = Lis2dh12 { full_scale: FullScale::G2, operating_mode: OperatingMode::Normal };

        // Wait for boot procedure to complete (datasheet says 5ms max)
        Self::delay_ms(5);

        // Verify device identity
        let who_am_i = driver.read_who_am_i(i2c)?;
        if who_am_i != WHO_AM_I_VALUE {
            return Err(xous::Error::NotFound);
        }

        // Initialize with safe defaults
        driver.init_defaults(i2c)?;

        Ok(driver)
    }

    /// Delay for the specified number of milliseconds
    fn delay_ms(ms: u64) { thread::sleep(Duration::from_millis(ms)); }

    /// Initialize device with default settings
    fn init_defaults(&mut self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        // CTRL_REG0: Keep default (SDO pull-up enabled)
        // Note: bit 4 must be 1 for correct operation
        self.write_register(i2c, regs::CTRL_REG0, 0x10)?;

        // CTRL_REG1: 200 Hz, normal mode, all axes enabled
        self.write_register(i2c, regs::CTRL_REG1, 0x67)?;

        // CTRL_REG2: No high-pass filter
        self.write_register(i2c, regs::CTRL_REG2, 0x00)?;

        // CTRL_REG3: No interrupts on INT1 initially
        self.write_register(i2c, regs::CTRL_REG3, 0x00)?;

        // CTRL_REG4: ±2g, BDU enabled, high-resolution disabled
        self.write_register(i2c, regs::CTRL_REG4, 0x80)?;

        // CTRL_REG5: No boot, no FIFO, no latch
        self.write_register(i2c, regs::CTRL_REG5, 0x00)?;

        // CTRL_REG6: No interrupts on INT2, active-high
        self.write_register(i2c, regs::CTRL_REG6, 0x00)?;

        Ok(())
    }

    // =========================================================================
    // Low-level register access
    // =========================================================================

    /// Read a single register
    fn read_register(&self, i2c: &mut dyn I2cApi, reg: u8) -> Result<u8, xous::Error> {
        let mut buf = [0u8; 1];
        i2c.i2c_read(LIS2DH12_ADDR, reg, &mut buf, true).check()?;
        Ok(buf[0])
    }

    /// Write a single register
    fn write_register(&self, i2c: &mut dyn I2cApi, reg: u8, value: u8) -> Result<(), xous::Error> {
        i2c.i2c_write(LIS2DH12_ADDR, reg, &[value]).check()
    }

    /// Read multiple consecutive registers
    fn read_registers(&self, i2c: &mut dyn I2cApi, start_reg: u8, buf: &mut [u8]) -> Result<(), xous::Error> {
        // Set MSB for auto-increment
        i2c.i2c_read(LIS2DH12_ADDR, start_reg | regs::AUTO_INCREMENT, buf, true).check()
    }

    /// Modify a register using read-modify-write
    fn modify_register<F>(&self, i2c: &mut dyn I2cApi, reg: u8, f: F) -> Result<(), xous::Error>
    where
        F: FnOnce(u8) -> u8,
    {
        let value = self.read_register(i2c, reg)?;
        self.write_register(i2c, reg, f(value))
    }

    // =========================================================================
    // Device identification and status
    // =========================================================================

    /// Read WHO_AM_I register (should return 0x33)
    pub fn read_who_am_i(&self, i2c: &mut dyn I2cApi) -> Result<u8, xous::Error> {
        self.read_register(i2c, regs::WHO_AM_I)
    }

    /// Read the status register
    pub fn read_status(&self, i2c: &mut dyn I2cApi) -> Result<Status, xous::Error> {
        Ok(Status::from(self.read_register(i2c, regs::STATUS_REG)?))
    }

    /// Check if new data is available
    pub fn data_available(&self, i2c: &mut dyn I2cApi) -> Result<bool, xous::Error> {
        Ok(self.read_status(i2c)?.xyz_data_available)
    }

    // =========================================================================
    // Operating mode configuration
    // =========================================================================

    /// Set the output data rate
    pub fn set_data_rate(&mut self, i2c: &mut dyn I2cApi, rate: DataRate) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG1, |v| (v & 0x0F) | (u8::from(rate) << 4))
    }

    /// Get the current output data rate
    pub fn get_data_rate(&self, i2c: &mut dyn I2cApi) -> Result<DataRate, xous::Error> {
        let reg = self.read_register(i2c, regs::CTRL_REG1)?;
        DataRate::try_from(reg >> 4)
    }

    /// Set the full-scale range
    pub fn set_full_scale(&mut self, i2c: &mut dyn I2cApi, scale: FullScale) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG4, |v| (v & 0xCF) | (u8::from(scale) << 4))?;
        self.full_scale = scale;
        Ok(())
    }

    /// Get the current full-scale range
    pub fn get_full_scale(&self, i2c: &mut dyn I2cApi) -> Result<FullScale, xous::Error> {
        let reg = self.read_register(i2c, regs::CTRL_REG4)?;
        FullScale::try_from((reg >> 4) & 0x03)
    }

    /// Set the operating mode (low-power, normal, or high-resolution)
    ///
    /// Note: Mode transitions have turn-on times that depend on ODR.
    /// This function adds a conservative delay to ensure the mode change is complete.
    pub fn set_operating_mode(
        &mut self,
        i2c: &mut dyn I2cApi,
        mode: OperatingMode,
    ) -> Result<(), xous::Error> {
        let (lpen, hr) = mode.to_bits();

        // If switching from high-resolution to another mode, read REFERENCE first
        // (per datasheet recommendation for proper filter block reset)
        if self.operating_mode == OperatingMode::HighResolution && mode != OperatingMode::HighResolution {
            let _ = self.read_register(i2c, regs::REFERENCE)?;
        }

        // Set LPen bit in CTRL_REG1
        self.modify_register(i2c, regs::CTRL_REG1, |v| if lpen { v | 0x08 } else { v & !0x08 })?;

        // Set HR bit in CTRL_REG4
        self.modify_register(i2c, regs::CTRL_REG4, |v| if hr { v | 0x08 } else { v & !0x08 })?;

        // Delay for mode transition (7/ODR worst case; at 10Hz = 700ms, but we use
        // a shorter delay assuming higher ODR or accepting some startup samples)
        // Using 10ms as a reasonable compromise
        Self::delay_ms(10);

        self.operating_mode = mode;
        Ok(())
    }

    /// Get the current operating mode
    pub fn get_operating_mode(&self, i2c: &mut dyn I2cApi) -> Result<OperatingMode, xous::Error> {
        let reg1 = self.read_register(i2c, regs::CTRL_REG1)?;
        let reg4 = self.read_register(i2c, regs::CTRL_REG4)?;
        let lpen = (reg1 & 0x08) != 0;
        let hr = (reg4 & 0x08) != 0;
        OperatingMode::from_bits(lpen, hr).ok_or(xous::Error::InvalidCoding)
    }

    /// Enable or disable specific axes
    pub fn set_axes_enabled(
        &self,
        i2c: &mut dyn I2cApi,
        x: bool,
        y: bool,
        z: bool,
    ) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG1, |v| {
            let mut new_val = v & 0xF8;
            if z {
                new_val |= 0x04;
            }
            if y {
                new_val |= 0x02;
            }
            if x {
                new_val |= 0x01;
            }
            new_val
        })
    }

    /// Enable Block Data Update (BDU)
    ///
    /// When enabled, output registers are not updated until both MSB and LSB
    /// have been read, preventing reading of partially-updated data.
    /// This is enabled by default.
    pub fn enable_bdu(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG4, |v| v | 0x80)
    }

    /// Disable Block Data Update (BDU)
    ///
    /// When disabled, output registers are continuously updated.
    /// This may be useful for debugging but risks reading inconsistent data.
    pub fn disable_bdu(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG4, |v| v & !0x80)
    }

    /// Enter power-down mode
    pub fn power_down(&mut self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.set_data_rate(i2c, DataRate::PowerDown)
    }

    // =========================================================================
    // Acceleration data reading
    // =========================================================================

    /// Read raw acceleration data from all three axes
    pub fn read_accel_raw(&self, i2c: &mut dyn I2cApi) -> Result<AccelData, xous::Error> {
        let mut buf = [0u8; 6];
        self.read_registers(i2c, regs::OUT_X_L, &mut buf)?;

        Ok(AccelData {
            x: i16::from_le_bytes([buf[0], buf[1]]),
            y: i16::from_le_bytes([buf[2], buf[3]]),
            z: i16::from_le_bytes([buf[4], buf[5]]),
        })
    }

    /// Read acceleration data in milli-g
    pub fn read_accel_mg(&self, i2c: &mut dyn I2cApi) -> Result<(i32, i32, i32), xous::Error> {
        let raw = self.read_accel_raw(i2c)?;
        Ok(raw.to_mg(self.full_scale, self.operating_mode))
    }

    /// Determine the current orientation of the device
    ///
    /// This checks if the device is face-up or face-down based on the Z-axis reading.
    /// For pivoting around Y-axis (X changes sign), use `get_x_orientation()`.
    pub fn get_orientation(&self, i2c: &mut dyn I2cApi) -> Result<Orientation, xous::Error> {
        let (x, _, _) = self.read_accel_mg(i2c)?;

        // At rest, gravity should read ~1000 mg on the axis pointing up
        // Use a threshold of 500 mg to account for tilt
        const THRESHOLD_MG: i32 = 500;

        if x > THRESHOLD_MG {
            Ok(Orientation::FaceUp)
        } else if x < -THRESHOLD_MG {
            Ok(Orientation::FaceDown)
        } else {
            Ok(Orientation::Unknown)
        }
    }

    /// Check if device is face-up or face-down based on X-axis (pivoting around Y)
    ///
    /// Returns true if X is positive (one orientation), false if negative (flipped)
    pub fn is_x_positive(&self, i2c: &mut dyn I2cApi) -> Result<bool, xous::Error> {
        let (x, _, _) = self.read_accel_mg(i2c)?;
        Ok(x > 0)
    }

    // =========================================================================
    // Temperature sensor
    // =========================================================================

    /// Enable the temperature sensor
    pub fn enable_temperature(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        // Enable temperature sensor (TEMP_EN = 11)
        self.write_register(i2c, regs::TEMP_CFG_REG, 0xC0)?;
        // Also need BDU bit set (should already be set from init)
        self.modify_register(i2c, regs::CTRL_REG4, |v| v | 0x80)
    }

    /// Disable the temperature sensor
    pub fn disable_temperature(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.write_register(i2c, regs::TEMP_CFG_REG, 0x00)
    }

    /// Read temperature data (relative, not absolute)
    ///
    /// The temperature sensor provides relative temperature change with 1°C/digit resolution.
    /// This is NOT calibrated for absolute temperature measurement.
    pub fn read_temperature_raw(&self, i2c: &mut dyn I2cApi) -> Result<i16, xous::Error> {
        let mut buf = [0u8; 2];
        self.read_registers(i2c, regs::OUT_TEMP_L, &mut buf)?;
        Ok(i16::from_le_bytes([buf[0], buf[1]]))
    }

    // =========================================================================
    // Debug helpers
    // =========================================================================

    /// Dump all interrupt-related register values for debugging
    ///
    /// Returns a struct with all the relevant register values that can be printed.
    /// NOTE: Reading INT1_SRC clears a latched interrupt, so this may affect interrupt state!
    pub fn debug_dump_int_config(&self, i2c: &mut dyn I2cApi) -> Result<IntDebugDump, xous::Error> {
        Ok(IntDebugDump {
            ctrl_reg1: self.read_register(i2c, regs::CTRL_REG1)?,
            ctrl_reg2: self.read_register(i2c, regs::CTRL_REG2)?,
            ctrl_reg3: self.read_register(i2c, regs::CTRL_REG3)?,
            ctrl_reg4: self.read_register(i2c, regs::CTRL_REG4)?,
            ctrl_reg5: self.read_register(i2c, regs::CTRL_REG5)?,
            ctrl_reg6: self.read_register(i2c, regs::CTRL_REG6)?,
            int1_cfg: self.read_register(i2c, regs::INT1_CFG)?,
            int1_ths: self.read_register(i2c, regs::INT1_THS)?,
            int1_duration: self.read_register(i2c, regs::INT1_DURATION)?,
            int1_src: self.read_register(i2c, regs::INT1_SRC)?,
            status_reg: self.read_register(i2c, regs::STATUS_REG)?,
        })
    }

    /// Poll INT1_SRC to check if interrupt condition is met (without relying on pin)
    ///
    /// This reads INT1_SRC which will also clear a latched interrupt.
    /// Returns true if the IA (interrupt active) bit is set.
    pub fn poll_int1_active(&self, i2c: &mut dyn I2cApi) -> Result<bool, xous::Error> {
        let src = self.read_register(i2c, regs::INT1_SRC)?;
        Ok((src & 0x40) != 0)
    }

    /// Simple interrupt test: configure with very low threshold and check if it triggers
    ///
    /// This sets threshold to 1 LSB (16mg at ±2g) with no duration requirement.
    /// Should trigger almost immediately if the device is experiencing any acceleration
    /// (including gravity if not perfectly level).
    ///
    /// Returns Ok(true) if interrupt triggered, Ok(false) if not.
    pub fn debug_test_int1_simple(&self, i2c: &mut dyn I2cApi) -> Result<bool, xous::Error> {
        // Save current config
        /*
        let saved_cfg = self.read_register(i2c, regs::INT1_CFG)?;
        let saved_ths = self.read_register(i2c, regs::INT1_THS)?;
        let saved_dur = self.read_register(i2c, regs::INT1_DURATION)?;
        */
        let saved_ctrl3 = self.read_register(i2c, regs::CTRL_REG3)?;

        self.write_register(i2c, regs::CTRL_REG5, 0x08)?;

        // Configure for very sensitive motion detection
        // INT1_CFG: OR combination, all axes high/low enabled
        self.write_register(i2c, regs::INT1_CFG, 0x7F)?;
        // INT1_THS: threshold = 1 (16mg at ±2g)
        self.write_register(i2c, regs::INT1_THS, 8)?;
        // INT1_DURATION: 0 (no minimum duration)
        self.write_register(i2c, regs::INT1_DURATION, 0)?;
        // CTRL_REG3: Enable IA1 on INT1 pin
        self.write_register(i2c, regs::CTRL_REG3, saved_ctrl3 | 0x40)?;

        self.write_register(i2c, regs::CTRL_REG2, 0b00_00_0001)?;
        self.read_register(i2c, regs::REFERENCE)?;

        // Wait a bit for new samples
        Self::delay_ms(150);

        // Clear any pending interrupt by reading INT1_SRC
        let _ = self.read_register(i2c, regs::INT1_SRC)?;

        // Check if interrupt is active
        let src = self.read_register(i2c, regs::INT1_SRC)?;
        let triggered = (src & 0x40) != 0;

        log::info!("debug_test_int1_simple: INT1_SRC = 0x{:02X}, triggered = {}", src, triggered);

        // Restore original config
        /*
        self.write_register(i2c, regs::INT1_CFG, saved_cfg)?;
        self.write_register(i2c, regs::INT1_THS, saved_ths)?;
        self.write_register(i2c, regs::INT1_DURATION, saved_dur)?;
        self.write_register(i2c, regs::CTRL_REG3, saved_ctrl3)?;
        */

        Ok(triggered)
    }

    // =========================================================================
    // Interrupt configuration - INT1
    //
    // INT1 is configured as push-pull, active-high by default.
    // When an interrupt condition is met, INT1 drives high.
    // =========================================================================

    /*
    INFO:bao_console::cmds::test: mg data: (252, 16, -980) (services/bao-console/src/cmds/test.rs:289)
    INFO:bao1x_hal::lis2dh12: setup int1: 3f, 6, 2 (libs/bao1x-hal/src/lis2dh12.rs:1065)
    INFO:bao_console::cmds::test: Motion detected! InterruptSource { active: true, x_high: true, x_low: false, y_high: false, y_low: true, z_high: true, z_low: false } (services/bao-console/src/cmds/test.rs:302)
    INFO:bao_console::cmds::test: Motion detected! InterruptSource { active: true, x_high: true, x_low: false, y_high: false, y_low: true, z_high: true, z_low: false } (services/bao-console/src/cmds/test.rs:302)
         */

    /// Configure interrupt 1 for motion/threshold detection
    pub fn configure_int1(&self, i2c: &mut dyn I2cApi, config: &InterruptConfig) -> Result<(), xous::Error> {
        // Write configuration register
        self.write_register(i2c, regs::INT1_CFG, config.to_cfg_byte())?;

        // Write threshold
        self.write_register(i2c, regs::INT1_THS, config.threshold & 0x7F)?;

        // Write duration
        self.write_register(i2c, regs::INT1_DURATION, config.duration & 0x7F)?;

        Ok(())
    }

    /// Enable INT1 interrupt output on the INT1 pin
    pub fn enable_int1_pin(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v | 0x40) // I1_IA1
    }

    /// Disable INT1 interrupt output on the INT1 pin
    pub fn disable_int1_pin(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v & !0x40)
    }

    /// Latch INT1 interrupt (stays active until INT1_SRC is read)
    pub fn set_int1_latch(&self, i2c: &mut dyn I2cApi, latch: bool) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG5, |v| if latch { v | 0x08 } else { v & !0x08 })
    }

    /// Read and clear INT1 source register
    pub fn read_int1_source(&self, i2c: &mut dyn I2cApi) -> Result<InterruptSource, xous::Error> {
        Ok(InterruptSource::from(self.read_register(i2c, regs::INT1_SRC)?))
    }

    // =========================================================================
    // Interrupt configuration - INT2
    // =========================================================================

    /// Configure interrupt 2 for motion/threshold detection
    pub fn configure_int2(&self, i2c: &mut dyn I2cApi, config: &InterruptConfig) -> Result<(), xous::Error> {
        self.write_register(i2c, regs::INT2_CFG, config.to_cfg_byte())?;
        self.write_register(i2c, regs::INT2_THS, config.threshold & 0x7F)?;
        self.write_register(i2c, regs::INT2_DURATION, config.duration & 0x7F)?;
        Ok(())
    }

    /// Enable INT2 interrupt output on the INT2 pin
    pub fn enable_int2_pin(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v | 0x20) // I2_IA2
    }

    /// Disable INT2 interrupt output on the INT2 pin
    pub fn disable_int2_pin(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v & !0x20)
    }

    /// Latch INT2 interrupt
    pub fn set_int2_latch(&self, i2c: &mut dyn I2cApi, latch: bool) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG5, |v| if latch { v | 0x02 } else { v & !0x02 })
    }

    /// Read and clear INT2 source register
    pub fn read_int2_source(&self, i2c: &mut dyn I2cApi) -> Result<InterruptSource, xous::Error> {
        Ok(InterruptSource::from(self.read_register(i2c, regs::INT2_SRC)?))
    }

    /// Set interrupt polarity for both INT1 and INT2
    ///
    /// Default is active-high (push-pull). Both pins share the same polarity setting.
    /// Note: If using INT1 to drive a transistor gate that pulls another signal low,
    /// keep the default active-high setting.
    pub fn set_interrupt_polarity(
        &self,
        i2c: &mut dyn I2cApi,
        polarity: InterruptPolarity,
    ) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| match polarity {
            InterruptPolarity::ActiveHigh => v & !0x02,
            InterruptPolarity::ActiveLow => v | 0x02,
        })
    }

    // =========================================================================
    // Click/Tap detection
    // =========================================================================

    /// Configure click/tap detection
    pub fn configure_click(&self, i2c: &mut dyn I2cApi, config: &ClickConfig) -> Result<(), xous::Error> {
        self.write_register(i2c, regs::CLICK_CFG, config.to_cfg_byte())?;
        self.write_register(i2c, regs::CLICK_THS, config.to_ths_byte())?;
        self.write_register(i2c, regs::TIME_LIMIT, config.time_limit & 0x7F)?;
        self.write_register(i2c, regs::TIME_LATENCY, config.time_latency)?;
        self.write_register(i2c, regs::TIME_WINDOW, config.time_window)?;
        Ok(())
    }

    /// Enable click interrupt on INT1 pin
    pub fn enable_click_int1(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v | 0x80)
    }

    /// Disable click interrupt on INT1 pin
    pub fn disable_click_int1(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v & !0x80)
    }

    /// Enable click interrupt on INT2 pin
    pub fn enable_click_int2(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v | 0x80)
    }

    /// Disable click interrupt on INT2 pin
    pub fn disable_click_int2(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v & !0x80)
    }

    /// Read and clear click source register
    pub fn read_click_source(&self, i2c: &mut dyn I2cApi) -> Result<ClickSource, xous::Error> {
        Ok(ClickSource::from(self.read_register(i2c, regs::CLICK_SRC)?))
    }

    // =========================================================================
    // Activity/Inactivity detection (Sleep-to-wake)
    // =========================================================================

    /// Configure sleep-to-wake / return-to-sleep function
    ///
    /// When acceleration falls below threshold, device switches to 10Hz low-power mode.
    /// When acceleration rises above threshold, device returns to configured mode.
    ///
    /// - `threshold`: Activity threshold (7-bit, in LSB units dependent on full scale)
    /// - `duration`: Duration before return-to-sleep (8-bit, in (8*LSB+1)/ODR units)
    pub fn configure_activity_detection(
        &self,
        i2c: &mut dyn I2cApi,
        threshold: u8,
        duration: u8,
    ) -> Result<(), xous::Error> {
        self.write_register(i2c, regs::ACT_THS, threshold & 0x7F)?;
        self.write_register(i2c, regs::ACT_DUR, duration)?;
        Ok(())
    }

    /// Enable activity interrupt on INT2 pin
    pub fn enable_activity_int2(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v | 0x08)
    }

    /// Disable activity interrupt on INT2 pin
    pub fn disable_activity_int2(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG6, |v| v & !0x08)
    }

    // =========================================================================
    // FIFO operations
    // =========================================================================

    /// Enable FIFO
    pub fn enable_fifo(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG5, |v| v | 0x40)
    }

    /// Disable FIFO
    pub fn disable_fifo(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG5, |v| v & !0x40)
    }

    /// Configure FIFO mode and watermark
    ///
    /// - `mode`: FIFO operating mode
    /// - `watermark`: Watermark level (0-31)
    /// - `trigger_int2`: If true, trigger on INT2 instead of INT1
    pub fn configure_fifo(
        &self,
        i2c: &mut dyn I2cApi,
        mode: FifoMode,
        watermark: u8,
        trigger_int2: bool,
    ) -> Result<(), xous::Error> {
        let mut value = (u8::from(mode) << 6) | (watermark & 0x1F);
        if trigger_int2 {
            value |= 0x20;
        }
        self.write_register(i2c, regs::FIFO_CTRL_REG, value)
    }

    /// Read FIFO status
    pub fn read_fifo_status(&self, i2c: &mut dyn I2cApi) -> Result<FifoStatus, xous::Error> {
        Ok(FifoStatus::from(self.read_register(i2c, regs::FIFO_SRC_REG)?))
    }

    /// Enable FIFO watermark interrupt on INT1
    pub fn enable_fifo_watermark_int1(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v | 0x04)
    }

    /// Enable FIFO overrun interrupt on INT1
    pub fn enable_fifo_overrun_int1(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG3, |v| v | 0x02)
    }

    // =========================================================================
    // High-pass filter configuration
    // =========================================================================

    /// Configure the high-pass filter
    pub fn configure_highpass(
        &self,
        i2c: &mut dyn I2cApi,
        mode: HighPassFilterMode,
        cutoff: u8,
        filter_data_output: bool,
    ) -> Result<(), xous::Error> {
        let mut value = (u8::from(mode) << 6) | ((cutoff & 0x03) << 4);
        if filter_data_output {
            value |= 0x08;
        }
        self.write_register(i2c, regs::CTRL_REG2, value)
    }

    /// Enable high-pass filter for click detection
    pub fn enable_highpass_click(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG2, |v| v | 0x04)
    }

    /// Enable high-pass filter for INT1 (AOI function)
    pub fn enable_highpass_int1(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG2, |v| v | 0x01)
    }

    /// Enable high-pass filter for INT2 (AOI function)
    pub fn enable_highpass_int2(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG2, |v| v | 0x02)
    }

    /// Reset high-pass filter by reading the REFERENCE register
    pub fn reset_highpass(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        let _ = self.read_register(i2c, regs::REFERENCE)?;
        Ok(())
    }

    // =========================================================================
    // Self-test
    // =========================================================================

    /// Enable self-test mode
    pub fn set_self_test(&self, i2c: &mut dyn I2cApi, mode: SelfTestMode) -> Result<(), xous::Error> {
        self.modify_register(i2c, regs::CTRL_REG4, |v| (v & 0xF9) | (u8::from(mode) << 1))
    }

    // =========================================================================
    // Convenience methods for common use cases
    // =========================================================================

    /// Quick setup for motion detection interrupt
    ///
    /// Sets up the accelerometer to generate an interrupt when motion exceeds
    /// the specified threshold in mg. Uses INT1 pin.
    ///
    /// - `threshold_mg`: Motion threshold in milli-g
    /// - `duration_ms`: Minimum duration of motion (approximate, depends on ODR)
    pub fn setup_motion_interrupt(
        &mut self,
        i2c: &mut dyn I2cApi,
        threshold_mg: u16,
        duration_samples: u8,
    ) -> Result<(), xous::Error> {
        // Convert mg threshold to register value
        let lsb_mg = self.full_scale.threshold_mg_per_lsb();
        let threshold = ((threshold_mg as u32) / (lsb_mg as u32)).min(127) as u8;

        let config = InterruptConfig::motion_any_axis(threshold, duration_samples);
        self.configure_int1(i2c, &config)?;
        self.set_int1_latch(i2c, true)?;
        self.enable_int1_pin(i2c)?;

        Ok(())
    }

    /// Quick setup for tap detection
    ///
    /// Sets up single-tap detection on all axes using INT1 pin.
    pub fn setup_tap_interrupt(
        &mut self,
        i2c: &mut dyn I2cApi,
        threshold_mg: u16,
    ) -> Result<(), xous::Error> {
        // Convert mg threshold to register value
        let lsb_mg = self.full_scale.threshold_mg_per_lsb();
        let threshold = ((threshold_mg as u32) / (lsb_mg as u32)).min(127) as u8;

        // Time limit depends on ODR - using conservative values
        let config = ClickConfig::single_tap_any(threshold, 10);
        self.configure_click(i2c, &config)?;
        self.enable_click_int1(i2c)?;

        Ok(())
    }

    /// Quick setup for orientation detection using 6D position recognition
    pub fn setup_orientation_detection(&self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        // Use 6D position detection with ~0.5g threshold (32 * 16mg = 512mg)
        let config = InterruptConfig::position_6d(32);
        self.configure_int1(i2c, &config)?;
        self.set_int1_latch(i2c, true)?;
        self.enable_int1_pin(i2c)?;

        Ok(())
    }
}

// =============================================================================
// Unit tests (for development/verification)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_rate_conversion() {
        assert_eq!(u8::from(DataRate::Hz10), 0b0010);
        assert_eq!(DataRate::try_from(0b0010).unwrap(), DataRate::Hz10);
    }

    #[test]
    fn test_full_scale_sensitivity() {
        assert_eq!(FullScale::G2.sensitivity_mg(OperatingMode::HighResolution), 1);
        assert_eq!(FullScale::G2.sensitivity_mg(OperatingMode::Normal), 4);
        assert_eq!(FullScale::G2.sensitivity_mg(OperatingMode::LowPower), 16);
    }

    #[test]
    fn test_interrupt_config() {
        let config = InterruptConfig::motion_any_axis(32, 5);
        assert_eq!(config.to_cfg_byte(), 0x3F); // All axes, OR mode
        assert_eq!(config.threshold, 32);
        assert_eq!(config.duration, 5);
    }

    #[test]
    fn test_accel_conversion() {
        let raw = AccelData {
            x: 0x1000, // 4096 in left-justified 12-bit = 256 in 12-bit value
            y: 0,
            z: 0,
        };
        let (x, y, z) = raw.to_mg(FullScale::G2, OperatingMode::HighResolution);
        // 256 * 1 mg/digit = 256 mg
        assert_eq!(x, 256);
        assert_eq!(y, 0);
        assert_eq!(z, 0);
    }
}

/*
This file based on driver generated by Claude Opus 4.5 based on the following prompt:

I'm looking to write a driver for the part in the attached PDF, the LIS2DH12TR. it's an accelerometer. [PDF is uploaded to the context buffer]

The framework I'm using is Xous. I've also pasted in a simple example driver for a different I2C part that gives you a template for how the driver should look. Note that for the driver you're writing, you only need to do the `std` code, so you can skip the no_std/std flags in the example and just do std. Xous is unusual in that it is an embedded microcontroller OS that also has std and runs in Rust. I'd also like you to improve the error handling from the example to use the `?` idiom to remove the match error handling - in general, all the errors in the API should a xous::Error type. I'll go into that more after going through the functional API.
[the file referred to is bao1x-hal/src/bmp180.rs]

Here's what the trait definition is for the I2C interface:

pub trait I2cApi {
    fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<I2cResult, xous::Error>;
    /// initiate an i2c read. The read buffer is passed during the await.
    fn i2c_read(
        &mut self,
        dev: u8,
        adr: u8,
        buf: &mut [u8],
        repeated_start: bool,
    ) -> Result<I2cResult, xous::Error>;
}

In the case of a write, the `dev` field is the device address; the `adr` is the address of the register; and `data` is a slice that contains the byte or bytes to be written consecutively starting at `adr`. The Result will generally be an Ack(usize) where the argument inside Ack is the number of bytes written.

In the case of a read, the arguments have a similar format, except that the read passes a &mut [u8] that is storage for the return result, and you can specify a `repeated_start` for the read (I think for this chip you want that to be `true`, but check my assumption).

The I2C write/read are non-atomic, but guaranteed to have the correct ordering. In other words, it's possible that another device will do something else in between the i2c write/read causing things to wait a bit. I will *not* have two drivers at once fighting for the acceleramoter, but for example a PMIC could take the I2C bus and do something in between a write and read to the accelerometer. I don't think this is an edge case of concern here but if you see any sequences in the accelerometer datasheet where you *must* have two operations complete with no interruption between them, flag it. There is a different API I have to force a bus-wide atomic, uninterruptable sequence of reads/writes.

The main things I need to do with the part are:

* determine the orientation of the product: the orientation determination is very coarse, I just need to know if it is placed face up or face down (in this case, pivoting around the Y axis so the X-reading would change sign)
* generate an interrupt when the product is being worn/moved. possibly fine to just use a "tap" detection mode but if it has a mode to detect movement greater than some threshold of acceleration, an API to configure that interrupt would be useful

In terms of the interrupt handler itself, I'll add an extension that responds to the interrupts separately from this driver; it doesn't need to exist in the I2C framework because the interrupt gets routed to a GPIO. So please just focus on:

* Extracting the register set with human-readable names
* Extracting getter/setters for various registers, using enums to define values with the appropriate from/into fields to make an ergonomic programming interface
* Note that this is configured with SA0 tied high, so you'll need to set the I2C address accordingly.

For error handling, the I2CResult possibilities are:

pub enum I2cResult {
    /// For the outbound message holder
    Pending,
    /// Returns # of bytes read or written if successful
    Ack(usize),
    /// An error occurred.
    Nack,
    /// An unhandled error has occurred.
    InternalError,
}

Ack is actually the Ok case - it's not an error. The `usize` argument is, in practice, *always* correct (the underlying implementation just passes back the requested number) so no code is needed to check that. Just ignore that number to keep the code compact.

I don't know if this is possible but I'd like a compact way to use the `?` idiom to convert the other I2cResult members into a xous::Error type. The xous error types are here:

pub enum Error {
    NoError = 0,
    BadAlignment = 1,
    BadAddress = 2,
    OutOfMemory = 3,
    MemoryInUse = 4,
    InterruptNotFound = 5,
    InterruptInUse = 6,
    InvalidString = 7,
    ServerExists = 8,
    ServerNotFound = 9,
    ProcessNotFound = 10,
    ProcessNotChild = 11,
    ProcessTerminated = 12,
    Timeout = 13,
    InternalError = 14,
    ServerQueueFull = 15,
    ThreadNotAvailable = 16,
    UnhandledSyscall = 17,
    InvalidSyscall = 18,
    ShareViolation = 19,
    InvalidThread = 20,
    InvalidPID = 21,
    UnknownError = 22,
    AccessDenied = 23,
    UseBeforeInit = 24,
    DoubleFree = 25,
    DebugInProgress = 26,
    InvalidLimit = 27,
    /// For lookups that result in not found (e.g., searching for keys, resources, names)
    NotFound = 28,
    /// Used when try_from/try_into can't map a number into a smaller number of options
    InvalidCoding = 29,
    /// Used for ECC errors, glitches, power failures, etc.
    HardwareError = 30,
    /// Used when buffers & messages fail to serialize or deserialize
    SerializationError = 31,
    /// Used when a call is correct but its arguments are out of bounds, invalid, or otherwise poorly
    /// specified
    InvalidArgument = 32,
    /// Catch-all for networking related problems (unreachable network, etc.)
    NetworkError = 33,
    /// Catch-all for storage related problems (particularly write/read ECC errors)
    StorageError = 34,
    /// Catch-all for resources that are busy or already allocated
    Unavailable = 35,
    /// For failed parsing attempts
    ParseError = 36,
    /// Invalid core number on multi-core APIs
    InvalidCore = 37,
    /// Reports when verification/check steps fail. Thrown when correctly functioning algorithms determine
    /// that an object is invalid.
    VerificationError = 38,
    /// Used to report higher-severity system security/integrity issues, such as glitch attacks, ECC errors,
    /// memory violations, bad states for hardened bools. Note that bad passwords/credentials should use
    /// "AccessDenied"
    SecurityError = 39,
}

Nack/Pending errors should map to xous::Error::Timeout; InternalError maps to InternalError; and Ack is the case that everything is OK, it's not an error so it doesn't map to an error type.

Ideally there's some nice Rust idiom or wrapper we can use to make this translation cleaner than a match statement on every I2C transaction, since we're going to be using it all over this driver, and it will impact readability if we're using a multi-line match statement or multi-line .unwrap_or_x() handling idioms. i.e. a wrapper trait or some intermediate would be ideal.

Alright, I think that should give you everything you need to know to do this work. If you get stuck, don't spin endlessly, ask questions. i have plenty of examples and what not I am just trying to keep your context buffer tidy so you can focus on the task at hand. Good luck.
*/
