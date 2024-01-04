#![cfg_attr(any(target_os = "none", not(feature = "std")), no_std)]
pub mod generated;
#[cfg(not(any(windows, unix)))]
pub use generated::*;
