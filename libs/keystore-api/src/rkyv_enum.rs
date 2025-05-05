// As of Rust 1.64.0:
//
// Rkyv-derived enums throw warnings that rkyv::Archive derived enums are never used
// and I can't figure out how to make them go away. Since they spam the build log,
// rkyv-derived enums are now isolated to their own file with a file-wide `dead_code`
// allow on top.
//
// This might be a temporary compiler regression, or it could just
// be yet another indicator that it's time to upgrade rkyv. However, we are waiting
// until rkyv hits 0.8 (the "shouldn't ever change again but still not sure enough
// for 1.0") release until we rework the entire system to chase the latest rkyv.
// As of now, the current version is 0.7.x and there isn't a timeline yet for 0.8.

#![allow(dead_code)]
use zeroize::Zeroize;

use crate::PAR_BLOCKS;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Zeroize)]
#[zeroize(drop)]
pub enum AesBlockType {
    SingleBlock([u8; 16]),
    ParBlock([[u8; 16]; PAR_BLOCKS]),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq, Copy, Clone)]
pub enum KeywrapError {
    InvalidDataSize,
    InvalidKekSize,
    InvalidOutputSize,
    IntegrityCheckFailed,
    /// this is a bodge to return an error code that upgrades from a faulty early version of AES-KWP
    /// only works for 256-bit keys, but that is also all we used.
    /// The return tuple is: (unwrapped key, correctly wrapped key)
    UpgradeToNew(([u8; 32], [u8; 40])),
}
