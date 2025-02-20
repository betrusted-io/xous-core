#[cfg(any(feature = "board-baosec", feature = "hosted-baosec", feature = "loader-baosec"))]
mod baosec;
#[cfg(any(feature = "board-baosec", feature = "hosted-baosec", feature = "loader-baosec"))]
pub use baosec::*;

#[cfg(any(feature = "board-baosor", feature = "loader-baosor"))]
mod baosor;
#[cfg(any(feature = "board-baosor", feature = "loader-baosor"))]
pub use baosor::*;
