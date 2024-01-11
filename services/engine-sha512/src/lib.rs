#![cfg_attr(target_os = "none", no_std)]

mod api;
pub use api::*;
mod consts;

mod sha256;
#[cfg(target_os = "xous")]
mod sha512;
#[cfg(not(target_os = "xous"))]
mod sha512_hosted;
#[cfg(not(target_os = "xous"))]
use sha512_hosted as sha512;

pub use digest::{self, Digest};
pub use sha256::{Sha224, Sha256};
pub use sha512::{Sha384, Sha512, Sha512Trunc224, Sha512Trunc256};
