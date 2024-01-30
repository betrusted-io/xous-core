#[cfg(all(not(target_os = "xous")))]
mod minifb;
#[cfg(all(not(target_os = "xous")))]
pub use crate::backend::minifb::*;

#[cfg(any(feature = "precursor", feature = "renode"))]
mod betrusted;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub use crate::backend::betrusted::*;

#[cfg(feature = "cramium-soc")]
mod cramium;
#[cfg(feature = "cramium-soc")]
pub use crate::backend::cramium::*;
