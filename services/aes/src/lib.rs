#![no_std]

/*
  Soft AES implementations vendored in from https://github.com/RustCrypto/block-ciphers.git
  Commit ref 7de364ede310f1ea7080c99ddf1138aeb47f9a69 0.8.1
  License is MIT/Apache 2.0.

  Soft AES is mainly here for validation/benchmarking comparison against the Vex-accelerated primitives
  and to help with API development. Core crypto primitives are vendored in so that the code is explicitly
  managed within Xous and not pulled in as a dependency that can be changed/poisoned on the fly. It also
  eliminates another foreign build.rs script that runs on the local build machine.
*/

mod soft;
pub use cipher;
use cipher::{
    consts::{U8, U16},
    generic_array::GenericArray,
};
pub use soft::{Aes128Soft, Aes192, Aes256Soft};

/// 128-bit AES block
pub type Block = GenericArray<u8, U16>;
/// Eight 128-bit AES blocks
pub type Block8 = GenericArray<Block, U8>;

// vex patches
#[cfg(all(target_arch = "riscv32", not(feature = "cramium-soc"), not(feature = "vexii-test")))]
mod vex;
// Note that we can't use 'feature' flags (for precursor, renode, hosted) because the AES
// library is patched into functions that are oblivious to these features.
// so this library has to fall back on the legacy method of determining which build target
// is being specified.
#[cfg(not(any(all(target_arch = "riscv32", not(feature = "cramium-soc")), feature = "vexii-test")))]
pub use soft::Aes128Soft as Aes128;
#[cfg(not(any(all(target_arch = "riscv32", not(feature = "cramium-soc")), feature = "vexii-test")))]
pub use soft::Aes256Soft as Aes256;
#[cfg(all(
    all(target_arch = "riscv32", not(feature = "cramium-soc"), not(feature = "vexii-test")),
    any(target_os = "none", target_os = "xous"),
))]
pub use vex::{Aes128, Aes256};

#[cfg(feature = "vexii-test")]
mod zkn;
#[cfg(feature = "vexii-test")]
pub use soft::Aes128Soft as Aes128;
#[cfg(feature = "vexii-test")]
pub use zkn::Aes256;

/// Size of an AES block (128-bits; 16-bytes)
pub const BLOCK_SIZE: usize = 16;
