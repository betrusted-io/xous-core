#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod hostaudio;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub use crate::backend::hostaudio::*;

#[cfg(any(target_os = "none", target_os = "xous"))]
mod tlv320aic3100;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub(crate) use crate::backend::tlv320aic3100::*;
