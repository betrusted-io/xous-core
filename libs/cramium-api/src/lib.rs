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

/// Constants used by both emulation and hardware implementations
pub const PERCLK: u32 = 100_000_000;
pub const SERVER_NAME_KBD: &str = "_Matrix keyboard driver_";
/// Do not change this constant, it is hard-coded into libraries in order to break
/// circular dependencies on the IFRAM block.
pub const SERVER_NAME_CRAM_HAL: &str = "_Cramium-SoC HAL_";

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
