#![rustfmt::skip]
// Copyright2019 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use p256::{
    ecdsa::{SigningKey, Signature as P256Signature, signature::{Signer, RandomizedSigner}},
};
use p256::ecdsa::VerifyingKey;
use rand_core::OsRng;

use super::rng256::Rng256;
use super::Hash256;

const INT256_NBYTES: usize = 32;
pub const NBYTES: usize = INT256_NBYTES;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "derive_debug", derive(Debug))]
pub struct SecKey {
    k: SigningKey,
    // k: NonZeroExponentP256,
}

impl SecKey {
    // we bypass the OpenSK "rng" specifier, because we have a standard RNG API in our system and do
    // not need to rely on its hack to pass an RNG around.
    pub fn gensk<R>(_rng: &mut R) -> SecKey
    where
        R: Rng256,
    {
        SecKey {
            k: SigningKey::random(&mut OsRng),
            // k: NonZeroExponentP256::gen_uniform(rng),
        }
    }

    pub fn genpk(&self) -> PubKey {
        PubKey {
            p: VerifyingKey::from(&self.k),
            // p: PointP256::base_point_mul(self.k.as_exponent()),
        }
    }

    // ECDSA signature based on a RNG to generate a suitable randomization parameter.
    // Under the hood, rejection sampling is used to make sure that the randomization parameter is
    // uniformly distributed.
    // The provided RNG must be cryptographically secure; otherwise this method is insecure.
    pub fn sign_rng<H, R>(&self, msg: &[u8], _rng: &mut R) -> Signature
    where
        H: Hash256,
        R: Rng256,
    {
        let p256sig = self.k.sign_with_rng(&mut OsRng, msg);
        Signature {
            sig: p256sig
        }
    }

    // Deterministic ECDSA signature based on RFC 6979 to generate a suitable randomization
    // parameter.
    pub fn sign_rfc6979<H>(&self, msg: &[u8]) -> Signature
    where
        H: Hash256,
    {
        let p256sig = self.k.sign(msg);
        Signature {
            sig: p256sig
        }
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Option<SecKey> {
        let sk = SigningKey::from_bytes(bytes);
        match sk {
            Ok(k) => Some(SecKey{k}),
            _ => None
        }
        /*
        let k = NonZeroExponentP256::from_int_checked(Int256::from_bin(bytes));
        // The branching here is fine because all this reveals is whether the key was invalid.
        if bool::from(k.is_none()) {
            return None;
        }
        let k = k.unwrap();
        Some(SecKey { k })*/
    }

    pub fn to_bytes(&self, bytes: &mut [u8; 32]) {
        bytes.copy_from_slice(
            self.k.to_bytes()
            .as_slice()
        );
    }
}

pub struct Signature {
    sig: P256Signature,
    // r: NonZeroExponentP256,
    // s: NonZeroExponentP256,
}
impl Signature {
    pub fn to_asn1_der(&self) -> Vec<u8> {
        // let's hope this shakes out in testing.
        self.sig.to_der().as_bytes().to_vec()
        /*
        const DER_INTEGER_TYPE: u8 = 0x02;
        const DER_DEF_LENGTH_SEQUENCE: u8 = 0x30;
        let r_encoding = self.sig.r().to_int().to_minimal_encoding();
        let s_encoding = self.s.to_int().to_minimal_encoding();
        // We rely on the encoding to be short enough such that
        // sum of lengths + 4 still fits into 7 bits.
        #[cfg(test)]
        assert!(r_encoding.len() <= 33);
        #[cfg(test)]
        assert!(s_encoding.len() <= 33);
        // The ASN1 of a signature is a two member sequence. Its length is the
        // sum of the integer encoding lengths and 2 header bytes per integer.
        let mut encoding = vec![
            DER_DEF_LENGTH_SEQUENCE,
            (r_encoding.len() + s_encoding.len() + 4) as u8,
        ];
        encoding.push(DER_INTEGER_TYPE);
        encoding.push(r_encoding.len() as u8);
        encoding.extend(r_encoding);
        encoding.push(DER_INTEGER_TYPE);
        encoding.push(s_encoding.len() as u8);
        encoding.extend(s_encoding);
        encoding */
    }

    // #[cfg(feature = "std")]
    fn to_p256(&self) -> P256Signature {
        self.sig
    }

    // this is only used by tests
    pub fn from_bytes(bytes: &[u8]) -> Option<Signature> {
        use p256::{NonZeroScalar, FieldBytes};
        use std::convert::TryFrom;
        if bytes.len() != 64 {
            None
        } else {
            let maybe_r_nzs = NonZeroScalar::try_from(&bytes[..32]);
            let maybe_s_nzs = NonZeroScalar::try_from(&bytes[32..]);
            if let Ok(r_nzs) = maybe_r_nzs {
                if let Ok(s_nzs) = maybe_s_nzs {
                    match P256Signature::from_scalars(
                        FieldBytes::from(r_nzs), // r
                        FieldBytes::from(s_nzs), // s
                    ) {
                        Ok(sig) => Some(Signature{sig}),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
            /*
            let r =
                NonZeroExponentP256::from_int_checked(Int256::from_bin(array_ref![bytes, 0, 32]));
            let s =
                NonZeroExponentP256::from_int_checked(Int256::from_bin(array_ref![bytes, 32, 32]));
            if bool::from(r.is_none()) || bool::from(s.is_none()) {
                return None;
            }
            let r = r.unwrap();
            let s = s.unwrap();
            Some(Signature { r, s })*/
        }
    }

    #[cfg(test)]
    fn to_bytes(&self, bytes: &mut [u8; 64]) {
        bytes[..32].copy_from_slice(self.sig.r().to_bytes().as_slice());
        bytes[32..].copy_from_slice(self.sig.s().to_bytes().as_slice());
        //self.r.to_int().to_bin(array_mut_ref![bytes, 0, 32]);
        //self.s.to_int().to_bin(array_mut_ref![bytes, 32, 32]);
    }
}

pub struct PubKey {
    p: VerifyingKey,
    // p: PointP256,
}
impl PubKey {
    pub const ES256_ALGORITHM: i64 = -7;
    #[cfg(feature = "with_ctap1")]
    const UNCOMPRESSED_LENGTH: usize = 1 + 2 * INT256_NBYTES;

    // #[cfg(feature = "std")]
    pub fn from_bytes_uncompressed(bytes: &[u8]) -> Option<PubKey> {
        match VerifyingKey::from_sec1_bytes(bytes) {
            Ok(p) => Some(PubKey{p}),
            _ => None
        }
    }

    /// Creates a new PubKey from its coordinates on the elliptic curve.
    pub fn from_coordinates(x: &[u8; INT256_NBYTES], y: &[u8; INT256_NBYTES]) -> Option<PubKey> {
        let encoded_point: p256::EncodedPoint =
            p256::EncodedPoint::from_affine_coordinates(x.into(), y.into(), false);
        match VerifyingKey::from_encoded_point(&encoded_point) {
            Ok(p) => Some(PubKey{p}),
            _ => None
        }
    }

    //#[cfg(test)] - commented out because for some reason we can't get the test configs to work right
    #[allow(dead_code)]
    fn to_bytes_uncompressed(&self, bytes: &mut [u8; 65]) {
        // not sure if this is correct -- we'll find out when we run the tests
        // this generates a SEC1 EncodedPoint, but I'm not sure if that's the same thing as
        // a SEC1-encoded public key.
        bytes.copy_from_slice(
            self.p.to_encoded_point(false)
            .as_bytes()
        )
        // self.p.to_bytes_uncompressed(bytes);
    }

    #[cfg(feature = "with_ctap1")]
    pub fn to_uncompressed(&self) -> [u8; PubKey::UNCOMPRESSED_LENGTH] {
        use arrayref::mut_array_refs;
        // Formatting according to:
        // https://tools.ietf.org/id/draft-jivsov-ecc-compact-05.html#overview
        const B0_BYTE_MARKER: u8 = 0x04;
        let mut representation = [0; PubKey::UNCOMPRESSED_LENGTH];
        let (marker, x, y) =
            mut_array_refs![&mut representation, 1, INT256_NBYTES, INT256_NBYTES];
        marker[0] = B0_BYTE_MARKER;
        x.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .x().unwrap()
            .as_slice()
        );
        y.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .y().unwrap()
            .as_slice()
        );
        representation
    }

    /// Writes the coordinates into the passed in arrays.
    pub fn to_coordinates(&self, x: &mut [u8; NBYTES], y: &mut [u8; NBYTES]) {
        x.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .x().unwrap()
            .as_slice()
        );
        y.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .y().unwrap()
            .as_slice()
        );
    }

    // #[cfg(feature = "std")]
    pub fn verify_vartime<H>(&self, msg: &[u8], sign: &Signature) -> bool
    where
        H: Hash256,
    {
        use p256::ecdsa::signature::Verifier;
        self.p.verify(msg, &sign.to_p256()).is_ok()
        /*
        let m = ExponentP256::modn(Int256::from_bin(&H::hash(msg)));

        let v = sign.s.inv();
        let u = &m * v.as_exponent();
        let v = &sign.r * &v;

        let u = self.p.points_mul(&u, v.as_exponent()).getx();

        ExponentP256::modn(u.to_int()) == *sign.r.as_exponent()
        */
    }
}

#[cfg(test)]
mod test {
    use super::super::rng256::ThreadRng256;
    use super::super::sha256::Sha256;
    use super::*;
    use arrayref::{array_ref, array_mut_ref};
    use byteorder::{BigEndian, ByteOrder};
    use p256::SecretKey;

    const BITS_PER_DIGIT: usize = 32;
    const BYTES_PER_DIGIT: usize = BITS_PER_DIGIT >> 3;
    const NDIGITS: usize = 8;
    pub const NBYTES: usize = NDIGITS * BYTES_PER_DIGIT;
    pub type Digit = u32;
    #[derive(Default)]
    pub struct Int256 {
        digits: [Digit; NDIGITS],
    }

    #[allow(clippy::unreadable_literal)]
    impl Int256 {
        // Curve order (prime)
        pub const N: Int256 = Int256 {
            digits: [
                0xfc632551, 0xf3b9cac2, 0xa7179e84, 0xbce6faad, 0xffffffff, 0xffffffff, 0x00000000,
                0xffffffff,
            ],
        };
        pub fn from_bin(src: &[u8; NBYTES]) -> Int256 {
            let mut digits = [0; NDIGITS];
            for i in 0..NDIGITS {
                digits[NDIGITS - 1 - i] = BigEndian::read_u32(array_ref![src, 4 * i, 4]);
            }
            Int256 { digits }
        }
        pub fn to_bin(&self, dst: &mut [u8; NBYTES]) {
            for i in 0..NDIGITS {
                BigEndian::write_u32(array_mut_ref![dst, 4 * i, 4], self.digits[NDIGITS - 1 - i]);
            }
        }
    }


    // Run more test iterations in release mode, as the code should be faster.
    #[cfg(not(debug_assertions))]
    const ITERATIONS: u32 = 10000;
    #[cfg(debug_assertions)]
    const ITERATIONS: u32 = 500;

    /** Test that key generation creates valid keys **/
    #[test]
    fn test_genpk_is_valid_random() {
        use p256::AffinePoint;
        use p256::elliptic_curve::sec1::FromEncodedPoint;
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            let encoded = pk.p.to_encoded_point(false);
            let maybe_affine = AffinePoint::from_encoded_point(&encoded);
            assert!(bool::from(maybe_affine.is_some()));
            // assert!(pk.p.is_valid_vartime());
        }
    }

    /** Serialization **/
    #[test]
    fn test_seckey_to_bytes_from_bytes() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk = SecKey::gensk(&mut rng);
            let mut bytes = [0; 32];
            sk.to_bytes(&mut bytes);
            let decoded_sk = SecKey::from_bytes(&bytes).unwrap();
            let mut checkbytes = [0; 32];
            decoded_sk.to_bytes(&mut checkbytes);
            assert_eq!(checkbytes, bytes);
        }
    }

    #[test]
    fn test_seckey_from_bytes_zero() {
        // Zero is not a valid exponent for a secret key.
        let bytes = [0; 32];
        let sk = SecKey::from_bytes(&bytes);
        assert!(sk.is_none());
    }

    #[test]
    fn test_seckey_from_bytes_n() {
        let mut bytes = [0; 32];
        Int256::N.to_bin(&mut bytes);
        let sk = SecKey::from_bytes(&bytes);
        assert!(sk.is_none());
    }

    #[test]
    fn test_seckey_from_bytes_ge_n() {
        let bytes = [0xFF; 32];
        let sk = SecKey::from_bytes(&bytes);
        assert!(sk.is_none());
    }

    /** Test vectors from RFC6979 **/
    fn int256_from_hex(x: &str) -> Int256 {
        let bytes = hex::decode(x).unwrap();
        assert_eq!(bytes.len(), 32);
        Int256::from_bin(array_ref![bytes.as_slice(), 0, 32])
    }

    // Test vectors from RFC6979, Section A.2.5.
    const RFC6979_X: &str = "C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721";
    const RFC6979_UX: &str = "60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6";
    const RFC6979_UY: &str = "7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299";

    #[test]
    fn test_rfc6979_keypair() {
        let sk = SecKey {
            k: SigningKey::from_bytes(&hex::decode(RFC6979_X).unwrap()).unwrap(),
            // k: NonZeroExponentP256::from_int_checked(int256_from_hex(RFC6979_X)).unwrap(),
        };
        let pk = sk.genpk();
        let encoded = pk.p.to_encoded_point(false);
        assert_eq!(encoded.x().unwrap().as_slice(), &hex::decode(RFC6979_UX).unwrap());
        assert_eq!(encoded.y().unwrap().as_slice(), &hex::decode(RFC6979_UY).unwrap());
    }

    fn test_rfc6979(msg: &str, k: &str, r: &str, s: &str) {
        let key = SigningKey::from_bytes(&hex::decode(RFC6979_X).unwrap()).unwrap();
        let sk = SecKey {
            k: key,
            // k: NonZeroExponentP256::from_int_checked(int256_from_hex(RFC6979_X)).unwrap(),
        };
        /* // unfortunately we don't have a routine to extract `k` in the RustCrypto libraries
        assert_eq!(
            sk.get_k_rfc6979::<Sha256>(msg.as_bytes()).to_int(),
            int256_from_hex(k)
        ); */
        let sign = sk.sign_rfc6979::<Sha256>(msg.as_bytes());
        let mut rs = [0u8; 64];
        sign.to_bytes(&mut rs);
        assert_eq!(&rs[..32], &hex::decode(r).unwrap());
        assert_eq!(&rs[32..], &hex::decode(s).unwrap());
    }

    #[test]
    fn test_rfc6979_sample() {
        let msg = "sample";
        let k = "A6E3C57DD01ABE90086538398355DD4C3B17AA873382B0F24D6129493D8AAD60";
        let r = "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716";
        let s = "F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8";
        test_rfc6979(msg, k, r, s);
    }

    #[test]
    fn test_rfc6979_test() {
        let msg = "test";
        let k = "D16B6AE827F17175E040871A1C7EC3500192C4C92677336EC2537ACAEE0008E0";
        let r = "F1ABB023518351CD71D881567B1EA663ED3EFCF6C5132B354F28D3B0B7D38367";
        let s = "019F4113742A2B14BD25926B49C649155F267E60D3814B4C0CC84250E46F0083";
        test_rfc6979(msg, k, r, s);
    }

    /** Tests that sign and verify are consistent **/
    // Test that signed messages are correctly verified.
    #[test]
    fn test_sign_rfc6979_verify_random() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let msg = rng.gen_uniform_u8x32();
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            let sign = sk.sign_rfc6979::<Sha256>(&msg);
            assert!(pk.verify_vartime::<Sha256>(&msg, &sign));
        }
    }

    // Test that signed messages are correctly verified.
    #[test]
    fn test_sign_verify_random() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let msg = rng.gen_uniform_u8x32();
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            let sign = sk.sign_rng::<Sha256, _>(&msg, &mut rng);
            assert!(pk.verify_vartime::<Sha256>(&msg, &sign));
        }
    }

    /** Tests that this code is compatible with the ring crate **/
    // Test that the ring crate works properly.
    #[test]
    fn test_ring_sign_ring_verify() {
        use ring::rand::SecureRandom;
        use ring::signature::{KeyPair, VerificationAlgorithm};

        let ring_rng = ring::rand::SystemRandom::new();

        for _ in 0..ITERATIONS {
            let mut msg_bytes: [u8; 64] = [Default::default(); 64];
            ring_rng.fill(&mut msg_bytes).unwrap();

            let pkcs8_bytes = ring::signature::EcdsaKeyPair::generate_pkcs8(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &ring_rng,
            )
            .unwrap();
            let key_pair = ring::signature::EcdsaKeyPair::from_pkcs8(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                pkcs8_bytes.as_ref(),
            )
            .unwrap();
            let public_key_bytes = key_pair.public_key().as_ref();

            let sig = key_pair.sign(&ring_rng, &msg_bytes).unwrap();
            let sig_bytes = sig.as_ref();

            assert!(ring::signature::ECDSA_P256_SHA256_FIXED
                .verify(
                    untrusted::Input::from(public_key_bytes),
                    untrusted::Input::from(&msg_bytes),
                    untrusted::Input::from(sig_bytes)
                )
                .is_ok());
        }
    }

    // Test that messages signed by the ring crate are correctly verified by this code.
    #[test]
    fn test_ring_sign_self_verify() {
        use ring::rand::SecureRandom;
        use ring::signature::KeyPair;

        let ring_rng = ring::rand::SystemRandom::new();

        for _ in 0..ITERATIONS {
            let mut msg_bytes: [u8; 64] = [Default::default(); 64];
            ring_rng.fill(&mut msg_bytes).unwrap();

            let pkcs8_bytes = ring::signature::EcdsaKeyPair::generate_pkcs8(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &ring_rng,
            )
            .unwrap();
            let key_pair = ring::signature::EcdsaKeyPair::from_pkcs8(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                pkcs8_bytes.as_ref(),
            )
            .unwrap();
            let public_key_bytes = key_pair.public_key().as_ref();

            let sig = key_pair.sign(&ring_rng, &msg_bytes).unwrap();
            let sig_bytes = sig.as_ref();

            let pk = PubKey::from_bytes_uncompressed(public_key_bytes).unwrap();
            let sign = Signature::from_bytes(sig_bytes).unwrap();
            assert!(pk.verify_vartime::<Sha256>(&msg_bytes, &sign));
        }
    }

    // Test that messages signed by this code are correctly verified by the ring crate.
    #[test]
    fn test_self_sign_ring_verify() {
        use ring::signature::VerificationAlgorithm;

        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let msg_bytes = rng.gen_uniform_u8x32();
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            let sign = sk.sign_rng::<Sha256, _>(&msg_bytes, &mut rng);

            let mut public_key_bytes: [u8; 65] = [Default::default(); 65];
            pk.to_bytes_uncompressed(&mut public_key_bytes);
            let mut sig_bytes: [u8; 64] = [Default::default(); 64];
            sign.to_bytes(&mut sig_bytes);

            assert!(ring::signature::ECDSA_P256_SHA256_FIXED
                .verify(
                    untrusted::Input::from(&public_key_bytes),
                    untrusted::Input::from(&msg_bytes),
                    untrusted::Input::from(&sig_bytes)
                )
                .is_ok());
        }
    }

    #[test]
    fn test_signature_to_asn1_der_short_encodings() {
        let r_bytes = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01,
        ];
        //let r = NonZeroExponentP256::from_int_checked(Int256::from_bin(&r_bytes)).unwrap();
        let s_bytes = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xFF,
        ];
        //let s = NonZeroExponentP256::from_int_checked(Int256::from_bin(&s_bytes)).unwrap();
        //let signature = Signature { r, s };
        let signature = Signature::from_bytes(
            &[r_bytes, s_bytes].concat()
        ).unwrap();
        let expected_encoding = vec![0x30, 0x07, 0x02, 0x01, 0x01, 0x02, 0x02, 0x00, 0xFF];

        assert_eq!(signature.to_asn1_der(), expected_encoding);
    }

    #[test]
    fn test_signature_to_asn1_der_long_encodings() {
        let r_bytes = [
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA,
        ];
        //let r = NonZeroExponentP256::from_int_checked(Int256::from_bin(&r_bytes)).unwrap();
        let s_bytes = [
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB,
        ];
        //let s = NonZeroExponentP256::from_int_checked(Int256::from_bin(&s_bytes)).unwrap();
        //let signature = Signature { r, s };
        let signature = Signature::from_bytes(
            &[r_bytes, s_bytes].concat()
        ).unwrap();
        let expected_encoding = vec![
            0x30, 0x46, 0x02, 0x21, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x02, 0x21, 0x00, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB,
        ];

        assert_eq!(signature.to_asn1_der(), expected_encoding);
    }

    // TODO: Test edge-cases and compare the behavior with ring.
    // - Invalid public key (at infinity, values not less than the prime p), but ring doesn't
    // directly exposes key validation in its API.
}
