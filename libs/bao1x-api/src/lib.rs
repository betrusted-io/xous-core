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
pub mod pubkeys;

/// UF2 Family ID. Randomly generated, no collisions with the known list, still to be merged
/// into the "official" list
pub const BAOCHIP_1X_UF2_FAMILY: u32 = 0xa7d7_6373;

// system preemption interval
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

/// Constants used by both emulation and hardware implementations
pub const PERCLK: u32 = 100_000_000;
pub const SERVER_NAME_KBD: &str = "_Matrix keyboard driver_";
/// Do not change this constant, it is hard-coded into libraries in order to break
/// circular dependencies on the IFRAM block.
pub const SERVER_NAME_BAO1X_HAL: &str = "_bao1x-SoC HAL_";

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
