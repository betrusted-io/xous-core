#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
pub mod debug;
#[cfg(feature = "axp2101")]
pub mod axp2101;
pub mod board;
#[cfg(feature = "camera-gc2145")]
pub mod gc2145;
pub mod ifram;
pub mod iox;
#[cfg(feature = "camera-ov2640")]
pub mod ov2640;
pub mod sce;
#[cfg(feature = "display-sh1107")]
pub mod sh1107;
pub mod shared_csr;
pub mod udma;
pub mod usb;
pub use shared_csr::*;
pub mod mbox;
