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

pub struct BioSharedState {
    pub bio: CSR<u32>,
}
impl BioSharedState {
    #[cfg(feature = "baremetal")]
    pub fn new() -> Self {
        BioSharedState {
            bio: CSR::new(utra::bio::HW_BIO_BASE as *mut u32),
        }
    }

    #[cfg(not(feature = "baremetal"))]
    pub fn new() -> Self {
        // Note: this requires a memory region window to be manually specified in create-image
        // so that the loader maps the pages for the PIO block. This is because the PIO block is
        // an IP block that is created *outside* of the normal LiteX ecosystem. Specifically look in
        // xtask/src/builder.rs for a "--extra-svd" argument that refers to precursors/pio.svd.
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
}