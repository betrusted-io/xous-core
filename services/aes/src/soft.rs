//! AES block cipher constant-time implementation.
//!
//! The implementation uses a technique called [fixslicing][1], an improved
//! form of bitslicing which represents ciphers in a way which enables
//! very efficient constant-time implementations in software.
//!
//! [1]: https://eprint.iacr.org/2020/1123.pdf

#![deny(unsafe_code)]

pub(crate) mod fixslice32;
use fixslice32 as fixslice;

#[cfg(feature = "ctr")]
mod ctr;

#[cfg(feature = "ctr")]
pub use self::ctr::{Aes128Ctr, Aes192Ctr, Aes256Ctr};

use crate::{Block, ParBlocks};
use cipher::{
    consts::{U16, U24, U32, U8},
    generic_array::GenericArray,
    BlockCipher, BlockDecrypt, BlockEncrypt, NewBlockCipher,
};
use fixslice::{FixsliceKeys128, FixsliceKeys192, FixsliceKeys256, FIXSLICE_BLOCKS};

macro_rules! define_aes_impl {
    (
        $name:ident,
        $key_size:ty,
        $key_bits:expr,
        $fixslice_keys:ty,
        $fixslice_key_schedule:path,
        $fixslice_decrypt:path,
        $fixslice_encrypt:path,
        $doc:expr
    ) => {
        #[doc=$doc]
        #[derive(Clone)]
        pub struct $name {
            keys: $fixslice_keys,
        }
        impl $name {
            pub fn key_size(&self) -> usize {
                $key_bits as usize
            }
        }

        impl NewBlockCipher for $name {
            type KeySize = $key_size;

            #[inline]
            fn new(key: &GenericArray<u8, $key_size>) -> Self {
                Self {
                    keys: $fixslice_key_schedule(key),
                }
            }
        }

        impl BlockCipher for $name {
            type BlockSize = U16;
            type ParBlocks = U8;
        }

        impl BlockEncrypt for $name {
            #[inline]
            fn encrypt_block(&self, block: &mut Block) {
                let mut blocks = [Block::default(); FIXSLICE_BLOCKS];
                blocks[0].copy_from_slice(block);
                $fixslice_encrypt(&self.keys, &mut blocks);
                block.copy_from_slice(&blocks[0]);
            }

            #[inline]
            fn encrypt_par_blocks(&self, blocks: &mut ParBlocks) {
                for chunk in blocks.chunks_mut(FIXSLICE_BLOCKS) {
                    $fixslice_encrypt(&self.keys, chunk);
                }
            }
        }

        impl BlockDecrypt for $name {
            #[inline]
            fn decrypt_block(&self, block: &mut Block) {
                let mut blocks = [Block::default(); FIXSLICE_BLOCKS];
                blocks[0].copy_from_slice(block);
                $fixslice_decrypt(&self.keys, &mut blocks);
                block.copy_from_slice(&blocks[0]);
            }

            #[inline]
            fn decrypt_par_blocks(&self, blocks: &mut ParBlocks) {
                for chunk in blocks.chunks_mut(FIXSLICE_BLOCKS) {
                    $fixslice_decrypt(&self.keys, chunk);
                }
            }
        }

        opaque_debug::implement!($name);
    };
}
define_aes_impl!(
    Aes128Soft,
    U16,
    128,
    FixsliceKeys128,
    fixslice::aes128_key_schedule,
    fixslice::aes128_decrypt,
    fixslice::aes128_encrypt,
    "AES-128 block cipher instance"
);

define_aes_impl!(
    Aes192,
    U24,
    192,
    FixsliceKeys192,
    fixslice::aes192_key_schedule,
    fixslice::aes192_decrypt,
    fixslice::aes192_encrypt,
    "AES-192 block cipher instance"
);

define_aes_impl!(
    Aes256Soft,
    U32,
    256,
    FixsliceKeys256,
    fixslice::aes256_key_schedule,
    fixslice::aes256_decrypt,
    fixslice::aes256_encrypt,
    "AES-256 block cipher instance"
);
