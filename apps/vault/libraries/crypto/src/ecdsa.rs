// Copyright 2019 Google LLC
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

use cbor::{cbor_bytes, cbor_map_options};

use super::rng256::Rng256;
use super::Hash256;

const INT256_NBYTES: usize = 32;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "derive_debug", derive(Debug))]
pub struct SecKey {
    k: SigningKey,
    // k: NonZeroExponentP256,
}

pub struct Signature {
    sig: P256Signature,
    // r: NonZeroExponentP256,
    // s: NonZeroExponentP256,
}

pub struct PubKey {
    p: VerifyingKey,
    // p: PointP256,
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

    #[cfg(feature = "std")]
    fn to_p256(&self) -> P256Signature {
        self.sig
    }

    #[cfg(feature = "std")]
    pub fn from_bytes(bytes: &[u8]) -> Option<Signature> {
        if bytes.len() != 64 {
            None
        } else {
            match P256Signature::from_scalars(
                bytes[..32].try_into().unwrap(), // r
                bytes[32..].try_into().unwrap(), // s
            ) {
                Ok(sig) => Some(Signature{sig}),
                _ => None,
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
        use core::slice::SlicePattern;

        bytes[..32].copy_from_slice(self.sig.r().to_bytes().as_slice());
        bytes[32..].copy_from_slice(self.sig.s().to_bytes().as_slice());
        //self.r.to_int().to_bin(array_mut_ref![bytes, 0, 32]);
        //self.s.to_int().to_bin(array_mut_ref![bytes, 32, 32]);
    }
}

impl PubKey {
    pub const ES256_ALGORITHM: i64 = -7;
    #[cfg(feature = "with_ctap1")]
    const UNCOMPRESSED_LENGTH: usize = 1 + 2 * INT256_NBYTES;

    #[cfg(feature = "std")]
    pub fn from_bytes_uncompressed(bytes: &[u8]) -> Option<PubKey> {
        match VerifyingKey::from_sec1_bytes(bytes) {
            Ok(p) => Some(PubKey{p}),
            _ => None
        }
    }

    #[cfg(test)]
    fn to_bytes_uncompressed(&self, bytes: &mut [u8; 65]) {
        // not sure if this is correct -- we'll find out when we run the tests
        // this generates a SEC1 EncodedPoint, but I'm not sure if that's the same thing as
        // a SEC1-encoded public key.
        bytes.copy_from_slice(
            self.p.to_encoded_point(false)
            .to_bytes()
            .as_slice()
        )
        // self.p.to_bytes_uncompressed(bytes);
    }

    #[cfg(feature = "with_ctap1")]
    pub fn to_uncompressed(&self) -> [u8; PubKey::UNCOMPRESSED_LENGTH] {
        // Formatting according to:
        // https://tools.ietf.org/id/draft-jivsov-ecc-compact-05.html#overview
        const B0_BYTE_MARKER: u8 = 0x04;
        let mut representation = [0; PubKey::UNCOMPRESSED_LENGTH];
        let (marker, x, y) =
            mut_array_refs![&mut representation, 1, int256::NBYTES, int256::NBYTES];
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

    // Encodes the key according to CBOR Object Signing and Encryption, defined in RFC 8152.
    pub fn to_cose_key(&self) -> Option<Vec<u8>> {
        const EC2_KEY_TYPE: i64 = 2;
        const P_256_CURVE: i64 = 1;
        let mut x_bytes = vec![0; INT256_NBYTES];
        x_bytes.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .x().unwrap()
            .as_slice()
        );
        let x_byte_cbor: cbor::Value = cbor_bytes!(x_bytes);
        let mut y_bytes = vec![0; INT256_NBYTES];
        y_bytes.copy_from_slice(
            self.p
            .to_encoded_point(false)
            .y().unwrap()
            .as_slice()
        );
        let y_byte_cbor: cbor::Value = cbor_bytes!(y_bytes);
        let cbor_value = cbor_map_options! {
            1 => EC2_KEY_TYPE,
            3 => PubKey::ES256_ALGORITHM,
            -1 => P_256_CURVE,
            -2 => x_byte_cbor,
            -3 => y_byte_cbor,
        };
        let mut encoded_key = Vec::new();
        if cbor::write(cbor_value, &mut encoded_key) {
            Some(encoded_key)
        } else {
            None
        }
    }

    #[cfg(feature = "std")]
    pub fn verify_vartime<H>(&self, msg: &[u8], sign: &Signature) -> bool
    where
        H: Hash256,
    {
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

    // Run more test iterations in release mode, as the code should be faster.
    #[cfg(not(debug_assertions))]
    const ITERATIONS: u32 = 10000;
    #[cfg(debug_assertions)]
    const ITERATIONS: u32 = 500;

    /** Test that key generation creates valid keys **/
    #[test]
    fn test_genpk_is_valid_random() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            assert!(pk.p.is_valid_vartime());
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
            let decoded_sk = SecKey::from_bytes(&bytes);
            assert_eq!(decoded_sk, Some(sk));
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
            k: NonZeroExponentP256::from_int_checked(int256_from_hex(RFC6979_X)).unwrap(),
        };
        let pk = sk.genpk();
        assert_eq!(pk.p.getx().to_int(), int256_from_hex(RFC6979_UX));
        assert_eq!(pk.p.gety().to_int(), int256_from_hex(RFC6979_UY));
    }

    fn test_rfc6979(msg: &str, k: &str, r: &str, s: &str) {
        let sk = SecKey {
            k: NonZeroExponentP256::from_int_checked(int256_from_hex(RFC6979_X)).unwrap(),
        };
        assert_eq!(
            sk.get_k_rfc6979::<Sha256>(msg.as_bytes()).to_int(),
            int256_from_hex(k)
        );
        let sign = sk.sign_rfc6979::<Sha256>(msg.as_bytes());
        assert_eq!(sign.r.to_int(), int256_from_hex(r));
        assert_eq!(sign.s.to_int(), int256_from_hex(s));
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
        let r = NonZeroExponentP256::from_int_checked(Int256::from_bin(&r_bytes)).unwrap();
        let s_bytes = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xFF,
        ];
        let s = NonZeroExponentP256::from_int_checked(Int256::from_bin(&s_bytes)).unwrap();
        let signature = Signature { r, s };
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
        let r = NonZeroExponentP256::from_int_checked(Int256::from_bin(&r_bytes)).unwrap();
        let s_bytes = [
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB,
        ];
        let s = NonZeroExponentP256::from_int_checked(Int256::from_bin(&s_bytes)).unwrap();
        let signature = Signature { r, s };
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
