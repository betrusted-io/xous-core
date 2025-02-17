#[cfg(any(feature = "board-baosec", feature = "hosted-baosec"))]
mod baosec;
#[cfg(any(feature = "board-baosec", feature = "hosted-baosec"))]
pub use baosec::*;

#[cfg(feature = "board-baosor")]
mod baosor;
#[cfg(feature = "board-baosor")]
pub use baosor::*;
