#[cfg(not(target_os = "none"))]
mod hostaudio;
#[cfg(not(target_os = "none"))]
pub use crate::backend::hostaudio::*;

#[cfg(target_os = "none")]
mod tlv320aic3100;
#[cfg(target_os = "none")]
pub(crate) use crate::backend::tlv320aic3100::*;
