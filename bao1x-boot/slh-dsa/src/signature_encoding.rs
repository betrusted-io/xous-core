use crate::ParameterSet;
use crate::{
    Shake128s,
    fors::ForsSignature,
    hashes::{
        Sha2_128f, Sha2_128s, Sha2_192f, Sha2_192s, Sha2_256f, Sha2_256s, Shake128f, Shake192f,
        Shake192s, Shake256f, Shake256s,
    },
    hypertree::HypertreeSig,
};
use ::signature::{Error, SignatureEncoding};
use hybrid_array::{
    Array, ArraySize,
    sizes::{U7856, U16224, U17088, U29792, U35664, U49856},
};
use pkcs8::{AlgorithmIdentifierRef, der::AnyRef, spki::AssociatedAlgorithmIdentifier};
use typenum::Unsigned;

#[cfg(feature = "alloc")]
use pkcs8::{
    der::{self, asn1::BitString},
    spki::SignatureBitStringEncoding,
};

#[derive(Debug, Clone)]
/// A parsed SLH-DSA signature for a given parameter set
///
/// Note that this is a large stack-allocated value and may overflow the stack on
/// small devices. The stack representation consumes `P::SigLen` bytes
///
/// There are no invariants maintained by this struct - every field is a hash value
pub struct Signature<P: ParameterSet> {
    pub(crate) randomizer: Array<u8, P::N>,
    pub(crate) fors_sig: ForsSignature<P>,
    pub(crate) ht_sig: HypertreeSig<P>,
}

impl<P: ParameterSet> PartialEq for Signature<P> {
    fn eq(&self, other: &Self) -> bool {
        self.randomizer == other.randomizer
            && self.fors_sig == other.fors_sig
            && self.ht_sig == other.ht_sig
    }
}

impl<P: ParameterSet> Eq for Signature<P> {}

impl<P: ParameterSet> Signature<P> {
    #[cfg(feature = "alloc")]
    /// Serialize the signature to a `Vec<u8>` of length `P::SigLen`.
    pub fn to_vec(&self) -> alloc::vec::Vec<u8> {
        let mut bytes = alloc::vec::Vec::with_capacity(P::SigLen::USIZE);
        bytes.extend_from_slice(&self.randomizer);
        bytes.extend_from_slice(&self.fors_sig.to_vec());
        bytes.extend_from_slice(&self.ht_sig.to_vec());
        debug_assert!(bytes.len() == P::SigLen::USIZE);
        bytes
    }

    /// Serialize the signature to a new stack-allocated array
    /// This clones the underlying fields
    pub fn to_bytes(&self) -> Array<u8, P::SigLen> {
        let mut bytes = Array::<u8, P::SigLen>::default();
        let r_size = P::N::USIZE;
        let fors_size = ForsSignature::<P>::SIZE;
        bytes[..r_size].copy_from_slice(&self.randomizer);
        self.fors_sig
            .write_to(&mut bytes[r_size..r_size + fors_size]);
        self.ht_sig.write_to(&mut bytes[r_size + fors_size..]);
        bytes
    }
}

impl<P: ParameterSet> core::hash::Hash for Signature<P> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state);
    }
}

impl<P: ParameterSet> TryFrom<&[u8]> for Signature<P> {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != P::SigLen::USIZE {
            return Err(Error::new()); // TODO: Real error
        }

        let (rand_bytes, rest) = bytes.split_at(P::N::USIZE);
        #[allow(deprecated)]
        let randomizer = Array::clone_from_slice(rand_bytes);

        let (fors_bytes, ht_bytes) = rest.split_at(ForsSignature::<P>::SIZE);
        let fors_sig = ForsSignature::try_from(fors_bytes).map_err(|()| Error::new())?;
        let ht_sig = HypertreeSig::try_from(ht_bytes).map_err(|()| Error::new())?;

        Ok(Signature {
            randomizer,
            fors_sig,
            ht_sig,
        })
    }
}

#[cfg(feature = "alloc")]
impl<P: ParameterSet> From<&Signature<P>> for alloc::vec::Vec<u8> {
    fn from(sig: &Signature<P>) -> alloc::vec::Vec<u8> {
        sig.to_vec()
    }
}

/// A trait specifying the length of a serialized signature for a given parameter set
pub trait SignatureLen {
    /// The length of the signature in bytes
    type SigLen: ArraySize;
}

impl<P: ParameterSet> SignatureEncoding for Signature<P> {
    type Repr = Array<u8, P::SigLen>;

    fn encoded_len(&self) -> usize {
        P::SigLen::USIZE
    }
}

#[cfg(feature = "alloc")]
impl<P: ParameterSet> SignatureBitStringEncoding for Signature<P> {
    fn to_bitstring(&self) -> der::Result<BitString> {
        BitString::new(0, self.to_vec())
    }
}

impl<P: ParameterSet> AssociatedAlgorithmIdentifier for Signature<P> {
    type Params = AnyRef<'static>;

    const ALGORITHM_IDENTIFIER: AlgorithmIdentifierRef<'static> = AlgorithmIdentifierRef {
        oid: P::ALGORITHM_OID,
        parameters: None,
    };
}

impl<P: ParameterSet> From<Signature<P>> for Array<u8, P::SigLen> {
    fn from(sig: Signature<P>) -> Array<u8, P::SigLen> {
        sig.to_bytes()
    }
}

impl<P: ParameterSet> From<&Array<u8, P::SigLen>> for Signature<P> {
    fn from(bytes: &Array<u8, P::SigLen>) -> Signature<P> {
        Signature::try_from(bytes.as_slice()).unwrap()
    }
}

impl SignatureLen for Shake128s {
    type SigLen = U7856;
}

impl SignatureLen for Shake128f {
    type SigLen = U17088;
}

impl SignatureLen for Shake192s {
    type SigLen = U16224;
}

impl SignatureLen for Shake192f {
    type SigLen = U35664;
}

impl SignatureLen for Shake256s {
    type SigLen = U29792;
}

impl SignatureLen for Shake256f {
    type SigLen = U49856;
}

impl SignatureLen for Sha2_128s {
    type SigLen = U7856;
}

impl SignatureLen for Sha2_128f {
    type SigLen = U17088;
}

impl SignatureLen for Sha2_192s {
    type SigLen = U16224;
}

impl SignatureLen for Sha2_192f {
    type SigLen = U35664;
}

impl SignatureLen for Sha2_256s {
    type SigLen = U29792;
}

impl SignatureLen for Sha2_256f {
    type SigLen = U49856;
}

#[cfg(test)]
mod tests {
    use crate::{
        ParameterSet, SigningKey, hashes::*, signature_encoding::Signature,
        util::macros::test_parameter_sets,
    };
    use hybrid_array::Array;
    use signature::{SignatureEncoding, Signer};

    fn test_serialize_deserialize<P: ParameterSet>() {
        let mut rng = rand::rng();
        let sk = SigningKey::<P>::new(&mut rng);
        let msg = b"Hello, world!";
        let sig = sk.try_sign(msg).unwrap();
        let sig_bytes = sig.to_bytes();
        assert_eq!(
            sig.encoded_len(),
            sig_bytes.len(),
            "sig.encoded_len() should equal encoded byte length"
        );
        let sig2 = Signature::<P>::try_from(sig_bytes.as_slice()).unwrap();
        assert_eq!(sig, sig2);
    }

    test_parameter_sets!(test_serialize_deserialize);

    #[cfg(feature = "alloc")]
    fn test_serialize_deserialize_vec<P: ParameterSet>() {
        let mut rng = rand::rng();
        let sk = SigningKey::<P>::new(&mut rng);
        let msg = b"Hello, world!";
        let sig = sk.try_sign(msg).unwrap();
        let sig_vec: alloc::vec::Vec<u8> = (&sig).into();
        assert_eq!(
            sig.encoded_len(),
            sig_vec.len(),
            "sig.encoded_len() should equal encoded byte length"
        );
        let sig2 = Signature::<P>::try_from(sig_vec.as_slice()).unwrap();
        assert_eq!(sig, sig2);
    }

    #[cfg(feature = "alloc")]
    test_parameter_sets!(test_serialize_deserialize_vec);

    #[test]
    fn test_deserialize_fail_on_incorrect_length() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let msg = b"Hello, world!";
        let sig = sk.try_sign(msg).unwrap();
        let sig_bytes: Array<u8, _> = sig.into();
        // Modify the signature bytes to an incorrect length
        let incorrect_sig_bytes = &sig_bytes[..sig_bytes.len() - 1];
        assert!(
            Signature::<Shake128f>::try_from(incorrect_sig_bytes).is_err(),
            "Deserialization should fail on incorrect length"
        );
    }
}
