#[cfg(any(feature="hosted"))]
mod hostaudio;
#[cfg(any(feature="hosted"))]
pub use crate::backend::hostaudio::*;

#[cfg(any(feature="precursor", feature="renode"))]
mod tlv320aic3100;
#[cfg(any(feature="precursor", feature="renode"))]
pub(crate) use crate::backend::tlv320aic3100::*;
