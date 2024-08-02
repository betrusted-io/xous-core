//! FIFO - 8-deep fifo head/tail access. Cores halt on overflow/underflow.
//! - x16 r/w  fifo[0]
//! - x17 r/w  fifo[1]
//! - x18 r/w  fifo[2]
//! - x19 r/w  fifo[3]
//!
//! Quantum - core will halt until host-configured clock divider pules occurs,
//! or an external event comes in on a host-specified GPIO pin.
//! - x20 -/w  halt to quantum
//!
//! GPIO - note clear-on-0 semantics for bit-clear for data pins!
//!   This is done so we can do a shift-and-move without an invert to
//!   bitbang a data pin. Direction retains a more "conventional" meaning
//!   where a write of `1` to either clear or set will cause the action,
//!   as pin direction toggling is less likely to be in a tight inner loop.
//! - x21 r/w  write: (x26 & x21) -> gpio pins; read: gpio pins -> x21
//! - x22 -/w  (x26 & x22) -> `1` will set corresponding pin on gpio
//! - x23 -/w  (x26 & x23) -> `0` will clear corresponding pin on gpio
//! - x24 -/w  (x26 & x24) -> `1` will make corresponding gpio pin an output
//! - x25 -/w  (x26 & x25) -> `1` will make corresponding gpio pin an input
//! - x26 r/w  mask GPIO action outputs
//!
//! Events - operate on a shared event register. Bits [31:24] are hard-wired to FIFO
//! level flags, configured by the host; writes to bits [31:24] are ignored.
//! - x27 -/w  mask event sensitivity bits
//! - x28 -/w  `1` will set the corresponding event bit. Only [23:0] are wired up.
//! - x29 -/w  `1` will clear the corresponding event bit Only [23:0] are wired up.
//! - x30 r/-  halt until ((x27 & events) != 0), and return unmasked `events` value
//!
//! Core ID & debug:
//! - x31 r/-  [31:30] -> core ID; [29:0] -> cpu clocks since reset

#![cfg_attr(feature = "baremetal", no_std)]
use core::mem::size_of;

use utralib::generated::*;

#[cfg(feature = "tests")]
pub mod bio_tests;

pub mod i2c;

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BioCore {
    Core0 = 0,
    Core1 = 1,
    Core2 = 2,
    Core3 = 3,
}

#[derive(Debug)]
pub enum BioError {
    /// specified state machine is not valid
    InvalidSm,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
    /// Loaded code did not match, first error at argument
    CodeCheck(usize),
}

pub fn get_id() -> u32 {
    let bio_ss = BioSharedState::new();
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio_bdma::SFR_CFGINFO.offset() as u32);
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio_bdma::HW_BIO_BDMA_BASE as u32);
    bio_ss.bio.r(utra::bio_bdma::SFR_CFGINFO)
}

/// used to generate some test vectors
pub fn lfsr_next(state: u16) -> u16 {
    let bit = ((state >> 8) ^ (state >> 4)) & 1;

    ((state << 1) + bit) & 0x1_FF
}

/// used to generate some test vectors
pub fn lfsr_next_u32(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}

pub const BIO_PRIVATE_MEM_LEN: usize = 4096;

pub struct BioSharedState {
    pub bio: CSR<u32>,
    pub imem_slice: [&'static mut [u32]; 4],
}
impl BioSharedState {
    #[cfg(feature = "baremetal")]
    pub fn new() -> Self {
        // map the instruction memory
        let imem_slice = unsafe {
            [
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM0_MEM as *mut u32,
                    HW_BIO_IMEM0_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM1_MEM as *mut u32,
                    HW_BIO_IMEM1_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM2_MEM as *mut u32,
                    HW_BIO_IMEM2_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM3_MEM as *mut u32,
                    HW_BIO_IMEM3_MEM_LEN / size_of::<u32>(),
                ),
            ]
        };

        BioSharedState { bio: CSR::new(utra::bio_bdma::HW_BIO_BDMA_BASE as *mut u32), imem_slice }
    }

    #[cfg(not(feature = "baremetal"))]
    pub fn new() -> Self {
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::bio_bdma::HW_BIO_BDMA_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        let imem0 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM0_MEM),
            None,
            utralib::HW_BIO_IMEM0_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem1 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM1_MEM),
            None,
            utralib::HW_BIO_IMEM1_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem2 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM2_MEM),
            None,
            utralib::HW_BIO_IMEM2_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem3 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM3_MEM),
            None,
            utralib::HW_BIO_IMEM3_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        BioSharedState {
            bio: CSR::new(csr.as_mut_ptr() as *mut u32),
            imem_slice: unsafe {
                [imem0.as_slice_mut(), imem1.as_slice_mut(), imem2.as_slice_mut(), imem3.as_slice_mut()]
            },
        }
    }

    pub fn load_code(&mut self, prog: &[u8], offset_bytes: usize, core: BioCore) {
        let offset = offset_bytes / core::mem::size_of::<u32>();
        for (i, chunk) in prog.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
                self.imem_slice[core as usize][i + offset] = word;
            } else {
                // copy the last word as a "ragged word"
                let mut ragged_word = 0;
                for (j, &b) in chunk.iter().enumerate() {
                    ragged_word |= (b as u32) << (4 - chunk.len() + j);
                }
                self.imem_slice[core as usize][i + offset] = ragged_word;
            }
        }
    }

    pub fn verify_code(&mut self, prog: &[u8], offset_bytes: usize, core: BioCore) -> Result<(), BioError> {
        let offset = offset_bytes / core::mem::size_of::<u32>();
        for (i, chunk) in prog.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
                let rbk = self.imem_slice[core as usize][i + offset];
                if rbk != word {
                    print!("{:?} expected {:x} got {:x} at {}\r", core, word, rbk, i + offset);
                    return Err(BioError::CodeCheck(i + offset));
                }
            } else {
                // copy the last word as a "ragged word"
                let mut ragged_word = 0;
                for (j, &b) in chunk.iter().enumerate() {
                    ragged_word |= (b as u32) << (4 - chunk.len() + j);
                }
                if self.imem_slice[core as usize][i + offset] != ragged_word {
                    return Err(BioError::CodeCheck(i + offset));
                };
            }
        }
        Ok(())
    }
}

#[macro_export]
/// This macro takes three identifiers and assembly code:
///   - name of the function to call to retrieve the assembled code
///   - a unique identifier that serves as label name for the start of the code
///   - a unique identifier that serves as label name for the end of the code
///   - a comma separated list of strings that form the assembly itself
///
///   *** The comma separated list must *not* end in a comma. ***
///
///   The macro is unable to derive names of functions or identifiers for labels
///   due to the partially hygienic macro rules of Rust, so you have to come
///   up with a list of unique names by yourself.
macro_rules! bio_code {
    ($fn_name:ident, $name_start:ident, $name_end:ident, $($item:expr),*) => {
        pub fn $fn_name() -> &'static [u8] {
            extern {
                static $name_start: *const u8;
                static $name_end: *const u8;
            }
            /*
            unsafe {
                report_api($name_start as u32);
                report_api($name_end as u32);
            }
            */
            // skip the first 4 bytes, as they contain the loading offset
            unsafe { core::slice::from_raw_parts($name_start.add(4), ($name_end as usize) - ($name_start as usize) - 4)}
        }

        core::arch::global_asm!(
            ".align 4",
            concat!(".globl ", stringify!($name_start)),
            concat!(stringify!($name_start), ":"),
            ".word .",
            $($item),*
            , ".align 4",
            concat!(".globl ", stringify!($name_end)),
            concat!(stringify!($name_end), ":"),
            ".word .",
        );
    };
}
