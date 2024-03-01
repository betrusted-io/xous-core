#![cfg_attr(feature = "baremetal", no_std)]
use utralib::generated::*;

#[cfg(feature = "tests")]
pub mod bio_tests;

#[derive(Debug)]
pub enum BioError {
    /// specified state machine is not valid
    InvalidSm,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
}

pub fn get_id() -> u32 {
    let bio_ss = BioSharedState::new();
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio::SFR_CFGINFO.offset() as u32);
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio::HW_BIO_BASE as u32);
    bio_ss.bio.r(utra::bio::SFR_CFGINFO)
}

/// used to generate some test vectors
pub fn lfsr_next(state: u16) -> u16 {
    let bit = ((state >> 8) ^ (state >> 4)) & 1;

    ((state << 1) + bit) & 0x1_FF
}

type BioRustFn = unsafe fn();

pub fn fn_to_slice(target_fn: BioRustFn, endcap_fn: BioRustFn) -> &'static [u8] {
    let start_ptr = target_fn as *const u8;
    unsafe {
        core::slice::from_raw_parts(start_ptr,
            (endcap_fn as *const u8) as usize - start_ptr as usize
        )
    }
}

pub struct BioSharedState {
    pub bio: CSR<u32>,
    pub imem_slice: &'static mut [u32],
}
impl BioSharedState {
    #[cfg(feature = "baremetal")]
    pub fn new() -> Self {
        // map the instruction memory
        let imem_slice = unsafe {
            core::slice::from_raw_parts_mut(
                utralib::generated::HW_BIO_RAM_MEM as *mut u32,
                utralib::generated::HW_BIO_RAM_MEM_LEN
            )
        };

        BioSharedState {
            bio: CSR::new(utra::bio::HW_BIO_BASE as *mut u32),
            imem_slice,
        }
    }

    #[cfg(not(feature = "baremetal"))]
    pub fn new() -> Self {
        // TODO
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::bio::HW_BIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        BioSharedState {
            bio: CSR::new(csr.as_mut_ptr() as *mut u32),
        }
    }

    pub fn load_code(&mut self, prog: &[u8], offset_bytes: usize) {
        let offset = offset_bytes / core::mem::size_of::<u32>();
        for (i, chunk) in prog.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
                self.imem_slice[i + offset] = word;
            } else {
                // copy the last word as a "ragged word"
                let mut ragged_word = 0;
                for (j, &b) in chunk.iter().enumerate() {
                    ragged_word |= (b as u32) << (4 - chunk.len() + j);
                }
                self.imem_slice[i + offset] = ragged_word;
            }
        }
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
            unsafe {
                report_api($name_start as u32);
                report_api($name_end as u32);
            }
            unsafe { core::slice::from_raw_parts($name_start, ($name_end as usize) - ($name_start as usize))}
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