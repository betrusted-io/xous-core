#![no_std]

pub use typenum::{
    Unsigned, U0, U1, U2, U3, U4, U5, U6, U7, U8, U9, U10, U11, U12, U13, U14, U15, U16, U17, U18, U19, U20,
    U21, U22, U23, U24, U25, U26, U27, U28, U29, U30, U31, U32, U33, U34, U35, U36, U37, U38, U39, U40, U41,
    U42, U43, U44, U45, U46, U47, U48, U49, U50,
    IsLess, IsLessOrEqual, IsGreaterOrEqual, Cmp,
};

// Re-export the proc macro
pub use packing_codegen::Packed;

mod bit;
pub use bit::*;

mod endian;
pub use endian::*;

mod error;
pub use error::*;

mod packed;
pub use packed::*;

mod primitive_packing;
pub use primitive_packing::*;
