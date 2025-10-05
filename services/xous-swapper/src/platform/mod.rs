#[cfg(any(feature = "bao1x"))]
pub mod bao1x;
#[cfg(any(feature = "bao1x"))]
pub use bao1x::hw::*;

#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod precursor;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub use precursor::hw::*;
