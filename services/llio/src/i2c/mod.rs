#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(not(target_os = "none"), allow(unused_imports))]
#![cfg_attr(not(target_os = "none"), allow(unused_variables))]

#[cfg(not(target_os = "xous"))]
mod hosted;
#[cfg(not(target_os = "xous"))]
pub use crate::i2c::hosted::*;

#[cfg(any(feature = "precursor", feature = "renode"))]
mod hardware;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) use crate::i2c::hardware::*;
