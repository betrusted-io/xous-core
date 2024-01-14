// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use crate::Endian;


pub trait PackedSize where Self: Sized {
    /// Number of bytes this struct packs to/from
    const BYTES: usize;
}

/// Trait that enables endian aware conversion to/from bytes for packable types
///
/// Mostly for primitives. Currently expected to accept/return arrays by proc macro.
/// Benchmarking showed allocating a new 1 to 8 byte array during to/from bytes
/// performed exactly the same as manual bit shifting or various other shenanigans.
///
/// TODO: Above perf statement likely only holds true up to a certain size. Currently
/// nested structs and packing/unpacking them to/from &mut [u8] is not supported.
pub trait PackedBytes<B>: PackedSize {
    type Error;
    fn to_bytes<En: Endian>(&self) -> Result<B, Self::Error>;
    fn from_bytes<En: Endian>(bytes: B) -> Result<Self, Self::Error>;
}

/// Trait that enables packing and unpacking to/from byte slices
///
/// This is the trait that the proc macro implements on structs. Supports arbitrary
/// field alignment - i.e. fields needn't start or end on 8-bit byte boundaries.
/// For example, 2 bools, a 10-bit number and a 4-bit number could be packed into
/// a pair of bytes such that the 10-bit number straddles them and doesn't align with
/// any byte boundaries
///
/// Allowing arbitray field alignment should be zero-cost as the field definitions
/// will all be constants.
pub trait Packed: PackedSize {
    type Error;
    fn pack(&self, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn unpack(bytes: &[u8]) -> Result<Self, Self::Error>;
}
