#![cfg_attr(rustfmt, rustfmt_skip)]
use core::fmt::Debug;

use crate::address::Address;
use crate::fors::ForsParams;
use crate::hashes::HashSuite;
use crate::hypertree::HypertreeParams;
use crate::wots::WotsParams;
use crate::xmss::XmssParams;
use crate::{ParameterSet, PkSeed, SkPrf, SkSeed};
use ::shake::Shake256;
use const_oid::db::fips205;
use digest::{ExtendableOutput, Update};
use hybrid_array::typenum::consts::{U16, U21, U30, U32};
#[cfg(feature = "sp800-230-highsec")]
use hybrid_array::typenum::consts::U41;
use hybrid_array::typenum::{U24, U34, U39, U42, U47, U49};
use hybrid_array::{Array, ArraySize};
use typenum::U;

/// Implementation of the component hash functions using SHAKE256
///
/// Follows section 11.1 of FIPS-205
#[derive(Debug, Clone)]
pub struct Shake<N, M> {
    _n: core::marker::PhantomData<N>,
    _m: core::marker::PhantomData<M>,
    cached_hasher: Shake256,
}

impl<N: ArraySize, M: ArraySize> HashSuite for Shake<N, M>
where
    // `Sync`: typenum size types are zero-sized and always Sync; the explicit
    // bound is needed because this impl is generic, so the PhantomData<N/M>
    // markers can't otherwise satisfy HashSuite's MaybeSync supertrait when the
    // `parallel` feature is enabled. Sync is a core trait: no_std-safe.
    N: Debug + Clone + PartialEq + Eq + Sync,
    M: Debug + Clone + PartialEq + Eq + Sync,
{
    type N = N;
    type M = M;

    fn new_from_pk_seed(pk_seed: &PkSeed<Self::N>) -> Self {
        Self {
            _n: core::marker::PhantomData,
            _m: core::marker::PhantomData,
            cached_hasher: Shake256::default().chain(pk_seed.as_ref()),
        }
    }

    fn prf_msg(
        sk_prf: &SkPrf<Self::N>,
        opt_rand: &Array<u8, Self::N>,
        msg: &[&[impl AsRef<[u8]>]],
    ) -> Array<u8, Self::N> {
        let mut hasher = Shake256::default()
            .chain(sk_prf.as_ref())
            .chain(opt_rand.as_slice());
        msg.iter()
            .copied()
            .flatten()
            .for_each(|msg_part| hasher.update(msg_part.as_ref()));
        let mut output = Array::<u8, Self::N>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }

    fn h_msg(
        rand: &Array<u8, Self::N>,
        pk_seed: &PkSeed<Self::N>,
        pk_root: &Array<u8, Self::N>,
        msg: &[&[impl AsRef<[u8]>]],
    ) -> Array<u8, Self::M> {
        let mut hasher = Shake256::default()
            .chain(rand.as_slice())
            .chain(pk_seed.as_ref())
            .chain(<Array<u8, N> as AsRef<[u8]>>::as_ref(pk_root));
        msg.iter()
            .copied()
            .flatten()
            .for_each(|msg_part| hasher.update(msg_part.as_ref()));
        let mut output = Array::<u8, Self::M>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }

    fn prf_sk(&self, sk_seed: &SkSeed<Self::N>, adrs: &impl Address) -> Array<u8, Self::N> {
        let hasher = self
            .cached_hasher
            .clone()
            .chain(adrs.as_ref())
            .chain(sk_seed.as_ref());
        let mut output = Array::<u8, Self::N>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }

    fn t<L: ArraySize>(
        &self,
        adrs: &impl Address,
        m: &Array<Array<u8, Self::N>, L>,
    ) -> Array<u8, Self::N> {
        let mut hasher = self.cached_hasher.clone().chain(adrs.as_ref());
        m.iter().for_each(|x| hasher.update(x.as_slice()));
        let mut output = Array::<u8, Self::N>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }

    fn h(
        &self,
        adrs: &impl Address,
        m1: &Array<u8, Self::N>,
        m2: &Array<u8, Self::N>,
    ) -> Array<u8, Self::N> {
        let hasher = self
            .cached_hasher
            .clone()
            .chain(adrs.as_ref())
            .chain(m1.as_slice())
            .chain(m2.as_slice());
        let mut output = Array::<u8, Self::N>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }

    fn f(&self, adrs: &impl Address, m: &Array<u8, Self::N>) -> Array<u8, Self::N> {
        let hasher = self
            .cached_hasher
            .clone()
            .chain(adrs.as_ref())
            .chain(m.as_slice());
        let mut output = Array::<u8, Self::N>::default();
        hasher.finalize_xof_into(&mut output);
        output
    }
}

// TODO: Consolidate parameters between Shake and SHA2 instances

/// SHAKE256 at L1 security with small signatures
pub type Shake128s = Shake<U16, U30>;
impl WotsParams for Shake128s {
    type WotsMsgLen = U<32>;
    type WotsSigLen = U<35>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake128s {
    type HPrime = U<9>;
}
impl HypertreeParams for Shake128s {
    type D = U<7>;
    type H = U<63>;
}
impl ForsParams for Shake128s {
    type K = U<14>;
    type A = U<12>;
    type MD = U<{ (12 * 14usize).div_ceil(8) }>;
}
impl ParameterSet for Shake128s {
    const NAME: &'static str = "SLH-DSA-SHAKE-128s";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_128_S;
}

/// SHAKE256 at L1 security with fast signatures
pub type Shake128f = Shake<U16, U34>;
impl WotsParams for Shake128f {
    type WotsMsgLen = U<32>;
    type WotsSigLen = U<35>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake128f {
    type HPrime = U<3>;
}
impl HypertreeParams for Shake128f {
    type D = U<22>;
    type H = U<66>;
}
impl ForsParams for Shake128f {
    type K = U<33>;
    type A = U<6>;
    type MD = U<25>;
}
impl ParameterSet for Shake128f {
    const NAME: &'static str = "SLH-DSA-SHAKE-128f";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_128_F;
}

/// SHAKE256 at L3 security with small signatures
pub type Shake192s = Shake<U24, U39>;
impl WotsParams for Shake192s {
    type WotsMsgLen = U<{ 24 * 2 }>;
    type WotsSigLen = U<{ 24 * 2 + 3 }>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake192s {
    type HPrime = U<9>;
}
impl HypertreeParams for Shake192s {
    type D = U<7>;
    type H = U<63>;
}
impl ForsParams for Shake192s {
    type K = U<17>;
    type A = U<14>;
    type MD = U<{ (14 * 17usize).div_ceil(8) }>;
}
impl ParameterSet for Shake192s {
    const NAME: &'static str = "SLH-DSA-SHAKE-192s";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_192_S;
}

/// SHAKE256 at L3 security with fast signatures
pub type Shake192f = Shake<U24, U42>;
impl WotsParams for Shake192f {
    type WotsMsgLen = U<{ 24 * 2 }>;
    type WotsSigLen = U<{ 24 * 2 + 3 }>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake192f {
    type HPrime = U<3>;
}
impl HypertreeParams for Shake192f {
    type D = U<22>;
    type H = U<66>;
}
impl ForsParams for Shake192f {
    type K = U<33>;
    type A = U<8>;
    type MD = U<{ (33 * 8usize).div_ceil(8) }>;
}
impl ParameterSet for Shake192f {
    const NAME: &'static str = "SLH-DSA-SHAKE-192f";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_192_F;
}

/// SHAKE256 at L5 security with small signatures
pub type Shake256s = Shake<U32, U47>;
impl WotsParams for Shake256s {
    type WotsMsgLen = U<{ 32 * 2 }>;
    type WotsSigLen = U<{ 32 * 2 + 3 }>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake256s {
    type HPrime = U<8>;
}
impl HypertreeParams for Shake256s {
    type D = U<8>;
    type H = U<64>;
}
impl ForsParams for Shake256s {
    type K = U<22>;
    type A = U<14>;
    type MD = U<{ (14 * 22usize).div_ceil(8) }>;
}
impl ParameterSet for Shake256s {
    const NAME: &'static str = "SLH-DSA-SHAKE-256s";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_256_S;
}

/// SHAKE256 at L5 security with fast signatures
pub type Shake256f = Shake<U32, U49>;
impl WotsParams for Shake256f {
    type WotsMsgLen = U<{ 32 * 2 }>;
    type WotsSigLen = U<{ 32 * 2 + 3 }>;
    type LgW = U<4>;
    type CkLen = U<3>;
}
impl XmssParams for Shake256f {
    type HPrime = U<4>;
}
impl HypertreeParams for Shake256f {
    type D = U<17>;
    type H = U<68>;
}
impl ForsParams for Shake256f {
    type K = U<35>;
    type A = U<9>;
    type MD = U<{ (35 * 9usize).div_ceil(8) }>;
}
impl ParameterSet for Shake256f {
    const NAME: &'static str = "SLH-DSA-SHAKE-256f";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier = fips205::ID_SLH_DSA_SHAKE_256_F;
}

// ---------------------------------------------------------------------------
// NIST SP 800-230 (ipd) limited-signature parameter sets (2^24 signatures max).
// d = 1 (single hypertree layer) and adjusted Winternitz parameter:
//   w = 4  (lg_w = 2) for security levels 1 and 5,
//   w = 8  (lg_w = 3) for security level 3.
// These are NOT approved for general use; the 2^24 per-key signature limit is a
// strict security requirement (see SP 800-230 §3).
//
// NOTE: OIDs are PROVISIONAL placeholders — SP 800-230 is a draft and NIST has
// not assigned object identifiers. They MUST be replaced with the official
// values before these are used in any interoperable PKCS#8 / SPKI context.
// ---------------------------------------------------------------------------

/// SHAKE256 at L1 security, SP 800-230 limited-signature (2^24) variant
pub type Shake128_24 = Shake<U16, U21>;
impl WotsParams for Shake128_24 {
    type WotsMsgLen = U<64>;
    type WotsSigLen = U<68>;
    type LgW = U<2>;
    type CkLen = U<4>;
}
impl XmssParams for Shake128_24 {
    type HPrime = U<22>;
}
impl HypertreeParams for Shake128_24 {
    type D = U<1>;
    type H = U<22>;
}
impl ForsParams for Shake128_24 {
    type K = U<6>;
    type A = U<24>;
    type MD = U<{ (6 * 24usize).div_ceil(8) }>;
}
impl ParameterSet for Shake128_24 {
    const NAME: &'static str = "SLH-DSA-SHAKE-128-24";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier =
        pkcs8::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.0.800.230.2.1");
}

/// SHAKE256 at L3 security, SP 800-230 limited-signature (2^24) variant
#[cfg(feature = "sp800-230-highsec")]
pub type Shake192_24 = Shake<U24, U32>;
#[cfg(feature = "sp800-230-highsec")]
impl WotsParams for Shake192_24 {
    type WotsMsgLen = U<64>;
    type WotsSigLen = U<67>;
    type LgW = U<3>;
    type CkLen = U<3>;
}
#[cfg(feature = "sp800-230-highsec")]
impl XmssParams for Shake192_24 {
    type HPrime = U<21>;
}
#[cfg(feature = "sp800-230-highsec")]
impl HypertreeParams for Shake192_24 {
    type D = U<1>;
    type H = U<21>;
}
#[cfg(feature = "sp800-230-highsec")]
impl ForsParams for Shake192_24 {
    type K = U<9>;
    type A = U<25>;
    type MD = U<{ (9 * 25usize).div_ceil(8) }>;
}
#[cfg(feature = "sp800-230-highsec")]
impl ParameterSet for Shake192_24 {
    const NAME: &'static str = "SLH-DSA-SHAKE-192-24";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier =
        pkcs8::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.0.800.230.2.2");
}

/// SHAKE256 at L5 security, SP 800-230 limited-signature (2^24) variant
#[cfg(feature = "sp800-230-highsec")]
pub type Shake256_24 = Shake<U32, U41>;
#[cfg(feature = "sp800-230-highsec")]
impl WotsParams for Shake256_24 {
    type WotsMsgLen = U<128>;
    type WotsSigLen = U<133>;
    type LgW = U<2>;
    type CkLen = U<5>;
}
#[cfg(feature = "sp800-230-highsec")]
impl XmssParams for Shake256_24 {
    type HPrime = U<21>;
}
#[cfg(feature = "sp800-230-highsec")]
impl HypertreeParams for Shake256_24 {
    type D = U<1>;
    type H = U<21>;
}
#[cfg(feature = "sp800-230-highsec")]
impl ForsParams for Shake256_24 {
    type K = U<12>;
    type A = U<25>;
    type MD = U<{ (12 * 25usize).div_ceil(8) }>;
}
#[cfg(feature = "sp800-230-highsec")]
impl ParameterSet for Shake256_24 {
    const NAME: &'static str = "SLH-DSA-SHAKE-256-24";
    const ALGORITHM_OID: pkcs8::ObjectIdentifier =
        pkcs8::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.0.800.230.2.3");
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    fn prf_msg<H: HashSuite>() {
        let sk_prf = SkPrf(Array::<u8, H::N>::from_fn(|_| 0));
        let opt_rand = Array::<u8, H::N>::from_fn(|_| 1);
        let msg = [2u8; 32];

        let expected = hex!("bc5c062307df0a41aeeae19ad655f7b2");

        let result = H::prf_msg(&sk_prf, &opt_rand, &[&[msg]]);

        assert_eq!(result.as_slice(), expected);
    }

    #[test]
    fn prf_msg_16_30() {
        prf_msg::<Shake128f>();
    }
}