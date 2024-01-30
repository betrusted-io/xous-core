#[cfg(not(target_os = "xous"))]
mod hostaudio;
#[cfg(not(target_os = "xous"))]
pub use crate::backend::hostaudio::*;

#[cfg(any(feature = "precursor", feature = "renode"))]
mod tlv320aic3100;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) use crate::backend::tlv320aic3100::*;
