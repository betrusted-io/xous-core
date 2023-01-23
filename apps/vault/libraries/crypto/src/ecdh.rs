// Xous maintainer's note:
//
// This library is vendored in from the Google OpenSK reference implementation.
// The OpenSK library contains its own implementations of crypto functions.
// The port to Xous attempts to undo that, but where possible leaves a thin
// adapter between the OpenSK custom APIs and the more "standard" Rustcrypto APIs.
// There is always a hazard in adapting crypto APIs and reviewers should take
// note of this. However, by calling out the API differences, it hopefully highlights
// any potential problems in the OpenSK library, rather than papering them over.
//
// Leaving the OpenSK APIs in place also makes it easier to apply upstream
// patches from OpenSK to fix bugs in the code base.

// Original copyright notice preserved below:

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

// Additional note: currently this is a very low-confidence API port. Much testing needed.

use p256::{EncodedPoint, PublicKey};
use p256::ecdh::EphemeralSecret;
use rand_core::OsRng;
use p256::elliptic_curve::consts::U32;
use p256::elliptic_curve::generic_array::GenericArray;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use std::convert::TryInto;
#[cfg(test)]
use std::convert::TryFrom;
#[cfg(test)]
use p256::ecdh::SharedSecret;
#[cfg(test)]
use p256::elliptic_curve::ecdh::diffie_hellman;
#[cfg(test)]
use p256::NonZeroScalar;
#[cfg(test)]
use rand_core::{CryptoRng, RngCore};

use crate::sha256::Sha256;
use crate::Hash256;

use super::rng256::Rng256;
pub const NBYTES: usize = 32;

#[cfg(test)]
pub struct SecKey {
    a: EphemeralSecretTest,
}
#[cfg(not(test))]
pub struct SecKey {
    a: EphemeralSecret,
}

//#[cfg_attr(test, derive(Clone, PartialEq, Debug))]
#[derive(Clone, PartialEq, Debug)]
pub struct PubKey {
    // pub p: EncodedPoint,
    pub p: PublicKey,
}

impl SecKey {
    // we bypass the OpenSK "rng" specifier, because we have a standard RNG API in our system and do
    // not need to rely on its hack to pass an RNG around.
    pub fn gensk<R>(_rng: &mut R) -> SecKey
    where
        R: Rng256,
    {
        SecKey {
            #[cfg(test)]
            a: EphemeralSecretTest::random(&mut OsRng),
            #[cfg(not(test))]
            a: EphemeralSecret::random(&mut OsRng),
        }
    }
    #[cfg(test)]
    pub fn to_bytes(&self) -> Vec::<u8> {
        self.a.to_bytes()
    }
    #[cfg(test)]
    pub fn from_bytes(b: &[u8]) -> SecKey {
        SecKey {
            a: EphemeralSecretTest::from_bytes(b),
        }
    }

    pub fn genpk(&self) -> PubKey {
        PubKey {
            p: PublicKey::from_sec1_bytes(EncodedPoint::from(self.a.public_key()).as_bytes()).expect("invalid self-generated PK"),
        }
    }
    // DH key agreement method defined in the FIDO2 specification, Section 5.5.4. "Getting
    // sharedSecret from Authenticator"
    pub fn exchange_x_sha256(&self, other: &PubKey) -> [u8; 32] {
        let shared = self.a.diffie_hellman(&other.p);
        let mut hasher = Sha256::new();
        hasher.update(shared.as_bytes().as_slice().try_into().unwrap());
        hasher.finalize()
    }

    pub fn exchange_x(&self, other: &PubKey) -> [u8; 32] {
        let shared = self.a.diffie_hellman(&other.p);
        let mut ret = [0u8; 32];
        ret.copy_from_slice(shared.as_bytes().as_slice());
        ret
    }
}

#[cfg(test)]
pub struct EphemeralSecretTest {
    scalar: NonZeroScalar,
}
#[cfg(test)]
impl EphemeralSecretTest
{
    pub fn random(rng: impl CryptoRng + RngCore) -> Self {
        Self {
            scalar: NonZeroScalar::random(rng),
        }
    }
    pub fn to_bytes(&self) -> Vec::<u8> {
        self.scalar.to_bytes().to_vec()
    }
    /// Generate an [`EphemeralSecret`] from a test vector.
    pub fn from_bytes(b: &[u8]) -> Self {
        Self {
            scalar: NonZeroScalar::try_from(b).unwrap(),
        }
    }

    /// Get the public key associated with this ephemeral secret.
    ///
    /// The `compress` flag enables point compression.
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_secret_scalar(&self.scalar)
    }

    /// Compute a Diffie-Hellman shared secret from an ephemeral secret and the
    /// public key of the other participant in the exchange.
    pub fn diffie_hellman(&self, public_key: &PublicKey) -> SharedSecret {
        diffie_hellman(&self.scalar, public_key.as_affine())
    }
}

impl PubKey {
    #[cfg(test)]
    fn from_bytes_uncompressed(bytes: &[u8]) -> Option<PubKey> {
        use p256::elliptic_curve::sec1::FromEncodedPoint;

        if let Ok(ep) = EncodedPoint::from_bytes(bytes) {
            let maybe_p = PublicKey::from_encoded_point(&ep);
            if bool::from(maybe_p.is_some()) {
                Some(PubKey {p: maybe_p.unwrap()})
            } else {
                None
            }
            // PointP256::from_bytes_uncompressed_vartime(bytes).map(|p| PubKey { p })
        } else {
            None
        }
    }

    #[cfg(test)]
    fn to_bytes_uncompressed(&self, bytes: &mut [u8; 65]) {
        bytes.copy_from_slice(self.p.to_encoded_point(false).as_bytes());
        //self.p.to_bytes_uncompressed(bytes);
    }

    pub fn from_coordinates(x: &[u8; NBYTES], y: &[u8; NBYTES]) -> Option<PubKey> {
        match PublicKey::from_sec1_bytes(
            EncodedPoint::from_affine_coordinates(
                GenericArray::<u8, U32>::from_slice(x),
                GenericArray::<u8, U32>::from_slice(y),
                false
            ).as_bytes()
        ) {
            Ok(p) => {
                Some(PubKey{p})
            }
            _ => None,
        }
    }

    pub fn to_coordinates(&self, x: &mut [u8; NBYTES], y: &mut [u8; NBYTES]) {
        let enc_point = self.p.to_encoded_point(false);
        x.copy_from_slice(
            enc_point.x().unwrap().as_slice()
        );
        y.copy_from_slice(
            enc_point.y().unwrap().as_slice()
        );
    }
}

#[cfg(test)]
mod test {
    use p256::AffinePoint;
    use p256::elliptic_curve::sec1::FromEncodedPoint;

    use crate::sha256::Sha256;

    use super::super::rng256::ThreadRng256;
    use super::*;

    // Run more test iterations in release mode, as the code should be faster.
    #[cfg(not(debug_assertions))]
    const ITERATIONS: u32 = 10000;
    #[cfg(debug_assertions)]
    const ITERATIONS: u32 = 500;

    // This test looks weird, but it's coded so I can copy-paste vectors from both a test bench and the device
    // and confirm that the math is correct, and that the vectors as printed are also correct without having
    // to manually check them. The test is also good in that it checks a full e2e exchange between device
    // and host, as opposed to just checking againt itself.
    #[test]
    fn ecdh_test_vectors() {
        let host_bytes = hex::decode("555054552084aad44bc2306fc07723309c679b2b94c030fc39b23c670813da9e").unwrap();
        let host_test = EphemeralSecretTest::from_bytes(host_bytes.as_slice());
        let host_pk = host_test.public_key();
        println!("host_ephemeral: {:x?}", host_bytes);
        println!("host_pk: {:?}", host_pk);
        let mut hpk_x = [0u8; 32];
        let mut hpk_y = [0u8; 32];
        hpk_x.copy_from_slice(host_pk.to_encoded_point(false).x().unwrap().as_slice());
        hpk_y.copy_from_slice(host_pk.to_encoded_point(false).y().unwrap().as_slice());
        println!("HOST pk coords computed from private key (host_pk): x: {:?} y: {:?}", hpk_x, hpk_y);
        let hpk_x_devreported = [28u8, 37, 33, 239, 215, 47, 125, 182, 240, 107, 35, 100, 62, 18, 4, 87, 187, 171, 184, 10, 171, 34, 173, 102, 33, 97, 192, 12, 229, 101, 232, 37];
        let hpk_y_devreported = [234u8, 162, 255, 73, 37, 228, 113, 9, 190, 194, 228, 203, 16, 80, 70, 55, 220, 244, 109, 232, 83, 169, 100, 10, 203, 220, 185, 32, 222, 78, 128, 255];
        let hpk_x_hstreported = hex::decode("1c2521efd72f7db6f06b23643e120457bbabb80aab22ad662161c00ce565e825").unwrap();
        let hpk_y_hstreported = hex::decode("eaa2ff4925e47109bec2e4cb10504637dcf46de853a9640acbdcb920de4e80ff").unwrap();
        let host_pk_native = PubKey::from_coordinates(
            &hpk_x_devreported,
            &hpk_y_devreported,
        ).unwrap();
        println!("hpk_x_devreported: {:?}", hpk_x_devreported);
        println!("hpk_x_hstreported: {:?}", hpk_x_hstreported);
        println!("hpk_y_devreported: {:?}", hpk_y_devreported.as_slice());
        println!("hpk_y_hstreported: {:?}", hpk_y_hstreported.as_slice());
        assert!(&hpk_x_devreported == hpk_x_hstreported.as_slice());
        assert!(&hpk_y_devreported == hpk_y_hstreported.as_slice());

        let device_bytes = [226, 46, 196, 239, 246, 91, 241, 12, 18, 69, 191, 4, 251, 90, 35, 175, 171, 187, 133, 22, 70, 129, 145, 180, 143, 163, 55, 184, 180, 53, 21, 23];
        let device_test = EphemeralSecretTest::from_bytes(device_bytes.as_slice());
        let device_pk_computed = device_test.public_key();
        let mut dev_x_computed = [0u8; 32];
        let mut dev_y_computed = [0u8; 32];
        dev_x_computed.copy_from_slice(device_pk_computed.to_encoded_point(false).x().unwrap().as_slice());
        dev_y_computed.copy_from_slice(device_pk_computed.to_encoded_point(false).y().unwrap().as_slice());
        println!("dev_x_computed: {:?}", dev_x_computed);
        println!("dev_y_computed: {:?}", dev_y_computed);

        let device_x_devreported = [19, 104, 193, 88, 255, 172, 103, 22, 8, 144, 14, 69, 205, 48, 242, 20, 61, 24, 127, 2, 174, 186, 129, 60, 134, 207, 226, 128, 98, 33, 71, 17];
        let device_y_devreported = [49, 167, 248, 159, 106, 202, 174, 64, 46, 251, 201, 62, 73, 12, 109, 207, 121, 72, 58, 202, 247, 230, 61, 193, 52, 90, 128, 141, 28, 255, 48, 82];
        let device_pk = PubKey::from_coordinates(
            &device_x_devreported,
            &device_y_devreported,
        ).unwrap();
        let device_x_hstreported = [19u8, 104, 193, 88, 255, 172, 103, 22, 8, 144, 14, 69, 205, 48, 242, 20, 61, 24, 127, 2, 174, 186, 129, 60, 134, 207, 226, 128, 98, 33, 71, 17,  ];
        let device_y_hstreported = [49u8, 167, 248, 159, 106, 202, 174, 64, 46, 251, 201, 62, 73, 12, 109, 207, 121, 72, 58, 202, 247, 230, 61, 193, 52, 90, 128, 141, 28, 255, 48, 82,  ];
        println!("dev_x_hstreported: {:?}", device_x_hstreported);
        println!("dev_y_hstreported: {:?}", device_y_hstreported);
        assert!(device_x_hstreported == device_x_devreported);
        assert!(device_y_hstreported == device_y_devreported);
        assert!(dev_x_computed == device_x_hstreported);
        assert!(dev_y_computed == device_y_hstreported);
        println!("device_pk {:?}", device_pk);

        // this computation is just wrong?
        let dev_sec_key = SecKey::from_bytes(&device_bytes);
        //let ss_enc = host_test.diffie_hellman(&device_pk.p);
        //let ss = ss_enc.as_bytes();
        let ss = dev_sec_key.exchange_x_sha256(&host_pk_native);
        // ss is derived using the host's private key, and the device's public key:
        println!("shared secret: {:?}", ss);

        let host_ss: [u8; 32] = [127, 102, 177, 139, 141, 9, 166, 158, 187, 8, 208, 104, 131, 208, 114, 196, 22, 16, 179, 94, 237, 156, 143, 7, 157, 188, 10, 227, 168, 171, 124, 217,  ];
        //assert!(&host_ss == ss.as_slice());
        let dev_printed_ss: [u8; 32] = [243, 239, 96, 163, 197, 222, 100, 144, 94, 171, 211, 129, 183, 100, 165, 75, 6, 210, 183, 83, 224, 80, 127, 250, 42, 145, 139, 202, 193, 73, 166, 53];
        //assert!(&dev_printed_ss == ss.as_slice());

        let contents = [163, 75, 48, 54, 207, 234, 121, 106, 88, 111, 33, 197, 16, 45, 205, 61, 178, 38, 2, 179, 235, 124, 199, 89, 206, 47, 108, 147, 129, 238, 30, 12, 17, 59, 211, 73, 89, 94, 14, 45, 27, 18, 136, 80, 14, 65, 127, 186, 70, 207, 32, 26, 231, 82, 197, 119, 1, 140, 13, 15, 185, 46, 77, 135];
        let contents2 = [163u8, 75, 48, 54, 207, 234, 121, 106, 88, 111, 33, 197, 16, 45, 205, 61, 178, 38, 2, 179, 235, 124, 199, 89, 206, 47, 108, 147, 129, 238, 30, 12, 17, 59, 211, 73, 89, 94, 14, 45, 27, 18, 136, 80, 14, 65, 127, 186, 70, 207, 32, 26, 231, 82, 197, 119, 1, 140, 13, 15, 185, 46, 77, 135,  ];
        assert!(contents == contents2);
        let pin = [193, 72, 232, 147, 205, 36, 2, 24, 145, 248, 146, 163, 227, 67, 221, 246];
        let computed_mac = crate::hmac::hmac_256::<Sha256>(ss.as_slice(), &contents);
        println!("computed_mac: {:?}", computed_mac);
        assert!(&computed_mac[..16] == &pin);
    }

    /** Test that key generation creates valid keys **/
    #[test]
    fn test_gen_pub_is_valid_random() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            // this is a recoding of the is_valid_point_vartime() call
            let encoded = pk.p.to_encoded_point(false);
            let maybe_affine = AffinePoint::from_encoded_point(&encoded);
            assert!(bool::from(maybe_affine.is_some()));
            // assert!(pk.p.is_valid_vartime());
        }
    }

    /** Test that the exchanged key is the same on both sides **/
    #[test]
    fn test_exchange_x_sha256_is_symmetric() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk_a = SecKey::gensk(&mut rng);
            let pk_a = sk_a.genpk();
            let sk_b = SecKey::gensk(&mut rng);
            let pk_b = sk_b.genpk();
            assert_eq!(sk_a.exchange_x_sha256(&pk_b), sk_b.exchange_x_sha256(&pk_a));
        }
    }

    #[test]
    fn test_exchange_x_sha256_bytes_is_symmetric() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk_a = SecKey::gensk(&mut rng);
            let mut pk_bytes_a = [Default::default(); 65];
            sk_a.genpk().to_bytes_uncompressed(&mut pk_bytes_a);

            let sk_b = SecKey::gensk(&mut rng);
            let mut pk_bytes_b = [Default::default(); 65];
            sk_b.genpk().to_bytes_uncompressed(&mut pk_bytes_b);

            let pk_a = PubKey::from_bytes_uncompressed(&pk_bytes_a).unwrap();
            let pk_b = PubKey::from_bytes_uncompressed(&pk_bytes_b).unwrap();
            assert_eq!(sk_a.exchange_x_sha256(&pk_b), sk_b.exchange_x_sha256(&pk_a));
        }
    }

    // TODO: tests with invalid public shares.
}
