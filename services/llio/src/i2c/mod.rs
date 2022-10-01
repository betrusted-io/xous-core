#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(not(target_os = "none"), allow(unused_imports))]
#![cfg_attr(not(target_os = "none"), allow(unused_variables))]

#[cfg(any(feature="hosted"))]
mod hosted;
#[cfg(any(feature="hosted"))]
pub use crate::i2c::hosted::*;

#[cfg(any(feature="precursor", feature="renode"))]
mod hardware;
#[cfg(any(feature="precursor", feature="renode"))]
pub(crate) use crate::i2c::hardware::*;
