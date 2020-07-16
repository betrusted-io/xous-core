#![cfg_attr(not(any(windows,unix)), no_std)]

#[macro_use]
extern crate bitflags;
extern crate num_derive;
extern crate num_traits;

pub mod arch;

pub mod definitions;
pub mod syscall;
pub mod carton;
mod messages;

pub use definitions::*;
pub use syscall::*;
pub use messages::*;
pub use arch::ContextInit;

/// Convert a four-letter string into a 32-bit int.
#[macro_export]
macro_rules! make_name {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c) as usize
    }};
}
