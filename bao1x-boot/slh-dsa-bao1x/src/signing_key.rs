use crate::address::{ForsTree, WotsHash};
use crate::signature_encoding::Signature;
use crate::util::split_digest;
use crate::verifying_key::VerifyingKey;
use crate::{ParameterSet, PkSeed, Sha2L1, Sha2L35, Shake, VerifyingKeyLen};
use ::signature::{
    Error, KeypairRef, MultipartSigner, RandomizedMultipartSigner, RandomizedSigner, Signer,
    rand_core::{CryptoRng, TryCryptoRng},
};
use hybrid_array::{Array, ArraySize};
use pkcs8::{
    der::AnyRef,
    spki::{AlgorithmIdentifier, AssociatedAlgorithmIdentifier, SignatureAlgorithmIdentifier},
};
use typenum::{U, U16, U24, U32, Unsigned};

#[cfg(feature = "zeroize")]
use zeroize::{Zeroize, ZeroizeOnDrop};

#[cfg(feature = "alloc")]
use pkcs8::{
    EncodePrivateKey,
    der::{self, asn1::OctetStringRef},
};

// NewTypes for ensuring hash argument order correctness
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SkSeed<N: ArraySize>(pub(crate) Array<u8, N>);
impl<N: ArraySize> AsRef<[u8]> for SkSeed<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<N: ArraySize> From<&[u8]> for SkSeed<N> {
    fn from(slice: &[u8]) -> Self {
        #[allow(deprecated)]
        Self(Array::clone_from_slice(slice))
    }
}
impl<N: ArraySize> SkSeed<N> {
    pub(crate) fn new<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let mut bytes = Array::<u8, N>::default();
        rng.fill_bytes(bytes.as_mut_slice());
        Self(bytes)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SkPrf<N: ArraySize>(pub(crate) Array<u8, N>);
impl<N: ArraySize> AsRef<[u8]> for SkPrf<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<N: ArraySize> From<&[u8]> for SkPrf<N> {
    fn from(slice: &[u8]) -> Self {
        #[allow(deprecated)]
        Self(Array::clone_from_slice(slice))
    }
}
impl<N: ArraySize> SkPrf<N> {
    pub(crate) fn new<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let mut bytes = Array::<u8, N>::default();
        rng.fill_bytes(bytes.as_mut_slice());
        Self(bytes)
    }
}

/// A `SigningKey` allows signing messages with a fixed parameter set
#[derive(Clone, Debug)]
pub struct SigningKey<P: ParameterSet> {
    pub(crate) sk_seed: SkSeed<P::N>,
    pub(crate) sk_prf: SkPrf<P::N>,
    pub(crate) verifying_key: VerifyingKey<P>,
}

impl<P: ParameterSet> PartialEq for SigningKey<P> {
    fn eq(&self, other: &Self) -> bool {
        self.sk_seed == other.sk_seed
            && self.sk_prf == other.sk_prf
            && self.verifying_key == other.verifying_key
    }
}

impl<P: ParameterSet> Eq for SigningKey<P> {}

#[cfg(feature = "zeroize")]
impl<P: ParameterSet> Drop for SigningKey<P> {
    fn drop(&mut self) {
        self.sk_seed.0.zeroize();
        self.sk_prf.0.zeroize();
    }
}

#[cfg(feature = "zeroize")]
impl<P: ParameterSet> ZeroizeOnDrop for SigningKey<P> {}

/// A trait specifying the length of a serialized signing key for a given parameter set
pub trait SigningKeyLen: VerifyingKeyLen {
    /// The length of the serialized signing key in bytes
    type SkLen: ArraySize;
}

impl<P: ParameterSet> SigningKey<P> {
    /// Create a new `SigningKey` from a cryptographic random number generator
    pub fn new<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let sk_seed = SkSeed::new(rng);
        let sk_prf = SkPrf::new(rng);
        let pk_seed = PkSeed::new(rng);
        Self::from_seed(sk_seed, sk_prf, pk_seed)
    }

    fn from_seed(sk_seed: SkSeed<P::N>, sk_prf: SkPrf<P::N>, pk_seed: PkSeed<P::N>) -> Self {
        let mut adrs = WotsHash::default();
        adrs.layer_adrs.set(P::D::U32 - 1);
        let xmss = P::new_from_pk_seed(&pk_seed);

        let pk_root = xmss.xmss_node(&sk_seed, 0, P::HPrime::U32, &adrs);
        let verifying_key = VerifyingKey { pk_seed, pk_root };
        SigningKey {
            sk_seed,
            sk_prf,
            verifying_key,
        }
    }

    #[doc(hidden)]
    #[allow(clippy::must_use_candidate)]
    /// Construct a new SigningKey from pre-chosen seeds.
    /// Implements [slh_keygen_internal] as defined in FIPS-205.
    /// Published for KAT validation purposes but not intended for general use.
    pub fn slh_keygen_internal(sk_seed: &[u8], sk_prf: &[u8], pk_seed: &[u8]) -> Self {
        let sk_seed = SkSeed::from(sk_seed);
        let sk_prf = SkPrf::from(sk_prf);
        let pk_seed = PkSeed::from(pk_seed);
        Self::from_seed(sk_seed, sk_prf, pk_seed)
    }

    #[doc(hidden)]
    /// Sign a message with a pre-chosen randomizer.
    /// Implements [slh_sign_internal] as defined in FIPS-205.
    /// Published for KAT validation purposes but not intended for general use.
    /// opt_rand must be a P::N length slice, panics otherwise.
    pub fn slh_sign_internal(&self, msg: &[&[u8]], opt_rand: Option<&[u8]>) -> Signature<P> {
        self.raw_slh_sign_internal(&[msg], opt_rand)
    }

    fn raw_slh_sign_internal(&self, msg: &[&[&[u8]]], opt_rand: Option<&[u8]>) -> Signature<P> {
        let rand = opt_rand
            .unwrap_or(&self.verifying_key.pk_seed.0)
            .try_into()
            .unwrap();

        let sk_seed = &self.sk_seed;
        let pk_seed = &self.verifying_key.pk_seed;
        let ht = P::new_from_pk_seed(pk_seed);

        let randomizer = P::prf_msg(&self.sk_prf, rand, msg);

        let digest = P::h_msg(&randomizer, pk_seed, &self.verifying_key.pk_root, msg);
        let (md, idx_tree, idx_leaf) = split_digest::<P>(&digest);
        let adrs = ForsTree::new(idx_tree, idx_leaf);
        let fors_sig = ht.fors_sign(md, sk_seed, &adrs);

        let fors_pk = ht.fors_pk_from_sig(&fors_sig, md, &adrs);
        let ht_sig = ht.ht_sign(&fors_pk, sk_seed, idx_tree, idx_leaf);

        Signature {
            randomizer,
            fors_sig,
            ht_sig,
        }
    }

    /// Implements [slh-sign] as defined in FIPS-205, using a context string.
    /// Context strings must be 255 bytes or less.
    /// # Errors
    /// Returns an error if the context string is too long.
    pub fn try_sign_with_context(
        &self,
        msg: &[u8],
        ctx: &[u8],
        opt_rand: Option<&[u8]>,
    ) -> Result<Signature<P>, Error> {
        self.raw_try_sign_with_context(&[msg], ctx, opt_rand)
    }

    fn raw_try_sign_with_context(
        &self,
        msg: &[&[u8]],
        ctx: &[u8],
        opt_rand: Option<&[u8]>,
    ) -> Result<Signature<P>, Error> {
        let ctx_len = u8::try_from(ctx.len()).map_err(|_| Error::new())?;
        let ctx_len_bytes = ctx_len.to_be_bytes();

        let ctx_msg = [&[&[0], &ctx_len_bytes, ctx], msg];
        Ok(self.raw_slh_sign_internal(&ctx_msg, opt_rand))
    }

    /// Serialize the signing key to a new stack-allocated array
    ///
    /// This clones the underlying fields
    pub fn to_bytes(&self) -> Array<u8, P::SkLen> {
        let mut bytes = Array::<u8, P::SkLen>::default();
        bytes[..P::N::USIZE].copy_from_slice(&self.sk_seed.0);
        bytes[P::N::USIZE..2 * P::N::USIZE].copy_from_slice(&self.sk_prf.0);
        bytes[2 * P::N::USIZE..].copy_from_slice(&self.verifying_key.to_bytes());
        bytes
    }

    /// Serialize the signing key to a new heap-allocated vector
    #[cfg(feature = "alloc")]
    pub fn to_vec(&self) -> alloc::vec::Vec<u8>
    where
        P: VerifyingKeyLen,
    {
        self.to_bytes().to_vec()
    }
}

impl<P: ParameterSet> TryFrom<&[u8]> for SigningKey<P> {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != P::SkLen::USIZE {
            return Err(Error::new());
        }

        let (sk_seed_bytes, rest) = bytes.split_at(P::N::USIZE);
        let (sk_prf_bytes, verifying_key_bytes) = rest.split_at(P::N::USIZE);
        let verifying_key = VerifyingKey::try_from(verifying_key_bytes)?;

        Ok(SigningKey {
            sk_seed: SkSeed::from(sk_seed_bytes),
            sk_prf: SkPrf::from(sk_prf_bytes),
            verifying_key,
        })
    }
}

impl<P: ParameterSet> Signer<Signature<P>> for SigningKey<P> {
    fn try_sign(&self, msg: &[u8]) -> Result<Signature<P>, Error> {
        self.try_multipart_sign(&[msg])
    }
}

impl<P: ParameterSet> MultipartSigner<Signature<P>> for SigningKey<P> {
    fn try_multipart_sign(&self, msg: &[&[u8]]) -> Result<Signature<P>, Error> {
        self.raw_try_sign_with_context(msg, &[], None)
    }
}

impl<P: ParameterSet> RandomizedSigner<Signature<P>> for SigningKey<P> {
    fn try_sign_with_rng<R: TryCryptoRng + ?Sized>(
        &self,
        rng: &mut R,
        msg: &[u8],
    ) -> Result<Signature<P>, signature::Error> {
        self.try_multipart_sign_with_rng(rng, &[msg])
    }
}

impl<P: ParameterSet> RandomizedMultipartSigner<Signature<P>> for SigningKey<P> {
    fn try_multipart_sign_with_rng<R: TryCryptoRng + ?Sized>(
        &self,
        rng: &mut R,
        msg: &[&[u8]],
    ) -> Result<Signature<P>, Error> {
        let mut randomizer = Array::<u8, P::N>::default();
        rng.try_fill_bytes(randomizer.as_mut_slice())
            .map_err(|_| signature::Error::new())?;
        self.raw_try_sign_with_context(msg, &[], Some(&randomizer))
    }
}

impl<P: ParameterSet> AsRef<VerifyingKey<P>> for SigningKey<P> {
    fn as_ref(&self) -> &VerifyingKey<P> {
        &self.verifying_key
    }
}

impl<P: ParameterSet> KeypairRef for SigningKey<P> {
    type VerifyingKey = VerifyingKey<P>;
}

impl<P> TryFrom<pkcs8::PrivateKeyInfoRef<'_>> for SigningKey<P>
where
    P: ParameterSet,
{
    type Error = pkcs8::Error;

    fn try_from(private_key_info: pkcs8::PrivateKeyInfoRef<'_>) -> pkcs8::Result<Self> {
        private_key_info
            .algorithm
            .assert_algorithm_oid(P::ALGORITHM_OID)?;

        Self::try_from(private_key_info.private_key.as_bytes())
            .map_err(|_| pkcs8::KeyError::Invalid.into())
    }
}

#[cfg(feature = "alloc")]
impl<P> EncodePrivateKey for SigningKey<P>
where
    P: ParameterSet,
{
    fn to_pkcs8_der(&self) -> pkcs8::Result<der::SecretDocument> {
        let algorithm_identifier = pkcs8::AlgorithmIdentifierRef {
            oid: P::ALGORITHM_OID,
            parameters: None,
        };

        let private_key = self.to_bytes();
        let pkcs8_key =
            pkcs8::PrivateKeyInfoRef::new(algorithm_identifier, OctetStringRef::new(&private_key)?);
        Ok(der::SecretDocument::encode_msg(&pkcs8_key)?)
    }
}

impl<P: ParameterSet> SignatureAlgorithmIdentifier for SigningKey<P> {
    type Params = AnyRef<'static>;

    const SIGNATURE_ALGORITHM_IDENTIFIER: AlgorithmIdentifier<Self::Params> =
        Signature::<P>::ALGORITHM_IDENTIFIER;
}

impl<M> SigningKeyLen for Sha2L1<U16, M> {
    type SkLen = U<{ 4 * 16 }>;
}

impl<M> SigningKeyLen for Sha2L35<U24, M> {
    type SkLen = U<{ 4 * 24 }>;
}
impl<M> SigningKeyLen for Sha2L35<U32, M> {
    type SkLen = U<{ 4 * 32 }>;
}

impl<M> SigningKeyLen for Shake<U16, M> {
    type SkLen = U<{ 4 * 16 }>;
}
impl<M> SigningKeyLen for Shake<U24, M> {
    type SkLen = U<{ 4 * 24 }>;
}
impl<M> SigningKeyLen for Shake<U32, M> {
    type SkLen = U<{ 4 * 32 }>;
}

#[cfg(test)]
mod tests {
    use crate::{ParameterSet, SigningKey, util::macros::test_parameter_sets};

    fn test_serialize_deserialize<P: ParameterSet>() {
        let mut rng = rand::rng();
        let sk = SigningKey::<P>::new(&mut rng);
        let bytes = sk.to_bytes();
        let sk2 = SigningKey::<P>::try_from(bytes.as_slice()).unwrap();
        assert_eq!(sk, sk2);
    }
    test_parameter_sets!(test_serialize_deserialize);

    #[cfg(feature = "alloc")]
    fn test_serialize_deserialize_vec<P: ParameterSet>() {
        let mut rng = rand::rng();
        let sk = SigningKey::<P>::new(&mut rng);
        let vec = sk.to_vec();
        let sk2 = SigningKey::<P>::try_from(vec.as_slice()).unwrap();
        assert_eq!(sk, sk2);
    }
    #[cfg(feature = "alloc")]
    test_parameter_sets!(test_serialize_deserialize_vec);

    #[test]
    fn test_deserialize_fail_on_incorrect_length() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let bytes = sk.to_bytes();
        let incorrect_bytes = &bytes[..bytes.len() - 1];
        assert!(SigningKey::<Shake128f>::try_from(incorrect_bytes).is_err());
    }
}
