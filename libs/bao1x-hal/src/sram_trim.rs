use arbitrary_int::{u2, u3};
use bitbybit::*;

#[bitfield(u32)]
pub struct SramTrimSpec {
    /// Raw level mode (2 bits)
    #[bits(0..=1, rw)]
    pub rawlm: u2,

    /// Raw level (1 bit)
    #[bit(2, rw)]
    pub rawl: bool,

    /// Write assist boost level mode (3 bits)
    #[bits(3..=5, rw)]
    pub wablm: u3,

    /// Write assist boost level (1 bit)
    #[bit(6, rw)]
    pub wabl: bool,

    /// EMA single (1 bit)
    #[bit(7, rw)]
    pub emas: bool,

    /// EMA width (2 bits)
    #[bits(8..=9, rw)]
    pub emaw: u2,

    /// EMA B (3 bits)
    #[bits(10..=12, rw)]
    pub emab: u3,

    /// EMA (3 bits)
    #[bits(13..=15, rw)]
    pub ema: u3,

    /// Target macro number
    #[bits(16..=23, rw)]
    pub target: u8,

    /// Reserved/unused bits (8 bits)
    #[bits(24..=31)]
    _reserved: u8,
}

// ===== 0.8V Trim Settings =====

const ACRAM2KX64_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(1))       // 2'b01 = 1
    .with_rawl(true)             // 1'b1 = true
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(26); // decimal 26

const AORAM1KX36_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(27); // decimal 27

const BIORAM1KX32_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(19); // decimal 19

const DTCM8KX36_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(6); // decimal 6

const IFRAM32KX36_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(3))       // 2'b11 = 3
    .with_rawl(true)             // 1'b1 = true
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(8); // decimal 8

const ITCM32KX18_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(7); // decimal 7

const RAM32KX72_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(1))       // 2'b01 = 1
    .with_rawl(true)             // 1'b1 = true
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(0); // decimal 0

const RAM8KX72_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(1); // decimal 1

const RF128X31_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(5); // decimal 5

const RF1KX72_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(2); // decimal 2

const RF256X27_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(3); // decimal 3

const RF512X39_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(4); // decimal 4

const SCEAESRAM1K_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(11); // decimal 11

const SCEALURAM3K_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(13); // decimal 13

const SCEHASHRAM3K_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(10); // decimal 10

const SCEPKERAM4K_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(1))       // 3'b001 = 1
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(12); // decimal 12

const SCESCERAM10K_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(4))         // 3'b100 = 4
    .with_target(9); // decimal 9

const FIFO32X19_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(22); // decimal 22

const RDRAM128X22_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(17); // decimal 17

const RDRAM1KX32_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(15); // decimal 15

const RDRAM512X64_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(16); // decimal 16

const RXFIFO128X32_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(21); // decimal 21

const SCEMIMMDPRAM_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(14); // decimal 14

const TXFIFO128X32_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(20); // decimal 20

const UDCMEM256X64_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(25); // decimal 25

const UDCMEMODB1088X64_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(24); // decimal 24

const UDCMEMSHARE1088X64_TRIM_08V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(4))        // 3'b100 = 4
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(23); // decimal 23

// ===== 0.9V Trim Settings =====

const ACRAM2KX64_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(26); // decimal 26

const AORAM1KX36_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(2))         // 3'b010 = 2
    .with_target(27); // decimal 27

const BIORAM1KX32_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(19); // decimal 19

const DTCM8KX36_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(6); // decimal 6

const IFRAM32KX36_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(1))       // 2'b01 = 1
    .with_rawl(true)             // 1'b1 = true
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(2))         // 3'b010 = 2
    .with_target(8); // decimal 8

const ITCM32KX18_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(7); // decimal 7

const RAM32KX72_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(0); // decimal 0

const RAM8KX72_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(1); // decimal 1

const RF128X31_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(5); // decimal 5

const RF1KX72_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(2); // decimal 2

const RF256X27_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(3); // decimal 3

const RF512X39_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(4); // decimal 4

const SCEAESRAM1K_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(11); // decimal 11

const SCEALURAM3K_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(13); // decimal 13

const SCEHASHRAM3K_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(10); // decimal 10

const SCEPKERAM4K_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(true)              // 1'b1 = true
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(1))        // 2'b01 = 1
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(12); // decimal 12

const SCESCERAM10K_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(0))        // 3'b000 = 0
    .with_ema(u3::new(3))         // 3'b011 = 3
    .with_target(9); // decimal 9

const FIFO32X19_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(22); // decimal 22

const RDRAM128X22_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(17); // decimal 17

const RDRAM1KX32_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(15); // decimal 15

const RDRAM512X64_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(16); // decimal 16

const RXFIFO128X32_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(21); // decimal 21

const SCEMIMMDPRAM_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(14); // decimal 14

const TXFIFO128X32_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(20); // decimal 20

const UDCMEM256X64_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(25); // decimal 25

const UDCMEMODB1088X64_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(24); // decimal 24

const UDCMEMSHARE1088X64_TRIM_09V: SramTrimSpec = SramTrimSpec::new_with_raw_value(0)
    .with_rawlm(u2::new(0))       // 2'b00 = 0
    .with_rawl(false)             // 1'b0 = false
    .with_wablm(u3::new(0))       // 3'b000 = 0
    .with_wabl(false)              // 1'b0 = false
    .with_emas(false)             // 1'b0 = false
    .with_emaw(u2::new(0))        // 2'b00 = 0
    .with_emab(u3::new(2))        // 3'b010 = 2
    .with_ema(u3::new(1))         // 3'b001 = 1
    .with_target(23); // decimal 23

/// SRAM instance names and their target IDs for reference
pub const SRAM_INSTANCES: [(u8, &str); 27] = [
    (26, "ACRAM2KX64"),
    (27, "AORAM1KX36"),
    (19, "BIORAM1KX32"),
    (6, "DTCM8KX36"),
    (8, "IFRAM32KX36"),
    (7, "ITCM32KX18"),
    (0, "RAM32KX72"),
    (1, "RAM8KX72"),
    (5, "RF128X31"),
    (2, "RF1KX72"),
    (3, "RF256X27"),
    (4, "RF512X39"),
    (11, "SCEAESRAM1K"),
    (13, "SCEALURAM3K"),
    (10, "SCEHASHRAM3K"),
    (12, "SCEPKERAM4K"),
    (9, "SCESCERAM10K"),
    (22, "FIFO32X19"),
    (17, "RDRAM128X22"),
    (15, "RDRAM1KX32"),
    (16, "RDRAM512X64"),
    (21, "RXFIFO128X32"),
    (14, "SCEMIMMDPRAM"),
    (20, "TXFIFO128X32"),
    (25, "UDCMEM256X64"),
    (24, "UDCMEMODB1088X64"),
    (23, "UDCMEMSHARE1088X64"),
];

/// SRAM trim configurations for 0.8V operation
pub const SRAM_TRIM_08V: [SramTrimSpec; 27] = [
    ACRAM2KX64_TRIM_08V,
    AORAM1KX36_TRIM_08V,
    BIORAM1KX32_TRIM_08V,
    DTCM8KX36_TRIM_08V,
    IFRAM32KX36_TRIM_08V,
    ITCM32KX18_TRIM_08V,
    RAM32KX72_TRIM_08V,
    RAM8KX72_TRIM_08V,
    RF128X31_TRIM_08V,
    RF1KX72_TRIM_08V,
    RF256X27_TRIM_08V,
    RF512X39_TRIM_08V,
    SCEAESRAM1K_TRIM_08V,
    SCEALURAM3K_TRIM_08V,
    SCEHASHRAM3K_TRIM_08V,
    SCEPKERAM4K_TRIM_08V,
    SCESCERAM10K_TRIM_08V,
    FIFO32X19_TRIM_08V,
    RDRAM128X22_TRIM_08V,
    RDRAM1KX32_TRIM_08V,
    RDRAM512X64_TRIM_08V,
    RXFIFO128X32_TRIM_08V,
    SCEMIMMDPRAM_TRIM_08V,
    TXFIFO128X32_TRIM_08V,
    UDCMEM256X64_TRIM_08V,
    UDCMEMODB1088X64_TRIM_08V,
    UDCMEMSHARE1088X64_TRIM_08V,
];

/// SRAM trim configurations for 0.9V operation
pub const SRAM_TRIM_09V: [SramTrimSpec; 27] = [
    ACRAM2KX64_TRIM_09V,
    AORAM1KX36_TRIM_09V,
    BIORAM1KX32_TRIM_09V,
    DTCM8KX36_TRIM_09V,
    IFRAM32KX36_TRIM_09V,
    ITCM32KX18_TRIM_09V,
    RAM32KX72_TRIM_09V,
    RAM8KX72_TRIM_09V,
    RF128X31_TRIM_09V,
    RF1KX72_TRIM_09V,
    RF256X27_TRIM_09V,
    RF512X39_TRIM_09V,
    SCEAESRAM1K_TRIM_09V,
    SCEALURAM3K_TRIM_09V,
    SCEHASHRAM3K_TRIM_09V,
    SCEPKERAM4K_TRIM_09V,
    SCESCERAM10K_TRIM_09V,
    FIFO32X19_TRIM_09V,
    RDRAM128X22_TRIM_09V,
    RDRAM1KX32_TRIM_09V,
    RDRAM512X64_TRIM_09V,
    RXFIFO128X32_TRIM_09V,
    SCEMIMMDPRAM_TRIM_09V,
    TXFIFO128X32_TRIM_09V,
    UDCMEM256X64_TRIM_09V,
    UDCMEMODB1088X64_TRIM_09V,
    UDCMEMSHARE1088X64_TRIM_09V,
];

/// Helper function to get the appropriate SRAM trim array based on voltage
///
/// # Arguments
/// * `voltage_mv` - Voltage in millivolts (e.g., 800 for 0.8V, 900 for 0.9V)
///
/// # Returns
/// * Reference to the appropriate SRAM trim configuration array
///
/// # Example
/// ```
/// let voltage_mv = 900; // 0.9V
/// let trim_configs = get_sram_trim_for_voltage(voltage_mv);
/// for config in trim_configs {
///     // Apply each SRAM trim configuration
///     apply_sram_trim(config);
/// }
/// ```
pub fn get_sram_trim_for_voltage(voltage_mv: u32) -> &'static [SramTrimSpec] {
    match voltage_mv {
        0..=850 => &SRAM_TRIM_08V[..], // Use 0.8V settings for voltages up to 850mV
        _ => &SRAM_TRIM_09V[..],       // Use 0.9V settings for voltages above 850mV
    }
}
