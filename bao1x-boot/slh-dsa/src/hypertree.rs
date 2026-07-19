use crate::signing_key::SkSeed;
use crate::{
    address::WotsHash,
    xmss::{XmssParams, XmssSig},
};
use core::fmt::Debug;
use hybrid_array::{Array, ArraySize};
use typenum::Unsigned;

#[derive(Clone, Debug)]
pub(crate) struct HypertreeSig<P: HypertreeParams>(Array<XmssSig<P>, P::D>);

impl<P: HypertreeParams> PartialEq for HypertreeSig<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P: HypertreeParams> Eq for HypertreeSig<P> {}

impl<P: HypertreeParams> HypertreeSig<P> {
    pub(crate) const SIZE: usize = XmssSig::<P>::SIZE * P::D::USIZE;

    pub(crate) fn write_to(&self, buf: &mut [u8]) {
        debug_assert!(
            buf.len() == Self::SIZE,
            "HT serialize length mismatch: {}, {}",
            buf.len(),
            Self::SIZE
        );

        buf.chunks_exact_mut(XmssSig::<P>::SIZE)
            .zip(self.0.iter())
            .for_each(|(buf, sig)| sig.write_to(buf));
    }

    #[cfg(feature = "alloc")]
    pub(crate) fn to_vec(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec![0u8; Self::SIZE];
        self.write_to(&mut buf);
        buf
    }
}

impl<P: HypertreeParams> TryFrom<&[u8]> for HypertreeSig<P> {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::SIZE {
            return Err(());
        }
        let sig = value
            .chunks(XmssSig::<P>::SIZE)
            .map(|c| XmssSig::try_from(c).unwrap())
            .collect();
        Ok(HypertreeSig(sig))
    }
}

pub(crate) trait HypertreeParams: XmssParams + Sized {
    type D: ArraySize + Debug + Eq;
    type H: ArraySize; // HPrime * D

    fn ht_sign(
        &self,
        m: &Array<u8, Self::N>,
        sk_seed: &SkSeed<Self::N>,
        mut idx_tree: u64,
        mut idx_leaf: u32,
    ) -> HypertreeSig<Self> {
        let mut adrs = WotsHash::default();
        // Currently no parameter set supports more than 2^64 trees
        // So tree_adrs_high is always unset
        adrs.tree_adrs_low.set(idx_tree);

        // Pre-allocate the array - Option should have no overhead after optimization
        let mut sig = Array::<_, Self::D>::default();

        sig[0] = Some(self.xmss_sign(m, sk_seed, idx_leaf, &adrs));
        let mut root = self.xmss_pk_from_sig(idx_leaf, sig[0].as_ref().unwrap(), m, &adrs);

        for j in 1..Self::D::U32 {
            // H' least significant bits of idx_leaf. H' is always less than 32 in FIPS-205 parameter sets
            idx_leaf = (idx_tree & ((1 << Self::HPrime::U32) - 1))
                .try_into()
                .expect("H' is less than 32");
            idx_tree >>= Self::HPrime::U64;

            adrs.layer_adrs.set(j);
            adrs.tree_adrs_low.set(idx_tree);

            sig[j as usize] = Some(self.xmss_sign(&root, sk_seed, idx_leaf, &adrs));
            if j != Self::D::U32 - 1 {
                root = self.xmss_pk_from_sig(
                    idx_leaf,
                    sig[j as usize].as_ref().unwrap(),
                    &root,
                    &adrs,
                );
            }
        }
        // TODO: Validate that these clones get optimized away
        HypertreeSig(sig.iter().cloned().map(Option::unwrap).collect())
    }

    fn ht_verify(
        &self,
        m: &Array<u8, Self::N>,
        sig: &HypertreeSig<Self>,
        mut idx_tree: u64,
        mut idx_leaf: u32,
        pk_root: &Array<u8, Self::N>,
    ) -> bool {
        let mut adrs = WotsHash::default();
        adrs.tree_adrs_low.set(idx_tree);

        let mut root = self.xmss_pk_from_sig(idx_leaf, &sig.0[0], m, &adrs);

        for j in 1..Self::D::U32 {
            // H' least significant bits of idx_leaf. H' is always less than 32 in FIPS-205 parameter sets
            idx_leaf = (idx_tree & ((1 << Self::HPrime::U32) - 1))
                .try_into()
                .expect("H' is less than 32");
            idx_tree >>= Self::HPrime::U64;

            adrs.layer_adrs.set(j);
            adrs.tree_adrs_low.set(idx_tree);

            root = self.xmss_pk_from_sig(idx_leaf, &sig.0[j as usize], &root, &adrs);
        }
        &root == pk_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PkSeed, hashes::Shake128f, util::macros::test_parameter_sets};
    use hybrid_array::Array;
    use rand::RngExt;

    fn test_ht_sign_verify<HTMode: HypertreeParams>() {
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);
        let htmode = HTMode::new_from_pk_seed(&pk_seed);

        let mut m = Array::<u8, HTMode::N>::default();
        rng.fill(m.as_mut_slice());

        let idx_tree = rng.random_range(
            0..=(1u64
                .checked_shl(HTMode::H::U32 - HTMode::HPrime::U32)
                .unwrap_or(0)
                .wrapping_sub(1)),
        );
        let idx_leaf = rng.random_range(0..(1 << (HTMode::HPrime::USIZE)));

        let mut adrs = WotsHash::default();
        adrs.tree_adrs_low.set(0);
        adrs.layer_adrs.set(HTMode::D::U32 - 1);

        let pk_root = htmode.xmss_node(&sk_seed, 0, HTMode::HPrime::U32, &adrs);

        let sig = htmode.ht_sign(&m, &sk_seed, idx_tree, idx_leaf);

        assert!(htmode.ht_verify(&m, &sig, idx_tree, idx_leaf, &pk_root));
    }

    test_parameter_sets!(test_ht_sign_verify);

    fn test_ht_sign_verify_fail<HTMode: HypertreeParams>() {
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);
        let htmode = HTMode::new_from_pk_seed(&pk_seed);

        let mut m = Array::<u8, HTMode::N>::default();
        rng.fill(m.as_mut_slice());

        let idx_tree = rng.random_range(
            0..=(1u64
                .checked_shl(HTMode::H::U32 - HTMode::HPrime::U32)
                .unwrap_or(0)
                .wrapping_sub(1)),
        );
        let idx_leaf = rng.random_range(0..(1 << (HTMode::HPrime::USIZE)));

        let mut adrs = WotsHash::default();
        adrs.tree_adrs_low.set(0);
        adrs.layer_adrs.set(HTMode::D::U32 - 1);

        let pk_root = htmode.xmss_node(&sk_seed, 0, HTMode::HPrime::U32, &adrs);

        let sig = htmode.ht_sign(&m, &sk_seed, idx_tree, idx_leaf);

        // Tweak the message to ensure verification fails
        m[0] ^= 0xff; // Invert the first byte of the message

        // Verification should fail since the message was tweaked
        assert!(!htmode.ht_verify(&m, &sig, idx_tree, idx_leaf, &pk_root));
    }

    test_parameter_sets!(test_ht_sign_verify_fail);

    #[test]
    #[cfg(feature = "alloc")]
    fn test_ht_sign_kat() {
        use hex_literal::hex;
        use shake::{Shake256, digest::ExtendableOutput};

        let sk_seed = SkSeed(Array([1; 16]));
        let pk_seed = PkSeed(Array([2; 16]));
        let ht = Shake128f::new_from_pk_seed(&pk_seed);
        let m = Array([3; 16]);

        let sig = ht.ht_sign(&m, &sk_seed, 3, 5);

        let sig_flattened = sig.to_vec();

        // We compare H(sig) rather than the full sig for test case brevity
        let mut sig_hash = [0u8; 16];
        Shake256::digest_xof(sig_flattened, sig_hash.as_mut_slice());
        let expected = hex!("7daa15a56a5b51d42cd0ff6903f10702");

        assert_eq!(sig_hash, expected);
    }
}
