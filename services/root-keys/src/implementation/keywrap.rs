#![forbid(unsafe_code)]

use aes::Aes256;
/// this code is vendored in from https://github.com/jedisct1/rust-aes-keywrap
/// From its original README:
/// AES Key Wrap for Rust
///
/// AES Key Wrap is a construction to encrypt secret keys using a master key.
///
/// This is an AES-KWP (NIST SP800-38F) implementation for Rust.
///
/// It is essentially a 5 round Feistel network using AES as the core function.
/// One half of each AES block is used to encrypt the key, and the second half
/// of the last permutation is used to compute a 64-bit MAC.
///
/// It doesn't require nonces, but still allows key reuse.
///
/// This is a NIST-blessed construction. Other than that, AES Key Wrap is inefficient
/// and is generally not very useful.

/// Turns out that this implementation does not seem to work. It does not generate
/// results that match the NIST test vectors at
/// https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/mac/kwtestvectors.zip
///
/// The `aes_kw` functions in the RustCrypto library do create matching results.
/// However, back when the PDDB was written, this crate didn't exist (the first commit
/// to `aes_kw` was Jan 2022, the PDDB was started back in October 2021, and around
/// the time I searched for an aes-kw implementation, the `aes_kw` code was just pushed
/// at 0.1.0, and not registering in Google).
///
/// Ah well. Better we caught it now than later! Unfortunately, this vendored-in
/// implementation has to stick around for a while, because we need it to do migrations
/// from the wrong version to the working version. I don't think this implementation
/// may necessarily be insecure -- it seems /very close/ to the spec, maybe just a
/// problem with how the block counter is incremented between rounds -- but it is still
/// not compliant, so, we should migrate away from it!
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use byteorder::{BigEndian, ByteOrder};

use crate::api::KeywrapError;

const FEISTEL_ROUNDS: usize = 5;

#[derive(Debug)]
pub struct Aes256KeyWrap {
    aes: Aes256,
}

impl Aes256KeyWrap {
    pub const KEY_BYTES: usize = 32;
    pub const MAC_BYTES: usize = 8;

    pub fn new(key: &[u8; Self::KEY_BYTES]) -> Self { Aes256KeyWrap { aes: Aes256::new(key.into()) } }

    #[allow(dead_code)]
    pub fn encapsulate(&self, input: &[u8]) -> Result<Vec<u8>, KeywrapError> {
        if input.len() > std::u32::MAX as usize || input.len() as u64 >= std::u64::MAX / FEISTEL_ROUNDS as u64
        {
            return Err(KeywrapError::InvalidDataSize);
        }
        let mut aiv: [u8; 8] = [0xa6u8, 0x59, 0x59, 0xa6, 0, 0, 0, 0];
        BigEndian::write_u32(&mut aiv[4..8], input.len() as u32);
        let mut block = [0u8; 16];
        let mut block = GenericArray::from_mut_slice(&mut block);
        block[0..8].copy_from_slice(&aiv);

        if input.len() == 8 {
            block[8..16].copy_from_slice(input);
            self.aes.encrypt_block(&mut block);
            return Ok(block.to_vec());
        }

        let mut counter = 0u64;
        let mut counter_bin = [0u8; 8];
        let mut output = vec![0u8; ((input.len() + 7) & !7) + Self::MAC_BYTES];
        output[8..][..input.len()].copy_from_slice(input);
        for _ in 0..FEISTEL_ROUNDS {
            let mut i = 8;
            while i <= (input.len() + 7) & !7 {
                block[8..16].copy_from_slice(&output[i..][0..8]);
                self.aes.encrypt_block(&mut block);
                counter += 1;
                BigEndian::write_u64(&mut counter_bin, counter);
                block[8..16].iter_mut().zip(counter_bin.iter()).for_each(|(a, b)| *a ^= b);
                output[i..i + 8].copy_from_slice(&block[8..16]);
                i += 8;
            }
        }
        output[0..8].copy_from_slice(&block[0..8]);
        Ok(output)
    }

    pub fn decapsulate(&self, input: &[u8], expected_len: usize) -> Result<Vec<u8>, KeywrapError> {
        if input.len() % 8 != 0 {
            return Err(KeywrapError::InvalidDataSize);
        }
        let output_len = input.len().checked_sub(Self::MAC_BYTES).ok_or(KeywrapError::InvalidOutputSize)?;
        if output_len > std::u32::MAX as usize || output_len as u64 >= std::u64::MAX / FEISTEL_ROUNDS as u64 {
            return Err(KeywrapError::InvalidDataSize);
        }
        if expected_len > output_len || (expected_len & !7) > output_len {
            return Err(KeywrapError::InvalidDataSize);
        }
        let mut output = vec![0u8; output_len];
        let mut aiv: [u8; 8] = [0xa6u8, 0x59, 0x59, 0xa6, 0, 0, 0, 0];
        BigEndian::write_u32(&mut aiv[4..8], expected_len as u32);

        let mut block = [0u8; 16];
        let mut block = GenericArray::from_mut_slice(&mut block);

        if output.len() == 8 {
            block.copy_from_slice(input);
            self.aes.decrypt_block(&mut block);
            let c = block[0..8].iter().zip(aiv.iter()).fold(0, |acc, (a, b)| acc | (a ^ b));
            if c != 0 {
                return Err(KeywrapError::IntegrityCheckFailed);
            }
            output[0..8].copy_from_slice(&block[8..16]);
            return Ok(output);
        }

        output.copy_from_slice(&input[8..]);
        block[0..8].copy_from_slice(&input[0..8]);
        let mut counter = (FEISTEL_ROUNDS * output.len() / 8) as u64;
        let mut counter_bin = [0u8; 8];
        for _ in 0..FEISTEL_ROUNDS {
            let mut i = output.len();
            while i >= 8 {
                i -= 8;
                block[8..16].copy_from_slice(&output[i..][0..8]);
                BigEndian::write_u64(&mut counter_bin, counter);
                counter -= 1;
                block[8..16].iter_mut().zip(counter_bin.iter()).for_each(|(a, b)| *a ^= b);
                self.aes.decrypt_block(&mut block);
                output[i..][0..8].copy_from_slice(&block[8..16]);
            }
        }
        let c = block[0..8].iter().zip(aiv.iter()).fold(0, |acc, (a, b)| acc | (a ^ b));
        if c != 0 {
            return Err(KeywrapError::IntegrityCheckFailed);
        }
        Ok(output)
    }
}

#[test]
fn aligned() {
    let secret = b"1234567812345678";
    let key = [42u8; 32];
    let kw = Aes256KeyWrap::new(&key);
    let wrapped = kw.encapsulate(secret).unwrap();
    let unwrapped = kw.decapsulate(&wrapped, secret.len()).unwrap();
    assert_eq!(secret, unwrapped.as_slice());
}

#[test]
fn not_aligned() {
    let secret = b"1234567812345";
    let key = [42u8; 32];
    let kw = Aes256KeyWrap::new(&key);
    let wrapped = kw.encapsulate(secret).unwrap();
    let unwrapped = kw.decapsulate(&wrapped, secret.len()).unwrap();
    assert_eq!(secret, &unwrapped.as_slice()[..secret.len()]);
}

#[test]
fn singleblock() {
    let secret = b"12345678";
    let key = [42u8; 32];
    let kw = Aes256KeyWrap::new(&key);
    let wrapped = kw.encapsulate(secret).unwrap();
    let unwrapped = kw.decapsulate(&wrapped, secret.len()).unwrap();
    assert_eq!(secret, unwrapped.as_slice());
}
