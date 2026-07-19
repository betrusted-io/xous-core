use crate::fors::ForsParams;
use hybrid_array::{Array, ArraySize, typenum::Unsigned};

// Algorithm 3
pub(crate) fn base_2b<OutLen: ArraySize, B: Unsigned>(x: &[u8]) -> Array<u16, OutLen> {
    debug_assert!(x.len() >= (OutLen::USIZE * B::USIZE).div_ceil(8));
    debug_assert!(B::USIZE <= 16);

    let mut bits = 0usize;
    let mut i = 0;
    let mut total = 0usize;

    Array::<u16, OutLen>::from_fn(|_: usize| {
        while bits < B::USIZE {
            total = (total << 8) + x[i] as usize;
            bits += 8;
            i += 1;
        }
        bits -= B::USIZE;
        let out = (total >> bits) & ((1 << B::U8) - 1);
        total &= (1 << bits) - 1; // Deviation from spec pseudocode - clear used component to prevent usize overflow
        out.try_into().expect("B is less than 16")
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
    }
}
