use std::convert::TryInto;

use crate::{api, backend::{BasisRoot, FREE_CACHE_SIZE}};
use pddb::PDDB_MAX_BASIS_NAME_LEN;
use rand_core::{CryptoRng, RngCore};
use core::ops::Deref;

/// OS-specific PDDB structures

pub(crate) struct PddbOs {
    spinor: spinor::Spinor,
    pddb_mr: xous::MemoryRange,
    trng: trng::Trng,
}

impl PddbOs {
    pub fn new() -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize),
            None,
            xous::PDDB_LEN as usize,
            xous::MemoryFlags::R,
        )
        .expect("Couldn't map the PDDB memory range");

        PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            pddb_mr: pddb,
            trng: trng::Trng::new(&xns).unwrap(),
        }
    }


    /// this function is dangerous in that calling it will completely erase all of the previous data
    /// in the PDDB an replace it with a brand-spanking new, blank PDDB.
    /// The number of servers that can connect to the Spinor crate is strictly tracked, so we borrow a reference
    /// to the Spinor object allocated to the PDDB implementation for this operation.
    pub fn format_pddb(&mut self) {
        // step 1. Erase the entire PDDB region.
        log::info!("Erasing the PDDB region");
        let blank_sector: [u8; 4096] = [0xff; 4096];

        for offset in (0..xous::PDDB_LEN).step_by(4096) {
            self.spinor.patch(
                self.pddb_mr.as_slice(),
                xous::PDDB_LOC,
                &blank_sector,
                offset
            ).expect("couldn't erase memory");
        }

        // step 2. create the system basis root structure
        let mut p_nonce: [u8; 12] = [0; 12];
        self.trng.fill_bytes(&mut p_nonce);
        let mut name: [u8; PDDB_MAX_BASIS_NAME_LEN] = [0; PDDB_MAX_BASIS_NAME_LEN];
        write!(name, PDDB_DEFAULT_SYSTEM_BASIS).unwrap();
        let mut basis_root = BasisRoot {
            p_nonce,
            magic: api::PDDB_MAGIC,
            version: api::PDDB_VERSION,
            journal_rev: 0,
            name,
            age: 0,
            num_dictionaries: 0,
            free_cache: [None; FREE_CACHE_SIZE],
        };
        // extract a slice-u8 that maps onto the basis_root record, allowing us to patch this into a FLASH page
        let br_slice: &[u8] = basis_root.deref();

        // TODO: unfortunately, we need to figure out some free space structure at this point, I think -- because
        // we are getting to the point of having to allocate the free_cache data before writing it.

        // next: figure out the length of the page table + mbbb (make before break buffer)
        //  + space for the (fscb) free space circular buffer, and then locate the basis root there

    }
}