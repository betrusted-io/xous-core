#![cfg_attr(baremetal, no_std)]

#[macro_use]
extern crate bitflags;

pub mod arch;

pub mod carton;
pub mod definitions;
mod messages;
pub mod syscall;

pub use arch::{ProcessArgs, ProcessInit, ProcessKey, ThreadInit};
pub use definitions::*;
pub use messages::*;
pub use syscall::*;

#[cfg(not(baremetal))]
pub use arch::ProcessArgsAsThread;

/// Convert a four-letter string into a 32-bit int.
#[macro_export]
macro_rules! make_name {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c) as usize
    }};
}
