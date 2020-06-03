#![cfg_attr(not(any(windows,unix)), no_std)]

#[macro_use]
extern crate bitflags;
extern crate num_derive;
extern crate num_traits;

#[cfg(not(any(windows,unix)))]
pub mod native;
#[cfg(not(any(windows,unix)))]
pub use native::*;

#[cfg(any(windows,unix))]
pub mod hosted;
#[cfg(any(windows,unix))]
pub use hosted::*;

pub mod definitions;
pub mod syscall;

pub use definitions::*;
pub use syscall::*;

/// Convert a four-letter string into a 32-bit int.
#[macro_export]
macro_rules! make_name {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c) as usize
    }};
}
