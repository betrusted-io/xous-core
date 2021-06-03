#![cfg_attr(target_os = "none", no_std)]

/*
  Soft AES implementations vendored in from https://github.com/RustCrypto/block-ciphers.git
  Commit ref d5aac29e88c45f5fc8bec0ba2bbbbbcd590a0a82, aes v0.7.4
  License is MIT/Apache 2.0.

  Soft AES is mainly here for validation/benchmarking comparison against the Vex-accelerated primitives
  and to help with API development. Core crypto primitives are vendored in so that the code is explicitly
  managed within Xous and not pulled in as a dependency that can be changed/poisoned on the fly. It also
  eliminates another foreign build.rs script that runs on the local build machine.
*/

mod soft;
mod vex;

pub use soft::{Aes128Soft, Aes192, Aes256};
pub use vex::Aes128;

#[cfg(feature = "ctr")]
pub use soft::{Aes128Ctr, Aes192Ctr, Aes256Ctr};

pub use cipher::{self, BlockCipher, BlockDecrypt, BlockEncrypt, NewBlockCipher};

/// 128-bit AES block
pub type Block = cipher::generic_array::GenericArray<u8, cipher::consts::U16>;

/// 8 x 128-bit AES blocks to be processed in parallel
pub type ParBlocks = cipher::generic_array::GenericArray<Block, cipher::consts::U8>;

/// Size of an AES block (128-bits; 16-bytes)
pub const BLOCK_SIZE: usize = 16;
