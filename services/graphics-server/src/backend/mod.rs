#[cfg(all(not(target_os="xous"),not(target_os="macos")))]
mod minifb;
#[cfg(all(not(target_os="xous"),not(target_os="macos")))]
pub use crate::backend::minifb::*;

#[cfg(all(not(target_os="xous"),target_os="macos"))]
mod minifb_macos;
#[cfg(all(not(target_os="xous"),target_os="macos"))]
pub use crate::backend::minifb_macos::*;

#[cfg(any(feature="precursor", feature="renode"))]
mod betrusted;
#[cfg(any(feature="precursor", feature="renode"))]
pub use crate::backend::betrusted::*;
