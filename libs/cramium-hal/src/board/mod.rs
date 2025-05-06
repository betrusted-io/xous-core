#[cfg(any(
    feature = "board-baosec",
    feature = "loader-baosec",
    feature = "test-baosec",
    feature = "kernel-baosec"
))]
pub mod baosec;
#[cfg(any(
    feature = "board-baosec",
    feature = "loader-baosec",
    feature = "test-baosec",
    feature = "kernel-baosec"
))]
pub use baosec::*;
#[cfg(any(feature = "board-baosor", feature = "loader-baosor"))]
pub mod baosor;
#[cfg(any(feature = "board-baosor", feature = "loader-baosor"))]
pub use baosor::*;
#[cfg(any(feature = "board-dabao", feature = "loader-dabao"))]
pub mod dabao;
#[cfg(any(feature = "board-dabao", feature = "loader-dabao"))]
pub use dabao::*;
