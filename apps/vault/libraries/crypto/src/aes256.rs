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

use super::util::Block16;
use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};

/** This structure caches the round keys, to avoid re-computing the key schedule for each block. **/
pub struct EncryptionKey {
    enc_cipher: Aes256,
    // have to keep a copy of this around because the API wants to create a DecryptionKey from this,
    // but the Aes256 key doesn't support a clone option.
    key: [u8; 32],
}

pub struct DecryptionKey {
    dec_cipher: Aes256,
}

impl EncryptionKey {
    // Computes the round keys.
    pub fn new(key: &[u8; 32]) -> EncryptionKey {
        let mut local_key = [0u8; 32];
        local_key.copy_from_slice(key);
        let enc_cipher = Aes256::new(GenericArray::from_slice(key));
        EncryptionKey {
            enc_cipher,
            key: local_key,
        }
    }

    // Encrypt an AES block in place.
    pub fn encrypt_block(&self, block: &mut Block16) {
        self.enc_cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
}

impl DecryptionKey {
    // Computes the round keys.
    pub fn new(key: &EncryptionKey) -> DecryptionKey {
        let dec_cipher = Aes256::new(GenericArray::from_slice(&key.key));

        DecryptionKey { dec_cipher }
    }

    // Decrypt an AES block in place.
    pub fn decrypt_block(&self, block: &mut Block16) {
        self.dec_cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Test vector from the NIST obtained at:
    // https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Standards-and-Guidelines/documents/examples/AES_ECB.pdf
    #[test]
    fn test_nist_aes256_ecb_encrypt() {
        let src = b"\x6b\xc1\xbe\xe2\x2e\x40\x9f\x96\
                    \xe9\x3d\x7e\x11\x73\x93\x17\x2a";
        let key = b"\x60\x3d\xeb\x10\x15\xca\x71\xbe\
                    \x2b\x73\xae\xf0\x85\x7d\x77\x81\
                    \x1f\x35\x2c\x07\x3b\x61\x08\xd7\
                    \x2d\x98\x10\xa3\x09\x14\xdf\xf4";
        let expected = b"\xf3\xee\xd1\xbd\xb5\xd2\xa0\x3c\
                         \x06\x4b\x5a\x7e\x3d\xb1\x81\xf8";

        let mut dst: Block16 = Default::default();
        dst.copy_from_slice(src);
        EncryptionKey::new(key).encrypt_block(&mut dst);
        assert_eq!(&dst, expected);
    }

    #[test]
    fn test_nist_aes256_ecb_decrypt() {
        let src = b"\xf3\xee\xd1\xbd\xb5\xd2\xa0\x3c\
                    \x06\x4b\x5a\x7e\x3d\xb1\x81\xf8";
        let key = b"\x60\x3d\xeb\x10\x15\xca\x71\xbe\
                    \x2b\x73\xae\xf0\x85\x7d\x77\x81\
                    \x1f\x35\x2c\x07\x3b\x61\x08\xd7\
                    \x2d\x98\x10\xa3\x09\x14\xdf\xf4";
        let expected = b"\x6b\xc1\xbe\xe2\x2e\x40\x9f\x96\
                         \xe9\x3d\x7e\x11\x73\x93\x17\x2a";

        let mut dst: Block16 = Default::default();
        dst.copy_from_slice(src);
        DecryptionKey::new(&EncryptionKey::new(key)).decrypt_block(&mut dst);
        assert_eq!(&dst, expected);
    }

    #[test]
    fn test_encrypt_decrypt() {
        // Test that decrypt_block is the inverse of encrypt_block for a bunch of block values.
        let key_bytes = b"\x60\x3d\xeb\x10\x15\xca\x71\xbe\
                          \x2b\x73\xae\xf0\x85\x7d\x77\x81\
                          \x1f\x35\x2c\x07\x3b\x61\x08\xd7\
                          \x2d\x98\x10\xa3\x09\x14\xdf\xf4";
        let enc_key = EncryptionKey::new(key_bytes);
        let dec_key = DecryptionKey::new(&enc_key);
        let mut block: Block16 = [0; 16];
        for i in 0..=255 {
            for j in 0..16 {
                block[j] = (i + j) as u8;
            }
            let expected = block;
            enc_key.encrypt_block(&mut block);
            dec_key.decrypt_block(&mut block);
            assert_eq!(block, expected);
        }
    }

    #[test]
    fn test_sbox_is_permutation() {
        let mut image = [false; 256];
        for &sboxed in SBOX.iter() {
            assert_eq!(image[sboxed as usize], false);
            image[sboxed as usize] = true;
        }
    }

    #[test]
    fn test_sbox_inv_is_permutation() {
        let mut image = [false; 256];
        for &sboxed in SBOX_INV.iter() {
            assert_eq!(image[sboxed as usize], false);
            image[sboxed as usize] = true;
        }
    }

    #[test]
    fn test_sbox_inverse() {
        for i in 0..=255 {
            assert_eq!(SBOX_INV[SBOX[i as usize] as usize], i);
        }
    }

    #[test]
    fn test_subbytes() {
        let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let expected = [
            99, 124, 119, 123, 242, 107, 111, 197, 48, 1, 103, 43, 254, 215, 171, 118,
        ];
        sub_bytes(&mut block);
        assert_eq!(block, expected);
    }

    #[test]
    fn test_subbytes_inv() {
        // Test that inv_sub_bytes is the inverse of sub_bytes for a bunch of block values.
        let mut block: Block16 = [0; 16];
        for i in 0..=255 {
            for j in 0..16 {
                block[j] = (i + j) as u8;
            }
            let expected = block;
            sub_bytes(&mut block);
            inv_sub_bytes(&mut block);
            assert_eq!(block, expected);
        }
    }

    #[test]
    fn test_shift_rows() {
        let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let expected = [0, 5, 10, 15, 4, 9, 14, 3, 8, 13, 2, 7, 12, 1, 6, 11];
        shift_rows(&mut block);
        assert_eq!(block, expected);
    }

    #[test]
    fn test_shift_rows_inv() {
        // Test that inv_shift_rows is the inverse of shift_rows for a bunch of block values.
        let mut block: Block16 = [0; 16];
        for i in 0..=255 {
            for j in 0..16 {
                block[j] = (i + j) as u8;
            }
            let expected = block;
            shift_rows(&mut block);
            inv_shift_rows(&mut block);
            assert_eq!(block, expected);
        }
    }

    #[test]
    fn test_mix_columns_inv() {
        // Test that inv_mix_columns is the inverse of mix_columns for a bunch of block values.
        let mut block: Block16 = [0; 16];
        for i in 0..=255 {
            for j in 0..16 {
                block[j] = (i + j) as u8;
            }
            let expected = block;
            mix_columns(&mut block);
            inv_mix_columns(&mut block);
            assert_eq!(block, expected);
        }
    }

    /** Comparison with AES-NI instructions for CPUs that support them **/
    #[cfg(all(target_arch = "x86_64", target_feature = "aes"))]
    mod aesni {
        use super::super::*;

        fn aes_enc_ni(block: &mut Block16, rkey: &Block16) {
            use core::arch::x86_64::{__m128i, _mm_aesenc_si128};

            unsafe {
                let block_mm: __m128i = core::mem::transmute(*block);
                let rkey_mm: __m128i = core::mem::transmute(*rkey);
                let encrypted_mm: __m128i = _mm_aesenc_si128(block_mm, rkey_mm);
                *block = core::mem::transmute(encrypted_mm)
            }
        }

        fn aes_enc_last_ni(block: &mut Block16, rkey: &Block16) {
            use core::arch::x86_64::{__m128i, _mm_aesenclast_si128};

            unsafe {
                let block_mm: __m128i = core::mem::transmute(*block);
                let rkey_mm: __m128i = core::mem::transmute(*rkey);
                let encrypted_mm: __m128i = _mm_aesenclast_si128(block_mm, rkey_mm);
                *block = core::mem::transmute(encrypted_mm)
            }
        }

        fn aes_dec_ni(block: &mut Block16, rkey: &Block16) {
            use core::arch::x86_64::{__m128i, _mm_aesdec_si128};

            unsafe {
                let block_mm: __m128i = core::mem::transmute(*block);
                let rkey_mm: __m128i = core::mem::transmute(*rkey);
                let decrypted_mm: __m128i = _mm_aesdec_si128(block_mm, rkey_mm);
                *block = core::mem::transmute(decrypted_mm)
            }
        }

        fn aes_dec_last_ni(block: &mut Block16, rkey: &Block16) {
            use core::arch::x86_64::{__m128i, _mm_aesdeclast_si128};

            unsafe {
                let block_mm: __m128i = core::mem::transmute(*block);
                let rkey_mm: __m128i = core::mem::transmute(*rkey);
                let decrypted_mm: __m128i = _mm_aesdeclast_si128(block_mm, rkey_mm);
                *block = core::mem::transmute(decrypted_mm)
            }
        }

        #[test]
        fn test_aes_enc_ni() {
            let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let mut block_ni = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let rkey = [
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            ];
            aes_enc(&mut block, &rkey);
            aes_enc_ni(&mut block_ni, &rkey);
            assert_eq!(block, block_ni);
        }

        #[test]
        fn test_aes_enc_last_ni() {
            let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let mut block_ni = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let rkey = [
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            ];
            aes_enc_last(&mut block, &rkey);
            aes_enc_last_ni(&mut block_ni, &rkey);
            assert_eq!(block, block_ni);
        }

        #[test]
        fn test_aes_dec_ni() {
            let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let mut block_ni = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let rkey = [
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            ];
            aes_dec(&mut block, &rkey);
            aes_dec_ni(&mut block_ni, &rkey);
            assert_eq!(block, block_ni);
        }

        #[test]
        fn test_aes_dec_last_ni() {
            let mut block = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let mut block_ni = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let rkey = [
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            ];
            aes_dec_last(&mut block, &rkey);
            aes_dec_last_ni(&mut block_ni, &rkey);
            assert_eq!(block, block_ni);
        }
    }
}
