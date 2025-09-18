pub(crate) const PANIC_STD_SERVER: &'static str = "panic-to-screen!";

#[cfg(any(feature = "precursor", feature = "renode"))]
mod betrusted;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) use betrusted::*;
#[cfg(feature = "bao1x")]
mod bao1x;
#[cfg(feature = "bao1x")]
pub(crate) use bao1x::*;
