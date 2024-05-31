pub(crate) const PANIC_STD_SERVER: &'static str = "panic-to-screen!";

#[cfg(any(feature = "precursor", feature = "renode"))]
mod betrusted;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) use betrusted::*;
#[cfg(feature = "cramium-soc")]
mod cramium;
#[cfg(feature = "cramium-soc")]
pub(crate) use cramium::*;
