#[cfg(any(feature = "cramium-soc"))]
pub mod cramium;
#[cfg(any(feature = "cramium-soc"))]
pub use cramium::hw::*;

#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod precursor;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub use precursor::hw::*;
