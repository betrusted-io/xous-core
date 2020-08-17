#[cfg(not(target_os = "none"))]
mod minifb;
#[cfg(not(target_os = "none"))]
pub use crate::backend::minifb::*;

#[cfg(target_os = "none")]
mod betrusted;
#[cfg(target_os = "none")]
pub use crate::backend::betrusted::*;
