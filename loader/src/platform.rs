#[cfg(any(feature = "precursor", feature = "renode"))]
mod precursor;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub use precursor::*;

#[cfg(any(feature = "cramium-soc", feature = "cramium-fpga"))]
mod cramium;
#[cfg(any(feature = "cramium-soc", feature = "cramium-fpga"))]
pub use cramium::*;

#[cfg(feature = "atsama5d27")]
pub mod atsama5d27;
#[cfg(feature = "atsama5d27")]
pub use self::atsama5d27::*;
