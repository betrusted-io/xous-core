//! Known-answer tests from the SPHINCS+ reference implementation
//! Generated via https://github.com/sphincs/sphincsplus on branch consistent_basew (eccdc43a99e194f52d5ef0e4030ef4dd1e31828b)
//! with PQCgenKAT_sign.c modified on line 59 to reduce iterations from 100 to 10
//!
//! These tests call the `slh_*_internal` functions directly, bypassing context processing.
#![cfg(feature = "alloc")]
use std::{array::from_fn, fmt::Write};

use aes::Aes256;
use cipher::{KeyIvInit, StreamCipher};
use core::{convert::Infallible, error, fmt};
use ctr::Ctr128BE;
use rand_core::{Rng, TryCryptoRng, TryRng, UnwrapErr};
use sha2::Digest;
use signature::Keypair;
use signature::SignatureEncoding;
use slh_dsa::*;
use typenum::Unsigned;

/// AES_CTR_DRBG - based RNG used by the SPHINCS+ reference implementation KATs
struct KatRng(Ctr128BE<Aes256>);

impl KatRng {
    fn new(entropy: &[u8; 48]) -> Self {
        let key = [0u8; 32];
        let mut iv = [0u8; 16];
        iv[15] = 1;
        let mut this = Self(Ctr128BE::<Aes256>::new_from_slices(&key, &iv).unwrap());
        this.update(Some(entropy));
        this
    }

    fn update(&mut self, entropy: Option<&[u8; 48]>) {
        let mut tmp = entropy.map_or([0u8; 48], |e| *e);
        self.0.apply_keystream(&mut tmp);
        self.0 = Ctr128BE::<Aes256>::new_from_slices(&tmp[0..32], &tmp[32..48]).unwrap();
        self.0.apply_keystream(&mut [0; 16]); // discard one block
    }
}

impl TryRng for KatRng {
    type Error = Infallible;

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        dest.fill(0);
        self.0.apply_keystream(dest);
        // Discard up to end of block if not a multiple of 16
        let pad = (16 - (dest.len() % 16)) % 16;
        self.0.apply_keystream(&mut [0; 16][..pad]);
        self.update(None);
        Ok(())
    }

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        rand_core::utils::next_word_via_fill(self)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        rand_core::utils::next_word_via_fill(self)
    }
}

impl TryCryptoRng for KatRng {}

// Mock RNG that just returns a pre-determined bytestring
struct ConstRng(Vec<u8>);

impl TryRng for ConstRng {
    type Error = RandError;
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RandError> {
        let len = dest.len();
        if len > self.0.len() {
            return Err(RandError);
        }

        dest.iter_mut()
            .zip(self.0.drain(..len))
            .for_each(|(d, s)| *d = s);
        Ok(())
    }

    fn try_next_u32(&mut self) -> Result<u32, RandError> {
        let mut buf = [0; 4];
        self.try_fill_bytes(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn try_next_u64(&mut self) -> Result<u64, RandError> {
        let mut buf = [0; 8];
        self.try_fill_bytes(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }
}

/// Not enough bytes error
#[derive(Debug)]
struct RandError;

impl fmt::Display for RandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not enough bytes")
    }
}

impl error::Error for RandError {}

impl TryCryptoRng for ConstRng {}

const ITERS: usize = 10;
fn test_kat<P: ParameterSet + VerifyingKeyLen>(expected: &str)
where
    Signature<P>: SignatureEncoding,
{
    let mut resp: String = "# SPHINCS+\n\n".to_string();
    let mut rng = KatRng::new(&from_fn(|i| i as u8));
    let mut seeds = [[0u8; 48]; ITERS];
    let mut msgs: Vec<Vec<u8>> = (0..ITERS).map(|i| vec![0; 33 * (i + 1)]).collect();

    for (seed, msg) in seeds.iter_mut().zip(msgs.iter_mut()) {
        rng.fill_bytes(seed);
        rng.fill_bytes(msg.as_mut_slice());
    }

    for i in 0..ITERS {
        let mut rng = KatRng::new(&seeds[i]);

        writeln!(&mut resp, "count = {}", i).unwrap();
        writeln!(&mut resp, "seed = {}", hex::encode_upper(seeds[i])).unwrap();

        let mlen = 33 * (i + 1);
        writeln!(resp, "mlen = {}", mlen).unwrap();
        let msg = msgs[i].as_slice();
        writeln!(resp, "msg = {}", hex::encode_upper(msg)).unwrap();

        let mut seed = vec![0; (P::VkLen::USIZE * 3) / 2];
        rng.fill_bytes(&mut seed);
        let seed_rng = ConstRng(seed);

        let sk = SigningKey::<P>::new(&mut UnwrapErr(seed_rng));
        let pk = sk.verifying_key();

        writeln!(resp, "pk = {}", hex::encode_upper(pk.to_bytes())).unwrap();
        writeln!(resp, "sk = {}", hex::encode_upper(sk.to_bytes())).unwrap();

        let mut opt_rand = vec![0; P::VkLen::USIZE / 2];
        rng.fill_bytes(opt_rand.as_mut());

        let sig = sk.slh_sign_internal(&[msg], Some(&opt_rand)).to_bytes();
        writeln!(resp, "smlen = {}", sig.as_slice().len() + msg.len()).unwrap();
        writeln!(
            resp,
            "sm = {}{}\n",
            hex::encode_upper(&sig),
            hex::encode_upper(msg)
        )
        .unwrap();
    }

    let shasum = sha2::Sha256::digest(resp.as_bytes());
    assert_eq!(hex::encode(shasum.as_slice()), expected);
}

/*
KATs generated via https://github.com/sphincs/sphincsplus on branch consistent_basew (eccdc43a99e194f52d5ef0e4030ef4dd1e31828b)
with PQCgenKAT_sign.c modified on line 59 to reduce iterations from 100 to 10

056ee235f9a8ff3fb3f3799807d82690ffe3ff5681f0c6e3809ddca4fb539b29 sphincs-shake-128f-simple
0d17bdb1d3e7d7f06e88d79f3b65fbeae70694914ec088a3d1c081e65c35a928 sphincs-sha2-192s-simple
1930c23fb13b4f95ec11343d9a68d270879bfbb8821c1cfb59b37cd0b5e4665e sphincs-shake-128s-simple
5e501f7c91fa189dff618dda9cca0511140fb85e133bab986c9ef89ed220389e sphincs-sha2-128s-simple
73519c7365ac46695ea96dae3283f05a2ddcb1e8f9e0ba800544ba737b746206 sphincs-sha2-192f-simple
7c8f16c7b9645df58518b1c0aa7a26f7a2e1b9ee860819f25305cf97aecce1f3 sphincs-shake-256s-simple
7d35e89f91116f0869d7a7591df026c4033f8ca3e33d795f03b25905277175cd sphincs-shake-192s-simple
be37b5222c98b3a1f0d2d3d69bc32205ed17e93c6a4da684c76ee1ca29ec28ef sphincs-shake-256f-simple
c8fcb441611200b19349f2e7fda07c4547bef22f35af6e47a2b8c824e0c2e0be sphincs-sha2-128f-simple
d83825fb99bc22a3eb4ae388a9e88716b5cae0622682a210bd11f7b8ffbd47ee sphincs-shake-192f-simple
e58442029ff40d3f61d5a7d3495fae38500ad4be4db9db1ef4f42a365077b070 sphincs-sha2-256f-simple
f935d6af17fa16f290421c5112c4c2cee445ba7c332a74fe3d88a7a219c2176b sphincs-sha2-256s-simple
*/

#[test]
fn test_kat_sha2_128f() {
    test_kat::<Sha2_128f>("c8fcb441611200b19349f2e7fda07c4547bef22f35af6e47a2b8c824e0c2e0be");
}

#[test]
fn test_kat_sha2_192s() {
    test_kat::<Sha2_192s>("0d17bdb1d3e7d7f06e88d79f3b65fbeae70694914ec088a3d1c081e65c35a928");
}

#[test]
fn test_kat_sha2_192f() {
    test_kat::<Sha2_192f>("73519c7365ac46695ea96dae3283f05a2ddcb1e8f9e0ba800544ba737b746206");
}

#[test]
fn test_kat_sha2_256s() {
    test_kat::<Sha2_256s>("f935d6af17fa16f290421c5112c4c2cee445ba7c332a74fe3d88a7a219c2176b");
}

#[test]
fn test_kat_sha2_256f() {
    test_kat::<Sha2_256f>("e58442029ff40d3f61d5a7d3495fae38500ad4be4db9db1ef4f42a365077b070");
}

#[test]
fn test_kat_shake_128s() {
    test_kat::<Shake128s>("1930c23fb13b4f95ec11343d9a68d270879bfbb8821c1cfb59b37cd0b5e4665e");
}

#[test]
fn test_kat_shake_128f() {
    test_kat::<Shake128f>("056ee235f9a8ff3fb3f3799807d82690ffe3ff5681f0c6e3809ddca4fb539b29");
}

#[test]
fn test_kat_shake_192s() {
    test_kat::<Shake192s>("7d35e89f91116f0869d7a7591df026c4033f8ca3e33d795f03b25905277175cd");
}

#[test]
fn test_kat_shake_192f() {
    test_kat::<Shake192f>("d83825fb99bc22a3eb4ae388a9e88716b5cae0622682a210bd11f7b8ffbd47ee");
}

#[test]
fn test_kat_shake_256s() {
    test_kat::<Shake256s>("7c8f16c7b9645df58518b1c0aa7a26f7a2e1b9ee860819f25305cf97aecce1f3");
}

#[test]
fn test_kat_shake_256f() {
    test_kat::<Shake256f>("be37b5222c98b3a1f0d2d3d69bc32205ed17e93c6a4da684c76ee1ca29ec28ef");
}
