use crate::{Block, ParBlocks};
use cipher::{
    consts::{U16, U24, U32, U8},
    generic_array::GenericArray,
    BlockCipher, BlockDecrypt, BlockEncrypt, NewBlockCipher,
};
mod aes128;
use aes128::*;

macro_rules! define_aes_impl {
    (
        $name:ident,
        $key_size:ty,
        $vex_keys:ty,
        $vex_dec_key_schedule:path,
        $vex_enc_key_schedule:path,
        $vex_decrypt:path,
        $vex_encrypt:path,
        $doc:expr
    ) => {
        #[doc=$doc]
        #[derive(Clone)]
        pub struct $name {
            enc_key: $vex_keys,
            dec_key: $vex_keys,
        }

        impl NewBlockCipher for $name {
            type KeySize = $key_size;

            #[inline]
            fn new(key: &GenericArray<u8, $key_size>) -> Self {
                Self {
                    enc_key: $vex_enc_key_schedule(key),
                    dec_key: $vex_dec_key_schedule(key),
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
                let mut blocks = [Block::default(); VEX_BLOCKS];
                blocks[0].copy_from_slice(block);
                $vex_encrypt(&self.enc_key, &mut blocks[0]);
                block.copy_from_slice(&blocks[0]);
            }

            #[inline]
            fn encrypt_par_blocks(&self, blocks: &mut ParBlocks) {
                for chunk in blocks.chunks_mut(VEX_BLOCKS) {
                    $vex_encrypt(&self.enc_key, &mut chunk[0]);
                }
            }
        }

        impl BlockDecrypt for $name {
            #[inline]
            fn decrypt_block(&self, block: &mut Block) {
                let mut blocks = [Block::default(); VEX_BLOCKS];
                blocks[0].copy_from_slice(block);
                $vex_decrypt(&self.dec_key, &mut blocks[0]);
                block.copy_from_slice(&blocks[0]);
            }

            #[inline]
            fn decrypt_par_blocks(&self, blocks: &mut ParBlocks) {
                for chunk in blocks.chunks_mut(VEX_BLOCKS) {
                    $vex_decrypt(&self.dec_key, &mut chunk[0]);
                }
            }
        }

        opaque_debug::implement!($name);
    };
}

define_aes_impl!(
    Aes128,
    U16,
    VexKeys128,
    aes128_dec_key_schedule,
    aes128_enc_key_schedule,
    aes128_vexriscv_decrypt,
    aes128_vexriscv_encrypt,
    "AES-128 block cipher instance"
);

/*
define_aes_impl!(
    Aes256,
    U32,
    vexKeys256,
    vex::aes256_key_schedule,
    vex::aes256_decrypt,
    vex::aes256_encrypt,
    "AES-256 block cipher instance"
);
*/