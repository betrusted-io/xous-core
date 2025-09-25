use bao1x_hal::sce::combohash::*;
use digest::{
    core_api::BlockSizeUser,
    generic_array::GenericArray,
    typenum::{U64, Unsigned},
};
use utralib::*;
// save some typing
const BLOCK_LEN: usize = <crate::Sha256 as BlockSizeUser>::BlockSize::USIZE;

/// Raw SHA-256 compression function.
///
/// This is a low-level "hazmat" API which provides direct access to the core
/// functionality of SHA-256.
#[cfg_attr(docsrs, doc(cfg(feature = "compress")))]
pub fn compress256(csr: &mut CSR<u32>, blocks: &[GenericArray<u8, U64>]) {
    // SAFETY: GenericArray<u8, U64> and [u8; 64] have
    // exactly the same memory layout
    let p = blocks.as_ptr() as *const [u8; 64];
    let blocks = unsafe { core::slice::from_raw_parts(p, blocks.len()) };
    compress(csr, blocks)
}

/// Invariants:
///   - every entry into compress() assumes that the msg buffer is empty.
///   - every exit from compress() thus must call the hasher to process all the blocks queued in the msg
///     buffer.
fn compress(csr: &mut CSR<u32>, blocks: &[[u8; BLOCK_LEN]]) {
    const BUF_BLOCKS: usize = utralib::HW_SEG_MSG_MEM_LEN / BLOCK_LEN;
    // safety: this is the actual location of the message buffer and its length according to the hardware
    // spec. only safe because this is machine mode and no-std (no threads, no concurrency)
    #[cfg(feature = "debug")]
    let msg_blocks: &mut [[u8; BLOCK_LEN]; BUF_BLOCKS] =
        unsafe { &mut *(utralib::HW_SEG_MSG_MEM as *mut [[u8; BLOCK_LEN]; BUF_BLOCKS]) };

    for block_chunk in blocks.chunks(BUF_BLOCKS) {
        // block_chunk has a length equal to or less than msg_buf due to .chunks() iterator above
        #[cfg(feature = "debug")]
        for (src, dst) in block_chunk.iter().zip(msg_blocks.iter_mut()) {
            crate::println!("  {:x?}", src);
            dst.copy_from_slice(src);
        }

        // process buffer contents every time we fill up msg_blocks

        // set msg input pointer offset
        csr.wo(utra::combohash::SFR_SEGPTR_SEGID_MSG, 0);
        // set output pointer offset
        csr.wo(utra::combohash::SFR_SEGPTR_SEGID_HOUT, 0);
        // set number of blocks in the buffer
        csr.wo(utra::combohash::SFR_OPT1, block_chunk.len() as u32 - 1);
        // set function
        csr.wfo(utra::combohash::SFR_CRFUNC_CR_FUNC, HashFunction::Sha256 as u32);
        // endianness swap
        csr.wo(
            utra::combohash::SFR_OPT3,
            Opt3SwapEndian::default()
                .with_seg_hout(true)
                .with_seg_msg(true)
                .with_seg_result(true)
                .raw_value(),
        );

        // debug dump
        #[cfg(feature = "debug")]
        for i in 0..utra::combohash::COMBOHASH_NUMREGS {
            crate::println!("{:x}: {:x}", i * 4, unsafe {
                (utra::combohash::HW_COMBOHASH_BASE as *const u32).add(i).read_volatile()
            });
        }

        // clear flags
        csr.wo(utra::combohash::SFR_FR, 0xf);
        // run the hash unit
        csr.wo(utra::combohash::SFR_AR, 0x5a);
        // wait to finish
        while csr.rf(utra::combohash::SFR_FR_MFSM_DONE) == 0 {
            // wait for mem to copy
        }
        // clear the flag on exit
        csr.rmwf(utra::combohash::SFR_FR_MFSM_DONE, 1);

        #[cfg(feature = "debug")]
        {
            let state = unsafe { core::slice::from_raw_parts(utralib::HW_SEG_HOUT_MEM as *const u32, 16) };
            let state_byte =
                unsafe { core::slice::from_raw_parts(utralib::HW_SEG_HOUT_MEM as *const u8, 32) };
            crate::println!("state: {:x?}", state);
            crate::println!("state_byte: {:x?}", state_byte);
        }

        // clear the first block setting - this was set by the new() function
        // it does not hurt to clear it every successive block
        csr.wfo(utra::combohash::SFR_OPT2_CR_OPT_IFSTART, 0);
    }
}
