#![cfg_attr(not(feature = "std"), no_std)]

pub mod udma;
pub use udma::*;
// A bunch of these are gated on "std" because they include APIs that
// aren't available in e.g. kernel or loader.
pub mod iox;
pub use iox::*;
pub mod i2c;
pub use i2c::*;
pub mod api;
pub use api::*;
#[cfg(feature = "std")]
pub mod keyboard;
pub mod offsets;
pub mod sce;
pub mod signatures;
pub use offsets::*;
pub mod clocks;
pub mod pubkeys;
use arbitrary_int::u31;
use bitbybit::bitfield;
pub use clocks::*;
#[cfg(feature = "std")]
pub mod bio;

/// UF2 Family ID. Randomly generated, no collisions with the known list, still to be merged
/// into the "official" list
pub const BAOCHIP_1X_UF2_FAMILY: u32 = 0xa7d7_6373;

// density 18, memory type 20, mfg ID C2 ==> MX25L128833F
// density 38, memory type 25, mfg ID C2 ==> MX25U12832F
// mfg ID 0b ==> XT25Q64FWOIGT cost down option (8MiB)
pub const SPI_FLASH_IDS: [u32; 3] = [0x1820c2, 0x3825c2, 0x17600b];
// KGD 5D, mfg ID 9D; remainder of bits are part of the EID
pub const RAM_IDS: [u32; 2] = [0x5D9D, 0x559d];

/// system preemption interval
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 10;

/// standard baud rate
pub const UART_BAUD: u32 = 1_000_000;

/// Constants used by both emulation and hardware implementations
pub const PERCLK: u32 = 100_000_000;
pub const SERVER_NAME_KBD: &str = "_Matrix keyboard driver_";
/// Do not change this constant, it is hard-coded into libraries in order to break
/// circular dependencies on the IFRAM block.
pub const SERVER_NAME_BAO1X_HAL: &str = "_bao1x-SoC HAL_";

/// Flags register in the backup register bank. Used to track system state between soft resets.
#[bitfield(u32)]
#[derive(PartialEq, Eq, Debug)]
pub struct BackupFlags {
    #[bits(1..=31, rw)]
    reserved: u31,
    /// When `false`, indicates that the time in the RTC register is not synchronized to the offset
    /// that is read from disk. Upon first encounter with an external time source, the offset should
    /// be captured and recorded to disk.
    #[bit(0, rw)]
    rtc_synchronized: bool,
}

pub mod camera {
    #[derive(Clone, Copy)]
    pub enum Resolution {
        Res480x272,
        Res640x480,
        Res320x240,
        Res160x120,
        Res256x256,
    }

    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum Format {
        Rgb565 = 0,
        Rgb555 = 1,
        Rgb444 = 2,
        BypassLe = 4,
        BypassBe = 5,
    }

    impl Into<(usize, usize)> for Resolution {
        fn into(self) -> (usize, usize) {
            match self {
                Resolution::Res160x120 => (160, 120),
                Resolution::Res320x240 => (320, 240),
                Resolution::Res480x272 => (480, 272),
                Resolution::Res640x480 => (640, 480),
                Resolution::Res256x256 => (256, 256),
            }
        }
    }
}

/// Version number of the below structure
pub const STATICS_IN_ROM_VERSION: u16 = 1;
/// This encodes to jal x0, 256 - jumps 256 bytes ahead from the current PC location.
pub const JUMP_INSTRUCTION: u32 = 0x1000006f;
/// In-ROM representation of static initialization data
/// Placed by the image creation tool, and used for bootstrapping the Rust environment
/// `usize` is *not* allowed because this structure is packed on a 64-bit host.
#[repr(C, align(256))]
pub struct StaticsInRom {
    // reserved for a jump-over instruction so that the structure can be located in-line in ROM.
    #[allow(dead_code)] // this should never actually be used
    pub jump_instruction: u32,
    // version number of this structure
    pub version: u16,
    // total number of valid pokes in `poke_table`
    pub valid_pokes: u16,
    // Origin of the data segment
    pub data_origin: u32,
    // overall size in bytes. [origin:origin+size] will be zeroized.
    pub data_size_bytes: u32,
    // poke table of values to stick in the data segment. This is needed in particular
    // to initialize `static` variables, such as Atomics and Mutexes, required by the
    // loader environment. Presented as (address, data) tuples, up to 40 of them.
    // Only entries from [0..valid_pokes] are processed.
    // The addresses and data are packed as u8's to avoid padding of the record.
    pub poke_table: [([u8; 2], [u8; 4]); 40],
}

impl StaticsInRom {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, core::mem::size_of::<Self>()) }
    }
}

/// Structure for recording message formats to be passed from interrupt handlers back to userspace.
/// A specific handler may or may not use any or all of the arguments: this simply provides storage
/// for all the possible arguments.
#[derive(Copy, Clone)]
pub struct IrqNotification {
    /// Specifies the bit position of the event in the irq bank
    pub bit: arbitrary_int::u4,
    /// Connection to send notifications to
    pub conn: xous::CID,
    /// Opcode argument for the notification
    pub opcode: usize,
    /// Up to four arguments to be passed on
    pub args: [usize; 4],
}

#[macro_export]
macro_rules! bollard {
    // A call with no args just inserts 4 illegal instructions
    () => {
        bollard!(4)
    };

    // A call with countermeasures specified interleaves countermeasures with illegal instructions
    // Interleaving is done because the countermeasure jump is a single point of failure
    // that could be bypassed. Leaving illegal instructions in hardens against that possibility.
    //
    // Note that the countermeasure routine needs to be within +/-1 MiB of the bollard
    ($countermeasure:path, $count:literal) => {{
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        unsafe {
            core::arch::asm!(
                "    j 2f",
                "1:",
                ".rept {count}",
                "    j {cm}",
                // this is an "invalid opcode" -- will trigger an instruction page fault
                "   .word 0xffffffff",
                ".endr",
                "2:",
                cm = sym $countermeasure,
                count = const $count,
                options(nomem, nostack, preserves_flags)
            );
        }

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }};
    ($count:literal) => {{
        // Force a compiler barrier to prevent reordering around the tripwire
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        unsafe {
            core::arch::asm!(
                "j 2f",
                // Label 1: illegal instruction sled
                "1:",
                ".rept {count}",
                // this is an "invalid opcode" -- will trigger an instruction page fault
                ".word 0xffffffff",
                ".endr",
                // Label 2: safe landing
                "2:",
                count = const $count,
                options(nomem, nostack, preserves_flags)
            );
        }

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }};
}

/// Hardened boolean type - values chosen for high Hamming distance
/// and resistance to stuck-at-zero/one faults.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct HardenedBool(u32);

impl HardenedBool {
    pub const FALSE: Self = Self(Self::FALSE_VALUE);
    const FALSE_VALUE: u32 = 0xA5A5_5A5A;
    pub const TRUE: Self = Self(Self::TRUE_VALUE);
    // Hamming distance of 16 between these values
    // Also chosen to not be simple patterns (0x0000, 0xFFFF, etc.)
    const TRUE_VALUE: u32 = 0x5A5A_A5A5;

    /// Check if true - returns None if value is corrupted
    #[inline(never)]
    pub fn is_true(self) -> Option<bool> {
        match self.0 {
            Self::TRUE_VALUE => Some(true),
            Self::FALSE_VALUE => Some(false),
            _ => None, // Corruption detected
        }
    }

    /// Constant-time equality check against TRUE
    #[inline(never)]
    pub fn check_true(self) -> bool {
        // Use volatile to prevent optimizer from simplifying
        let val = unsafe { core::ptr::read_volatile(&self.0) };
        val == Self::TRUE_VALUE
    }

    /// Return the complement - for redundant checking
    pub fn complement(self) -> u32 { !self.0 }

    /// Verify internal consistency (value is one of the two valid states)
    pub fn is_valid(self) -> bool { self.0 == Self::TRUE_VALUE || self.0 == Self::FALSE_VALUE }
}
