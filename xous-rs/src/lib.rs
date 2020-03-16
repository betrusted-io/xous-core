#![no_std]

#[macro_use]
extern crate bitflags;
extern crate num_derive;
extern crate num_traits;

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
        usize::from_le_bytes(c)
    }};
}
