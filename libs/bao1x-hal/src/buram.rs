use core::ops::Range;

const KEY_RANGE: Range<usize> = 2..8;
const KEY_LEN: usize = range_len(KEY_RANGE) * size_of::<u32>();
const HASH_LOC: usize = 0;
#[allow(dead_code)]
const RESERVED_LOC: usize = 1;

use crate::buram::murmur3::murmur3_32;

const fn range_len(r: Range<usize>) -> usize { r.end - r.start }

pub struct BackupManager {
    bu_reg: xous::MemoryRange,
    bu_ram: xous::MemoryRange,
}

impl BackupManager {
    #[cfg(not(feature = "std"))]
    pub fn new() -> Self {
        let bu_reg = unsafe {
            xous::MemoryRange::new(
                utralib::utra::aobureg::HW_AOBUREG_BASE,
                utralib::utra::aobureg::AOBUREG_NUMREGS * size_of::<u32>(),
            )
            .unwrap()
        };
        let bu_ram = unsafe {
            xous::MemoryRange::new(bao1x_api::offsets::AO_BU_MEM, bao1x_api::offsets::AO_BU_MEM_LEN).unwrap()
        };
        BackupManager { bu_reg, bu_ram }
    }

    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let bu_reg = xous::map_memory(
            xous::MemoryAddress::new(utralib::utra::aobureg::HW_AOBUREG_BASE),
            None,
            utralib::utra::aobureg::AOBUREG_NUMREGS * size_of::<u32>(),
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map backup register range");

        let bu_ram = xous::map_memory(
            xous::MemoryAddress::new(bao1x_api::offsets::AO_BU_MEM),
            None,
            bao1x_api::offsets::AO_BU_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map backup RAM range");
        BackupManager { bu_reg, bu_ram }
    }

    pub fn is_backup_valid(&self) -> bool {
        let bu_ram_u32: &[u32] = unsafe { self.bu_ram.as_slice() };
        let hash = murmur3_32(bu_ram_u32, 0x0);
        let bu_reg: &[u32] = unsafe { self.bu_reg.as_slice() };
        hash == bu_reg[HASH_LOC]
    }

    pub fn is_zero(&self) -> bool {
        let bu_reg: &[u32] = &unsafe { self.bu_reg.as_slice() }[..utralib::utra::aobureg::AOBUREG_NUMREGS];
        bu_reg.iter().all(|&x| x == 0)
    }

    pub fn make_valid(&mut self) {
        let bu_ram_u32: &[u32] = &unsafe { self.bu_ram.as_slice() };
        let hash = murmur3_32(bu_ram_u32, 0x0);
        unsafe {
            (self.bu_reg.as_mut_ptr() as *mut u32).add(HASH_LOC).write_volatile(hash);
        }
    }

    pub fn get_backup_key(&self) -> [u8; KEY_LEN] {
        let mut dest = [0u8; KEY_LEN];
        let bu_reg: &[u32] = &unsafe { self.bu_reg.as_slice() }[KEY_RANGE];
        for (&src, dst) in bu_reg.iter().zip(dest.chunks_mut(4)) {
            dst.copy_from_slice(&src.to_be_bytes());
        }
        dest
    }

    pub fn set_backup_key(&mut self, key: [u8; KEY_LEN]) {
        let bu_reg: &mut [u32] = &mut unsafe { self.bu_reg.as_slice_mut() }[KEY_RANGE];
        for (src, dst) in key.chunks(4).zip(bu_reg.iter_mut()) {
            *dst = u32::from_be_bytes(src.try_into().unwrap())
        }
    }

    pub fn bu_ram_as_slice(&self) -> &[u8] { unsafe { self.bu_ram.as_slice() } }

    pub fn bu_ram_as_mut(&mut self) -> &mut [u8] { unsafe { self.bu_ram.as_slice_mut() } }
}

pub mod murmur3 {
    // Copyright (c) 2020 Stu Small
    //
    // Licensed under the Apache License, Version 2.0
    // <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
    // license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
    // option. All files in the project carrying such notice may not be copied,
    // modified, or distributed except according to those terms.

    // This murmur3 code was vendored in on April 28, 2021. We choose to vendor the code for this simple
    // non-cryptographic hash in to reduce the number of crate dependencies in the build system. See
    // https://github.com/betrusted-io/xous-core/issues/54 for more details on why.
    // It also allowed us to adapt the code for our specific no_std needs.

    const C1: u32 = 0x85eb_ca6b;
    const C2: u32 = 0xc2b2_ae35;
    const R1: u32 = 16;
    const R2: u32 = 13;
    const M: u32 = 5;
    const N: u32 = 0xe654_6b64;

    pub fn murmur3_32(source: &[u32], seed: u32) -> u32 {
        let mut processed = 0;
        let mut state = seed;

        for &k in source.iter() {
            processed += 4;
            state ^= calc_k(k);
            state = state.rotate_left(R2);
            state = (state.wrapping_mul(M)).wrapping_add(N);
        }
        finish(state, processed)
    }

    fn finish(state: u32, processed: u32) -> u32 {
        let mut hash = state;
        hash ^= processed as u32;
        hash ^= hash.wrapping_shr(R1);
        hash = hash.wrapping_mul(C1);
        hash ^= hash.wrapping_shr(R2);
        hash = hash.wrapping_mul(C2);
        hash ^= hash.wrapping_shr(R1);
        hash
    }

    fn calc_k(k: u32) -> u32 {
        const C1: u32 = 0xcc9e_2d51;
        const C2: u32 = 0x1b87_3593;
        const R1: u32 = 15;
        k.wrapping_mul(C1).rotate_left(R1).wrapping_mul(C2)
    }
}
