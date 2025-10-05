use core::{fmt, slice::from_ref};

use digest::{
    HashMarker, InvalidOutputSize, Output,
    block_buffer::Eager,
    core_api::{
        AlgorithmName, Block, BlockSizeUser, Buffer, BufferKindUser, OutputSizeUser, TruncSide, UpdateCore,
        VariableOutputCore,
    },
    typenum::{U32, U64, U128, Unsigned},
};
use utralib::*;

use crate::sha256::compress256;
use crate::sha512::compress512;

/// Core block-level SHA-256 hasher with variable output size.
///
/// Supports initialization only for 32 byte output sizes,
/// i.e. 256 bits version
#[derive(Clone)]
pub struct Sha256VarCore {
    block_len: u64,
    csr: CSR<u32>,
    state: &'static [u32],
}

impl HashMarker for Sha256VarCore {}

impl BlockSizeUser for Sha256VarCore {
    type BlockSize = U64;
}

impl BufferKindUser for Sha256VarCore {
    type BufferKind = Eager;
}

impl UpdateCore for Sha256VarCore {
    #[inline]
    fn update_blocks(&mut self, blocks: &[Block<Self>]) {
        self.block_len += blocks.len() as u64;
        compress256(&mut self.csr, blocks);
    }
}

impl OutputSizeUser for Sha256VarCore {
    type OutputSize = U32;
}

impl VariableOutputCore for Sha256VarCore {
    const TRUNC_SIDE: TruncSide = TruncSide::Left;

    #[inline]
    fn new(output_size: usize) -> Result<Self, InvalidOutputSize> {
        assert!(output_size == 32, "Only SHA-256/256 is supported");
        let mut csr = CSR::new(utra::combohash::HW_COMBOHASH_BASE as *mut u32);
        csr.wfo(utra::combohash::SFR_OPT2_CR_OPT_IFSTART, 1); // this clears hash state
        let block_len = 0;
        // safety: all types representable, bounds are within the length of the specified hardware region
        // but ONLY safe in single-threaded, machine-mode environments!
        let state = unsafe {
            core::slice::from_raw_parts(
                utralib::HW_SEG_HOUT_MEM as *const u32,
                <crate::Sha256 as BlockSizeUser>::BlockSize::USIZE / size_of::<u32>(),
            )
        };
        Ok(Self { csr, block_len, state })
    }

    #[inline]
    fn finalize_variable_core(&mut self, buffer: &mut Buffer<Self>, out: &mut Output<Self>) {
        let bs = Self::BlockSize::U64;
        let bit_len = 8 * (buffer.get_pos() as u64 + bs * self.block_len);
        buffer.len64_padding_be(bit_len, |b| compress256(&mut self.csr, from_ref(b)));

        for (chunk, v) in out.chunks_exact_mut(4).zip(self.state.iter()) {
            chunk.copy_from_slice(&v.to_le_bytes());
        }
    }
}

impl AlgorithmName for Sha256VarCore {
    #[inline]
    fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha256") }
}

impl fmt::Debug for Sha256VarCore {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha256VarCore { ... }") }
}

/// Core block-level SHA-512 hasher with variable output size.
///
/// Supports initialization only for 64 byte output sizes,
/// i.e. 512 bits version.
#[derive(Clone)]
pub struct Sha512VarCore {
    block_len: u128,
    csr: CSR<u32>,
    state: &'static [u64],
}

impl HashMarker for Sha512VarCore {}

impl BlockSizeUser for Sha512VarCore {
    type BlockSize = U128;
}

impl BufferKindUser for Sha512VarCore {
    type BufferKind = Eager;
}

impl UpdateCore for Sha512VarCore {
    #[inline]
    fn update_blocks(&mut self, blocks: &[Block<Self>]) {
        self.block_len += blocks.len() as u128;
        compress512(&mut self.csr, blocks);
    }
}

impl OutputSizeUser for Sha512VarCore {
    type OutputSize = U64;
}

impl VariableOutputCore for Sha512VarCore {
    const TRUNC_SIDE: TruncSide = TruncSide::Left;

    #[inline]
    fn new(output_size: usize) -> Result<Self, InvalidOutputSize> {
        assert!(output_size == 64, "Only SHA-512/512 is supported");
        let mut csr = CSR::new(utra::combohash::HW_COMBOHASH_BASE as *mut u32);
        csr.wfo(utra::combohash::SFR_OPT2_CR_OPT_IFSTART, 1); // this clears hash state
        let block_len = 0;
        // safety: all types representable, bounds are within the length of the specified hardware region
        // but ONLY safe in single-threaded, machine-mode environments!
        let state = unsafe {
            core::slice::from_raw_parts(
                utralib::HW_SEG_HOUT_MEM as *const u64,
                <crate::Sha512 as BlockSizeUser>::BlockSize::USIZE / size_of::<u64>(),
            )
        };
        Ok(Self { csr, block_len, state })
    }

    #[inline]
    fn finalize_variable_core(&mut self, buffer: &mut Buffer<Self>, out: &mut Output<Self>) {
        let bs = Self::BlockSize::U64 as u128;
        let bit_len = 8 * (buffer.get_pos() as u128 + bs * self.block_len);
        buffer.len128_padding_be(bit_len, |b| compress512(&mut self.csr, from_ref(b)));

        for (chunk, v) in out.chunks_exact_mut(8).zip(self.state.iter()) {
            chunk.copy_from_slice(&v.to_le_bytes());
        }
    }
}

impl AlgorithmName for Sha512VarCore {
    #[inline]
    fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha512") }
}

impl fmt::Debug for Sha512VarCore {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Sha512VarCore { ... }") }
}
