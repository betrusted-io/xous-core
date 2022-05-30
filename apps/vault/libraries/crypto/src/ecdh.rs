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

use super::rng256::Rng256;
pub const NBYTES: usize = 32;

pub struct SecKey {
    a: EphemeralSecret,
}

#[cfg_attr(feature = "derive_debug", derive(Clone, PartialEq, Debug))]
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
            a: EphemeralSecret::random(&mut OsRng)
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
        shared.as_bytes().as_slice().try_into().unwrap()
    }
}

impl PubKey {
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
    use super::super::rng256::ThreadRng256;
    use super::*;

    // Run more test iterations in release mode, as the code should be faster.
    #[cfg(not(debug_assertions))]
    const ITERATIONS: u32 = 10000;
    #[cfg(debug_assertions)]
    const ITERATIONS: u32 = 500;

    /** Test that key generation creates valid keys **/
    #[test]
    fn test_gen_pub_is_valid_random() {
        let mut rng = ThreadRng256 {};

        for _ in 0..ITERATIONS {
            let sk = SecKey::gensk(&mut rng);
            let pk = sk.genpk();
            assert!(pk.p.is_valid_vartime());
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
