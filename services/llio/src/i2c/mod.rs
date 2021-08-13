#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(not(target_os = "none"), allow(unused_imports))]
#![cfg_attr(not(target_os = "none"), allow(unused_variables))]

#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod hosted;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub use crate::i2c::hosted::*;

#[cfg(any(target_os = "none", target_os = "xous"))]
mod hardware;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub(crate) use crate::i2c::hardware::*;
