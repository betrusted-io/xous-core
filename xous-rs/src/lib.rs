#![cfg_attr(any(target_os = "none", target_os = "xous"), no_std)]

pub mod arch;

pub mod carton;
pub mod definitions;

pub mod process;
pub mod services;
pub mod string;
pub mod stringbuffer;
pub mod syscall;

pub use arch::{ProcessArgs, ProcessInit, ProcessKey, ProcessStartup, ThreadInit};
pub use definitions::*;
pub use string::*;
pub use stringbuffer::*;
pub use syscall::*;

#[cfg(feature = "processes-as-threads")]
pub use crate::arch::ProcessArgsAsThread;