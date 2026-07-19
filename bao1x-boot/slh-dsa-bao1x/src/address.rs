//! Hash address definitions and serialization
//!
//!  From FIPS-205 section 4.2:
//! > An ADRS
//! > consists of public values that indicate the position of the value being computed by the function. A
//! > different ADRS value is used for each call to each function. In the case of PRF, this is in order
//! > to generate a large number of different secret values from a single seed. In the case of Tℓ, H, and
//! > F, it is used to mitigate multi-target attacks.
//!
//! Address fields are big-endian integers. We use zero-copyable structs to represent the addresses
//! and serialize transparently to bytes using the `zerocopy` crate.
//!
//! Note that `tree_adrs_high` is unused in all parameter sets currently defined by FIPS-205
//!
//! Rather than implementing a generic `setTypeAndClear` as specified in FIPS-205, we define specific transitions for those
//! address conversions which are actually used.

use hybrid_array::Array;
use typenum::U22;

use zerocopy::{
    Immutable, IntoBytes,
    byteorder::big_endian::{U32, U64},
};

/// `Address` represents a hash address as defined by FIPS-205 section 4.2
pub(crate) trait Address: AsRef<[u8]> {
    const TYPE_CONST: u32;

    #[allow(clippy::doc_markdown)] // False positive
    /// Returns the address as a compressed 22-byte array
    /// ADRSc = ADRS[3] ∥ ADRS[8 : 16] ∥ ADRS[19] ∥ ADRS[20 : 32]
    fn compressed(&self) -> Array<u8, U22> {
        let bytes = self.as_ref();
        let mut compressed = Array::<u8, U22>::default();
        compressed[0] = bytes[3];
        compressed[1..9].copy_from_slice(&bytes[8..16]);
        compressed[9] = bytes[19];
        compressed[10..22].copy_from_slice(&bytes[20..32]);
        compressed
    }
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct WotsHash {
    pub(crate) layer_adrs: U32,
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 0
    pub(crate) key_pair_adrs: U32,
    pub(crate) chain_adrs: U32,
    pub(crate) hash_adrs: U32,
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct WotsPk {
    pub(crate) layer_adrs: U32,
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 1
    pub(crate) key_pair_adrs: U32,
    padding: U64, // 0
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct HashTree {
    pub(crate) layer_adrs: U32,
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 2
    padding: U32,    // 0
    pub(crate) tree_height: U32,
    pub(crate) tree_index: U32,
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct ForsTree {
    layer_adrs: U32, // 0
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 3
    pub(crate) key_pair_adrs: U32,
    pub(crate) tree_height: U32,
    pub(crate) tree_index: U32,
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct ForsRoots {
    layer_adrs: U32, // 0
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 4
    pub(crate) key_pair_adrs: U32,
    padding: U64, // 0
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct WotsPrf {
    pub(crate) layer_adrs: U32,
    pub(crate) tree_adrs_high: U32,
    pub(crate) tree_adrs_low: U64,
    type_const: U32, // 5
    pub(crate) key_pair_adrs: U32,
    pub(crate) chain_adrs: U32,
    hash_adrs: U32, // 0
}

#[derive(Clone, IntoBytes, Immutable)]
#[repr(C)]
pub(crate) struct ForsPrf {
    layer_adrs: U32, // 0
    pub tree_adrs_high: U32,
    pub tree_adrs_low: U64,
    type_const: U32, // 6
    pub key_pair_adrs: U32,
    tree_height: U32, // 0
    pub tree_index: U32,
}

impl Address for WotsHash {
    const TYPE_CONST: u32 = 0;
}
impl AsRef<[u8]> for WotsHash {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for WotsPk {
    const TYPE_CONST: u32 = 1;
}
impl AsRef<[u8]> for WotsPk {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for HashTree {
    const TYPE_CONST: u32 = 2;
}
impl AsRef<[u8]> for HashTree {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for ForsTree {
    const TYPE_CONST: u32 = 3;
}
impl AsRef<[u8]> for ForsTree {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for ForsRoots {
    const TYPE_CONST: u32 = 4;
}
impl AsRef<[u8]> for ForsRoots {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for WotsPrf {
    const TYPE_CONST: u32 = 5;
}
impl AsRef<[u8]> for WotsPrf {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Address for ForsPrf {
    const TYPE_CONST: u32 = 6;
}
impl AsRef<[u8]> for ForsPrf {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl WotsHash {
    pub(crate) fn prf_adrs(&self) -> WotsPrf {
        WotsPrf {
            layer_adrs: self.layer_adrs,
            tree_adrs_low: self.tree_adrs_low,
            tree_adrs_high: self.tree_adrs_high,
            type_const: WotsPrf::TYPE_CONST.into(),
            key_pair_adrs: self.key_pair_adrs,
            chain_adrs: 0.into(),
            hash_adrs: 0.into(),
        }
    }

    pub(crate) fn pk_adrs(&self) -> WotsPk {
        WotsPk {
            layer_adrs: self.layer_adrs,
            tree_adrs_low: self.tree_adrs_low,
            tree_adrs_high: self.tree_adrs_high,
            type_const: WotsPk::TYPE_CONST.into(),
            key_pair_adrs: self.key_pair_adrs,
            padding: 0.into(),
        }
    }

    pub(crate) fn tree_adrs(&self) -> HashTree {
        HashTree {
            layer_adrs: self.layer_adrs,
            tree_adrs_low: self.tree_adrs_low,
            tree_adrs_high: self.tree_adrs_high,
            type_const: HashTree::TYPE_CONST.into(),
            padding: 0.into(),
            tree_height: 0.into(),
            tree_index: 0.into(),
        }
    }
}

impl ForsTree {
    pub(crate) fn new(tree_adrs_low: u64, key_pair_adrs: u32) -> ForsTree {
        ForsTree {
            layer_adrs: 0.into(),
            tree_adrs_low: tree_adrs_low.into(),
            tree_adrs_high: 0.into(),
            type_const: ForsTree::TYPE_CONST.into(),
            key_pair_adrs: key_pair_adrs.into(),
            tree_height: 0.into(),
            tree_index: 0.into(),
        }
    }
    pub(crate) fn prf_adrs(&self) -> ForsPrf {
        ForsPrf {
            layer_adrs: 0.into(),
            tree_adrs_low: self.tree_adrs_low,
            tree_adrs_high: self.tree_adrs_high,
            type_const: ForsPrf::TYPE_CONST.into(),
            key_pair_adrs: self.key_pair_adrs,
            tree_height: 0.into(),
            tree_index: self.tree_index,
        }
    }

    pub(crate) fn fors_roots(&self) -> ForsRoots {
        ForsRoots {
            layer_adrs: 0.into(),
            tree_adrs_low: self.tree_adrs_low,
            tree_adrs_high: self.tree_adrs_high,
            type_const: ForsRoots::TYPE_CONST.into(),
            key_pair_adrs: self.key_pair_adrs,
            padding: 0.into(),
        }
    }
}

impl Default for WotsHash {
    fn default() -> Self {
        WotsHash {
            layer_adrs: 0.into(),
            tree_adrs_low: 0.into(),
            tree_adrs_high: 0.into(),
            type_const: 0.into(),
            key_pair_adrs: 0.into(),
            chain_adrs: 0.into(),
            hash_adrs: 0.into(),
        }
    }
}
