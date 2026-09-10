#![cfg_attr(rustfmt, rustfmt_skip)]
use crate::ParameterSet;
use crate::Sha2L1;
use crate::Sha2L35;
use crate::Shake;
use crate::address::ForsTree;
use crate::signature_encoding::Signature;
use crate::util::split_digest;
use ::signature::{Error, MultipartVerifier, Verifier};
use hybrid_array::{Array, ArraySize};
use pkcs8::{der, spki};
use rand_core::CryptoRng;
use typenum::{U, U16, U24, U32, Unsigned};

#[cfg(feature = "alloc")]
use pkcs8::EncodePublicKey;

/// A trait specifying the length of a serialized verifying key for a given parameter set
pub trait VerifyingKeyLen {
    /// The length of the serialized verifying key in bytes
    type VkLen: ArraySize;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PkSeed<N: ArraySize>(pub(crate) Array<u8, N>);
impl<N: ArraySize> AsRef<[u8]> for PkSeed<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl<N: ArraySize> From<&[u8]> for PkSeed<N> {
    fn from(slice: &[u8]) -> Self {
        #[allow(deprecated)]
        Self(Array::clone_from_slice(slice))
    }
}
impl<N: ArraySize> PkSeed<N> {
    pub(crate) fn new<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let mut bytes = Array::<u8, N>::default();
        rng.fill_bytes(bytes.as_mut_slice());
        Self(bytes)
    }
}

/// A `VerifyingKey` is an SLH-DSA public key, allowing
/// verification of signatures created with the corresponding
/// `SigningKey`
#[derive(Debug)]
pub struct VerifyingKey<P: ParameterSet> {
    pub(crate) pk_seed: PkSeed<P::N>,
    pub(crate) pk_root: Array<u8, P::N>,
}

impl<P: ParameterSet> PartialEq for VerifyingKey<P> {
    fn eq(&self, other: &Self) -> bool {
        self.pk_seed == other.pk_seed && self.pk_root == other.pk_root
    }
}

impl<P: ParameterSet> Eq for VerifyingKey<P> {}

impl<P: ParameterSet + VerifyingKeyLen> VerifyingKey<P> {
    #[doc(hidden)]
    /// Verify a raw message (without context).
    /// Implements [slh_verify_internal] as defined in FIPS-205.
    /// Published for KAT validation purposes but not intended for general use.
    pub fn slh_verify_internal(
        &self,
        msg: &[&[u8]],
        signature: &Signature<P>,
    ) -> Result<(), Error> {
        self.raw_slh_verify_internal(&[msg], signature)
    }

    fn raw_slh_verify_internal(
        &self,
        msg: &[&[&[u8]]],
        signature: &Signature<P>,
    ) -> Result<(), Error> {
        let pk_seed = &self.pk_seed;
        let ht = P::new_from_pk_seed(pk_seed);
        let randomizer = &signature.randomizer;
        let fors_sig = &signature.fors_sig;
        let ht_sig = &signature.ht_sig;

        let digest = P::h_msg(randomizer, pk_seed, &self.pk_root, msg);
        let (md, idx_tree, idx_leaf) = split_digest::<P>(&digest);

        let adrs = ForsTree::new(idx_tree, idx_leaf);
        let fors_pk = ht.fors_pk_from_sig(fors_sig, md, &adrs);
        ht.ht_verify(&fors_pk, ht_sig, idx_tree, idx_leaf, &self.pk_root)
            .then_some(())
            .ok_or(Error::new())
    }

    /// Implements [slh-verify] as defined in FIPS-205, using a context string.
    /// Context strings must be 255 bytes or less.
    /// # Errors
    /// Returns an error if the context is too long or if the signature is invalid
    pub fn try_verify_with_context(
        &self,
        msg: &[u8],
        ctx: &[u8],
        signature: &Signature<P>,
    ) -> Result<(), Error> {
        self.raw_try_verify_with_context(&[msg], ctx, signature)
    }

    fn raw_try_verify_with_context(
        &self,
        msg: &[&[u8]],
        ctx: &[u8],
        signature: &Signature<P>,
    ) -> Result<(), Error> {
        let ctx_len = u8::try_from(ctx.len()).map_err(|_| Error::new())?;
        let ctx_len_bytes = ctx_len.to_be_bytes();

        let ctx_msg = [&[&[0], &ctx_len_bytes, ctx], msg];
        self.raw_slh_verify_internal(&ctx_msg, signature) // TODO - context processing
    }

    /// Serialize the verifying key to a new stack-allocated array
    ///
    /// This clones the underlying fields
    pub fn to_bytes(&self) -> Array<u8, P::VkLen> {
        let mut bytes = Array::<u8, P::VkLen>::default();
        debug_assert!(P::N::USIZE * 2 == P::VkLen::USIZE);
        bytes[..P::N::USIZE].copy_from_slice(&self.pk_seed.0);
        bytes[P::N::USIZE..].copy_from_slice(&self.pk_root);
        bytes
    }

    /// Serialize the verifying key to a new heap-allocated vector
    #[cfg(feature = "alloc")]
    pub fn to_vec(&self) -> alloc::vec::Vec<u8> {
        self.to_bytes().to_vec()
    }
}

/// Output of the fault-hardened verification API (`hardened-verify` feature).
///
/// Contains the candidate hypertree root computed from the signature — with
/// its bytes masked under a caller-supplied random `mask` — together with the
/// expected root from the verifying key. **This type deliberately makes no
/// accept/reject decision**: the caller must unmask and compare byte-by-byte
/// (see [`VerifyingKey::slh_verify_hardened`] for the recommended
/// comparison loop), so that a single instruction glitch cannot skip the
/// entire check the way it could a returned `Result`.
///
/// Masking rule: byte `i` of the candidate root is XORed with `0xFF` iff bit
/// `i` of `mask` is set (`n <= 32` for all SLH-DSA parameter sets, so `u32`
/// covers every byte). Because `mask` is a runtime random value, the compiler
/// cannot fold the caller's per-byte comparisons back into a single `memcmp`.
#[cfg(feature = "hardened-verify")]
pub struct HardenedVerifyOutput<P: ParameterSet> {
    masked_root: Array<u8, P::N>,
    expected_root: Array<u8, P::N>,
    mask: u32,
}

#[cfg(feature = "hardened-verify")]
impl<P: ParameterSet> HardenedVerifyOutput<P> {
    /// The candidate root implied by the signature, masked under `mask`.
    pub fn masked_root(&self) -> &[u8] {
        self.masked_root.as_slice()
    }

    /// The expected root (`pk_root` from the verifying key), unmasked.
    pub fn expected_root(&self) -> &[u8] {
        self.expected_root.as_slice()
    }

    /// The mask this output was created with (echoed back for convenience;
    /// callers should prefer their own copy of the value they passed in).
    pub fn mask(&self) -> u32 {
        self.mask
    }
}

/// Fault-hardened verification (`hardened-verify` feature).
///
/// Performs all the signature computation but **no final comparison**; returns
/// masked byte material for the caller to compare externally, one byte at a
/// time, so that defeating the check requires glitching every byte comparison
/// rather than a single branch or return value.
#[cfg(feature = "hardened-verify")]
impl<P: ParameterSet> VerifyingKey<P> {
    /// Compute the masked candidate root for `signature` over `msg`.
    ///
    /// Mirrors the internal path
    /// ([`slh_verify_internal`](Self::slh_verify_internal): raw message, no
    /// context prefix, matching the KAT vectors), so signatures must be
    /// produced by `slh_sign_internal` (as the `slh-sign` tool does), NOT by
    /// `Signer::try_sign`, which prepends the FIPS-205 context bytes
    /// (`0x00 0x00` even for an empty context).
    ///
    /// In the intended deployment `msg` is a short **pre-hash** of the actual
    /// payload (e.g. `SHA-256(image)`), computed by the caller — possibly on
    /// a hardware hasher, streamed from external storage — so this crate only
    /// ever hashes a few dozen bytes. Trade-off note: SLH-DSA proper is
    /// collision-*resilient* (its randomized message digest avoids relying on
    /// plain collision resistance of the hash), whereas signing a pre-hash
    /// makes forgery reducible to offline collisions in the pre-hash
    /// function. FIPS-205 sanctions this shape as HashSLH-DSA and rates
    /// SHA-256 sufficient for security category 1.
    ///
    /// This function cannot fail: a wrong signature, wrong key, or wrong
    /// message simply yields a candidate root that will not match, which the
    /// caller's comparison detects. There is deliberately no internal
    /// accept/reject decision to glitch.
    ///
    /// # Recommended caller-side comparison
    ///
    /// ```ignore
    /// let out = vk.slh_verify_hardened(&[digest], &sig, mask);
    /// let (mr, er) = (out.masked_root(), out.expected_root());
    /// if mr.len() != er.len() { fault_abort(); }
    /// let mut matched: usize = 0;
    /// for i in 0..mr.len() {
    ///     let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8); // 0x00 or 0xFF
    ///     let cand = core::hint::black_box(mr[i]) ^ unmask;
    ///     if cand != er[i] { fault_abort(); }   // independent per-byte branch
    ///     matched += 1;
    /// }
    /// if matched != mr.len() { fault_abort(); } // loop-completion cross-check
    /// // signature valid
    /// ```
    pub fn slh_verify_hardened(
        &self,
        msg: &[&[u8]],
        signature: &Signature<P>,
        mask: u32,
    ) -> HardenedVerifyOutput<P> {
        let digest = P::h_msg(&signature.randomizer, &self.pk_seed, &self.pk_root, &[msg]);
        let (md, idx_tree, idx_leaf) = split_digest::<P>(&digest);

        let ht = P::new_from_pk_seed(&self.pk_seed);
        let adrs = ForsTree::new(idx_tree, idx_leaf);
        let fors_pk = ht.fors_pk_from_sig(&signature.fors_sig, md, &adrs);
        let root = ht.ht_root(&fors_pk, &signature.ht_sig, idx_tree, idx_leaf);

        let masked_root = Array::from_fn(|i| {
            // XOR byte i with 0xFF iff mask bit i is set. n <= 32 always.
            root[i] ^ 0u8.wrapping_sub(((mask >> (i % 32)) & 1) as u8)
        });

        HardenedVerifyOutput {
            masked_root,
            expected_root: self.pk_root.clone(),
            mask,
        }
    }
}

impl<P: ParameterSet> core::hash::Hash for VerifyingKey<P> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state);
    }
}

impl<P: ParameterSet> Clone for VerifyingKey<P> {
    fn clone(&self) -> Self {
        VerifyingKey {
            pk_seed: self.pk_seed.clone(),
            pk_root: self.pk_root.clone(),
        }
    }
}

impl<P: ParameterSet> From<&VerifyingKey<P>> for Array<u8, P::VkLen> {
    fn from(vk: &VerifyingKey<P>) -> Array<u8, P::VkLen> {
        vk.to_bytes()
    }
}

impl<P: ParameterSet> From<Array<u8, P::VkLen>> for VerifyingKey<P> {
    #[allow(deprecated)] // clone_from_slice
    fn from(bytes: Array<u8, P::VkLen>) -> VerifyingKey<P> {
        debug_assert!(P::VkLen::USIZE == 2 * P::N::USIZE);
        let pk_seed = PkSeed(Array::clone_from_slice(&bytes[..P::N::USIZE]));
        let pk_root = Array::clone_from_slice(&bytes[P::N::USIZE..]);
        VerifyingKey { pk_seed, pk_root }
    }
}

impl<P: ParameterSet> TryFrom<&[u8]> for VerifyingKey<P> {
    type Error = Error;

    #[allow(deprecated)] // clone_from_slice
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != P::N::USIZE * 2 {
            return Err(Error::new());
        }
        let pk_seed = PkSeed(Array::clone_from_slice(&bytes[..P::N::USIZE]));
        let pk_root = Array::clone_from_slice(&bytes[P::N::USIZE..]);
        Ok(VerifyingKey { pk_seed, pk_root })
    }
}

impl<P: ParameterSet> Verifier<Signature<P>> for VerifyingKey<P> {
    fn verify(&self, msg: &[u8], signature: &Signature<P>) -> Result<(), Error> {
        self.multipart_verify(&[msg], signature)
    }
}

impl<P: ParameterSet> MultipartVerifier<Signature<P>> for VerifyingKey<P> {
    fn multipart_verify(&self, msg: &[&[u8]], signature: &Signature<P>) -> Result<(), Error> {
        self.raw_try_verify_with_context(msg, &[], signature) // TODO - context processing
    }
}

#[cfg(feature = "alloc")]
impl<P: ParameterSet> EncodePublicKey for VerifyingKey<P> {
    fn to_public_key_der(&self) -> pkcs8::spki::Result<der::Document> {
        let algorithm_identifier = pkcs8::AlgorithmIdentifierRef {
            oid: P::ALGORITHM_OID,
            parameters: None,
        };

        let public_key = self.to_bytes();
        let subject_public_key = der::asn1::BitStringRef::new(0, &public_key)?;

        pkcs8::SubjectPublicKeyInfo {
            algorithm: algorithm_identifier,
            subject_public_key,
        }
        .try_into()
    }
}

impl<P: ParameterSet> TryFrom<pkcs8::SubjectPublicKeyInfoRef<'_>> for VerifyingKey<P> {
    type Error = spki::Error;

    fn try_from(spki: pkcs8::SubjectPublicKeyInfoRef<'_>) -> spki::Result<Self> {
        spki.algorithm.assert_algorithm_oid(P::ALGORITHM_OID)?;

        Self::try_from(
            spki.subject_public_key
                .as_bytes()
                .ok_or_else(|| der::Tag::BitString.value_error().to_error())?,
        )
        .map_err(|_| spki::Error::KeyMalformed)
    }
}

impl<M> VerifyingKeyLen for Sha2L1<U16, M> {
    type VkLen = U<32>;
}

impl<M> VerifyingKeyLen for Sha2L35<U24, M> {
    type VkLen = U<48>;
}
impl<M> VerifyingKeyLen for Sha2L35<U32, M> {
    type VkLen = U<64>;
}

impl<M> VerifyingKeyLen for Shake<U16, M> {
    type VkLen = U<32>;
}
impl<M> VerifyingKeyLen for Shake<U24, M> {
    type VkLen = U<48>;
}
impl<M> VerifyingKeyLen for Shake<U32, M> {
    type VkLen = U<64>;
}

#[cfg(test)]
mod tests {
    use crate::*;
    use hybrid_array::Array;
    use signature::*;

    #[test]
    fn test_vk_serialize_deserialize() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let vk = sk.verifying_key();
        let vk_bytes: Array<u8, _> = (&vk).into();
        let vk2 = VerifyingKey::<Shake128f>::try_from(vk_bytes.as_slice()).unwrap();
        assert_eq!(vk, vk2);
    }
}

#[cfg(all(test, feature = "hardened-verify"))]
mod hardened_tests {
    use super::*;
    use crate::Shake128f;
    use crate::signing_key::SigningKey;
    use ::signature::Keypair;

    /// The recommended caller-side loop from the API docs, as a predicate.
    fn unmask_compare(out: &HardenedVerifyOutput<Shake128f>, mask: u32) -> bool {
        let (mr, er) = (out.masked_root(), out.expected_root());
        if mr.len() != er.len() {
            return false;
        }
        let mut matched = 0usize;
        for i in 0..mr.len() {
            let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8);
            if (mr[i] ^ unmask) != er[i] {
                return false;
            }
            matched += 1;
        }
        matched == mr.len()
    }

    #[test]
    fn hardened_accepts_and_rejects() {
        let sk = SigningKey::<Shake128f>::slh_keygen_internal(&[9u8; 16], &[8u8; 16], &[7u8; 16]);
        let vk = sk.verifying_key();

        // Pre-hash flow: the signed "message" is a short digest of the real
        // payload, computed by the caller (stand-in bytes here).
        let payload_digest = [0x42u8; 32];
        let msg: &[u8] = &payload_digest;
        let sig = sk.slh_sign_internal(&[msg], None);

        for mask in [0u32, 0xFFFF_FFFF, 0xA5A5_5A5A] {
            let out = vk.slh_verify_hardened(&[msg], &sig, mask);
            assert!(unmask_compare(&out, mask), "valid signature must unmask-compare equal");
        }

        let mut tampered = payload_digest;
        tampered[0] ^= 1;
        let out = vk.slh_verify_hardened(&[&tampered[..]], &sig, 0xDEAD_BEEF);
        assert!(!unmask_compare(&out, 0xDEAD_BEEF), "tampered digest must mismatch");

        // Cross-check against the crate's own internal verify.
        assert!(vk.slh_verify_internal(&[msg], &sig).is_ok());
    }
}