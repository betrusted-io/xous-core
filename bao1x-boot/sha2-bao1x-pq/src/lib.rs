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
    block_api::CtOutWrapper,
    consts::{U28, U32, U48, U64},
};

/// Block-level types
pub mod block_api;

#[cfg(feature = "debug")]
mod debug;
mod sha256;
mod sha512;

pub use block_api::{Sha256VarCore, Sha512VarCore};
#[cfg(feature = "compress")]
pub use sha256::compress256;
#[cfg(feature = "compress")]
pub use sha512::compress512;

digest::buffer_fixed!(
    /// SHA-256 hasher.
    pub struct Sha256(CtOutWrapper<block_api::Sha256VarCore, U32>);
    oid: "2.16.840.1.101.3.4.2.1";
    impl: BaseFixedTraits AlgorithmName Default HashMarker
        Reset FixedOutputReset ZeroizeOnDrop;
);
digest::buffer_fixed!(
    /// SHA-512 hasher.
    pub struct Sha512(CtOutWrapper<block_api::Sha512VarCore, U64>);
    oid: "2.16.840.1.101.3.4.2.3";
    impl: BaseFixedTraits AlgorithmName Default HashMarker
        Reset FixedOutputReset ZeroizeOnDrop;
);
