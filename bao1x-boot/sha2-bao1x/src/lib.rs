#![no_std]
//! This implementation is only suitable for no-std.
//!
//! DO NOT USE IN ENVIRONMENTS WITH CONCURRENCY OR VIRTUAL MEMORY!!!
//! This is a key assumption of many `unsafe` blocks in this implementation.
//!
//! See the `sha2` forked crate in betrusted-io for a `std`-capable implementation
//! that can handle the concurrency issues present in `std`

#[cfg(feature = "oid")]
use digest::const_oid::{AssociatedOid, ObjectIdentifier};
pub use digest::{self, Digest};
use digest::{
    consts::{U32, U64},
    core_api::{CoreWrapper, CtVariableCoreWrapper},
    impl_oid_carrier,
};

mod core_api;
mod debug;
mod sha256;
mod sha512;

pub use core_api::{Sha256VarCore, Sha512VarCore};
#[cfg(feature = "compress")]
pub use sha256::compress256;
#[cfg(feature = "compress")]
pub use sha512::compress512;

impl_oid_carrier!(OidSha256, "2.16.840.1.101.3.4.2.1");
impl_oid_carrier!(OidSha512, "2.16.840.1.101.3.4.2.3");

/// SHA-256 hasher.
pub type Sha256 = CoreWrapper<CtVariableCoreWrapper<Sha256VarCore, U32, OidSha256>>;
/// SHA-512 hasher.
pub type Sha512 = CoreWrapper<CtVariableCoreWrapper<Sha512VarCore, U64, OidSha512>>;
