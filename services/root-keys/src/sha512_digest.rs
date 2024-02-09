use core::fmt;

use digest::{
    block_buffer::Eager,
    core_api::{
        AlgorithmName, Block, BlockSizeUser, Buffer, BufferKindUser, OutputSizeUser, TruncSide, UpdateCore,
        VariableOutputCore,
    },
    typenum::{U128, U64},
    FixedOutput, HashMarker, InvalidOutputSize, Output, Update,
};

/// Wrap a pre-hash value in a Digest trait
#[derive(Clone)]
pub struct Sha512Prehash {
    /// track the length of the message processed so far
    hash: Option<[u8; 64]>,
}
impl Sha512Prehash {
    // use this function instead of default for more control over configuration of the hardware engine
    pub fn new() -> Self { Sha512Prehash { hash: None } }

    #[allow(dead_code)] // not used in hosted mode
    pub fn set_prehash(&mut self, hash: [u8; 64]) { self.hash = Some(hash); }
}
impl Default for Sha512Prehash {
    fn default() -> Self { Sha512Prehash::new() }
}

impl HashMarker for Sha512Prehash {}

impl BlockSizeUser for Sha512Prehash {
    type BlockSize = U128;
}

impl BufferKindUser for Sha512Prehash {
    type BufferKind = Eager;
}

impl OutputSizeUser for Sha512Prehash {
    type OutputSize = U64;
}

impl AlgorithmName for Sha512Prehash {
    #[inline]
    fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha512") }
}

impl fmt::Debug for Sha512Prehash {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha512Prehash { ... }") }
}

impl UpdateCore for Sha512Prehash {
    fn update_blocks(&mut self, _blocks: &[Block<Self>]) {
        panic!("Prehash implementation can't take block updates");
    }
}

impl VariableOutputCore for Sha512Prehash {
    const TRUNC_SIDE: TruncSide = TruncSide::Left;

    fn new(_output_size: usize) -> Result<Self, InvalidOutputSize> { Ok(Self { hash: None }) }

    fn finalize_variable_core(&mut self, _buffer: &mut Buffer<Self>, out: &mut Output<Self>) {
        for (dest, &src) in out.chunks_exact_mut(1).zip(self.hash.unwrap().iter()) {
            dest.copy_from_slice(&[src])
        }
    }
}

impl Update for Sha512Prehash {
    /// Update state using the provided data.
    fn update(&mut self, _data: &[u8]) {
        panic!("Prehash implementation can't take block updates");
    }
}
impl FixedOutput for Sha512Prehash {
    /// Consume value and write result into provided array.
    fn finalize_into(self, out: &mut Output<Self>) {
        for (dest, &src) in out.chunks_exact_mut(1).zip(self.hash.unwrap().iter()) {
            dest.copy_from_slice(&[src])
        }
    }
}
