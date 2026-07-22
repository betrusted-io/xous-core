#![cfg_attr(rustfmt, rustfmt_skip)]
use crate::fors::ForsParams;
use hybrid_array::{Array, ArraySize, typenum::Unsigned};

/// A bound that is `Sync` when the `parallel` feature is enabled and empty
/// otherwise.
///
/// Used as a supertrait of `HashSuite` so the rayon fork-join Merkle tree
/// builds (see `xmss_node` / `fors_node`) can share `&self` across worker
/// threads, without imposing any `Sync` requirement on sequential / `no_std`
/// builds. The blanket impls make this invisible to implementors either way:
/// every hash suite is a plain-data struct and is automatically `Sync`.
#[cfg(feature = "parallel")]
pub(crate) trait MaybeSync: Sync {}
#[cfg(feature = "parallel")]
impl<T: Sync> MaybeSync for T {}

#[cfg(not(feature = "parallel"))]
pub(crate) trait MaybeSync {}
#[cfg(not(feature = "parallel"))]
impl<T> MaybeSync for T {}

// Algorithm 3
//
// Returns `u32` chunks (not `u16`). The SP 800-230 limited-signature parameter
// sets use FORS tree heights `a` of 24 and 25, so a single base-`2^b` chunk no
// longer fits in a `u16`. `b` is at most 25 across all supported parameter sets
// (FORS `a`), well within `u32`; the accumulator is `u64` to hold up to `b + 7`
// bits without overflow on 32-bit targets.
pub(crate) fn base_2b<OutLen: ArraySize, B: Unsigned>(x: &[u8]) -> Array<u32, OutLen> {
    debug_assert!(x.len() >= (OutLen::USIZE * B::USIZE).div_ceil(8));
    debug_assert!(B::USIZE <= 25);

    let mut bits = 0usize;
    let mut i = 0;
    let mut total: u64 = 0;

    Array::<u32, OutLen>::from_fn(|_: usize| {
        while bits < B::USIZE {
            total = (total << 8) + u64::from(x[i]);
            bits += 8;
            i += 1;
        }
        bits -= B::USIZE;
        let out = (total >> bits) & ((1u64 << B::U8) - 1);
        total &= (1u64 << bits) - 1; // Deviation from spec pseudocode - clear used component to prevent overflow
        u32::try_from(out).expect("B is at most 25, so the chunk fits in u32")
    })
}

/// Separates the digest into the FORS message, the Xmss tree index, and the Xmss leaf index.
pub(crate) fn split_digest<P: ForsParams>(
    digest: &Array<u8, P::M>,
) -> (&Array<u8, P::MD>, u64, u32) {
    #[allow(deprecated)]
    let m = Array::from_slice(&digest[..P::MD::USIZE]);
    let idx_tree_size = (P::H::USIZE - P::HPrime::USIZE).div_ceil(8);
    let idx_leaf_size = P::HPrime::USIZE.div_ceil(8);
    let mut idx_tree_bytes = [0u8; 8];
    let mut idx_leaf_bytes = [0u8; 4];
    idx_tree_bytes[8 - idx_tree_size..]
        .copy_from_slice(&digest[P::MD::USIZE..P::MD::USIZE + idx_tree_size]);
    idx_leaf_bytes[4 - idx_leaf_size..].copy_from_slice(
        &digest[P::MD::USIZE + idx_tree_size..P::MD::USIZE + idx_tree_size + idx_leaf_size],
    );

    // For 256-bit parameters sets, Self::H::U32 - Self::HPrime::U32 = 64
    let mask: u64 = 1u64
        .checked_shl(P::H::U32 - P::HPrime::U32)
        .unwrap_or(0)
        .wrapping_sub(1);
    let idx_tree = u64::from_be_bytes(idx_tree_bytes) & mask;
    let idx_leaf = u32::from_be_bytes(idx_leaf_bytes) & ((1 << P::HPrime::USIZE) - 1);
    (m, idx_tree, idx_leaf)
}

#[cfg(test)]
pub(crate) mod macros {
    /// Generate a test case
    #[macro_export]
    macro_rules! gen_test {
        ($name:ident, $t:ty) => {
            paste::paste! {
               #[test]
               fn [<$name _ $t:lower>]() {
                   $name::<$t>()
               }
            }
        };
    }

    macro_rules! test_parameter_sets {
        ($name:ident) => {
            #[allow(unused_imports)]
            use crate::hashes::*;
            crate::gen_test!($name, Shake128f);
            crate::gen_test!($name, Shake128s);
            crate::gen_test!($name, Shake192f);
            crate::gen_test!($name, Shake192s);
            crate::gen_test!($name, Shake256f);
            crate::gen_test!($name, Shake256s);

            crate::gen_test!($name, Sha2_128f);
            crate::gen_test!($name, Sha2_128s);
            crate::gen_test!($name, Sha2_192f);
            crate::gen_test!($name, Sha2_192s);
            crate::gen_test!($name, Sha2_256f);
            crate::gen_test!($name, Sha2_256s);

            // NIST SP 800-230 limited-signature (2^24) parameter sets
            crate::gen_test!($name, Shake128_24);
            crate::gen_test!($name, Sha2_128_24);
            #[cfg(feature = "sp800-230-highsec")]
            crate::gen_test!($name, Shake192_24);
            #[cfg(feature = "sp800-230-highsec")]
            crate::gen_test!($name, Shake256_24);
            #[cfg(feature = "sp800-230-highsec")]
            crate::gen_test!($name, Sha2_192_24);
            #[cfg(feature = "sp800-230-highsec")]
            crate::gen_test!($name, Sha2_256_24);
        };
    }

    pub(crate) use test_parameter_sets;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_bigint::BoxedUint;
    use proptest::prelude::*;
    use typenum::U;

    fn test_base_2b<OutLen: ArraySize, B: Unsigned>(x: &[u8]) {
        if x.len() < (OutLen::USIZE * B::USIZE + 7) / 8 {
            return; // TODO: enforce this at the prop level
        }

        let a = base_2b::<OutLen, B>(x);
        let mut b = BoxedUint::from_be_slice_vartime(&x[..(OutLen::USIZE * B::USIZE + 7) / 8]);

        if (B::USIZE * OutLen::USIZE) % 8 != 0 {
            // Clear lower bits of b
            b >>= 8 - ((B::USIZE * OutLen::USIZE) % 8);
        }

        let c: BoxedUint = a.iter().fold(
            BoxedUint::zero_with_precision(b.bits_precision()),
            |acc, x| (acc << B::U32) + *x,
        );

        assert_eq!(b, c);
    }

    proptest! {
        // These are all the OutLen, B combinations used in the FIPS spec
        // TODO - explicitly tie to individual parameter sets

        #[test]
        fn test_base_2b_32_4(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<32>, U<4>>(&x);
        }

        #[test]
        fn test_base_2b_64_4(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<64>, U<4>>(&x);
        }

        #[test]
        fn test_base_2b_14_12(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<14>, U<12>>(&x);
        }

        #[test]
        fn test_base_2b_33_6(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<33>, U<6>>(&x);
        }

        #[test]
        fn test_base_2b_17_14(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<17>, U<14>>(&x);
        }

        #[test]
        fn test_base_2b_33_8(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<33>, U<8>>(&x);
        }

        #[test]
        fn test_base_2b_22_14(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<22>, U<14>>(&x);
        }

        #[test]
        fn test_base_2b_35_9(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<35>, U<9>>(&x);
        }

        // SP 800-230 FORS: k base-2^a chunks with a = 24 (L1) and a = 25 (L3/L5).
        // These exercise the u32 output path (a > 16).
        #[test]
        fn test_base_2b_6_24(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<6>, U<24>>(&x);
        }

        #[test]
        fn test_base_2b_9_25(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<9>, U<25>>(&x);
        }

        #[test]
        fn test_base_2b_12_25(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<12>, U<25>>(&x);
        }

        // SP 800-230 WOTS+ checksum expansion: len2 base-2^lg_w chunks.
        #[test]
        fn test_base_2b_4_2(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<4>, U<2>>(&x);
        }

        #[test]
        fn test_base_2b_5_2(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<5>, U<2>>(&x);
        }

        #[test]
        fn test_base_2b_3_3(x in prop::collection::vec(any::<u8>(), 0..100)){
            test_base_2b::<U<3>, U<3>>(&x);
        }
    }
}