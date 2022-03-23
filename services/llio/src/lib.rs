#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;
pub use api::*;

pub mod i2c_lib;
pub use i2c_lib::I2c;
pub mod llio_lib;
pub use llio_lib::Llio;
