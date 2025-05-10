#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
#[cfg(not(feature = "hosted-baosec"))]
pub mod debug;
#[cfg(feature = "axp2101")]
pub mod axp2101;
pub mod board;
#[cfg(feature = "camera-gc2145")]
pub mod gc2145;
#[cfg(not(feature = "hosted-baosec"))]
pub mod ifram;
#[cfg(not(feature = "hosted-baosec"))]
pub mod iox;
#[cfg(feature = "camera-ov2640")]
pub mod ov2640;
#[cfg(not(feature = "hosted-baosec"))]
pub mod sce;
#[cfg(feature = "display-sh1107")]
pub mod sh1107;
#[cfg(not(feature = "hosted-baosec"))]
pub mod shared_csr;
#[cfg(not(feature = "hosted-baosec"))]
pub mod udma;
#[cfg(not(feature = "hosted-baosec"))]
pub mod usb;
#[cfg(not(feature = "hosted-baosec"))]
pub use shared_csr::*;
#[cfg(not(feature = "hosted-baosec"))]
pub mod mbox;
