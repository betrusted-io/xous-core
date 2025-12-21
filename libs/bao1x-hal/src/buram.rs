use core::ops::Range;

const KEY_RANGE: Range<usize> = 2..8;
const KEY_LEN: usize = range_len(KEY_RANGE) * size_of::<u32>();
const HASH_LOC: usize = 0;
const FLAGS_LOC: usize = 1;

pub const ERASURE_PROOF_RANGE_BYTES: Range<usize> = 0..32;
// special case: don't hash in the erasure_proof range, because this
// value is passed between boot0 all the way to the loader for checking,
// *but* the consistency check on backup regs is done in boot1!
//
// Conveniently, due to an errata, this location of RAM is not preserved
// on soft reset, and thus it's perfect for this application.
pub const HASH_REGION_START: usize = ERASURE_PROOF_RANGE_BYTES.end;

// This is an additional range in backup RAM that is not preserved
// across soft resets - and thus this value cannot be used in the hash.
pub const ERRATUM_RANGE_BYTES: Range<usize> = 0x2000..0x2020;

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
        let bu_ram =
            unsafe { xous::MemoryRange::new(utralib::HW_AORAM_MEM, utralib::HW_AORAM_MEM_LEN).unwrap() };
        BackupManager { bu_reg, bu_ram }
    }

    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let bu_reg = xous::map_memory(
            xous::MemoryAddress::new(utralib::utra::aobureg::HW_AOBUREG_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map backup register range");

        let bu_ram = xous::map_memory(
            xous::MemoryAddress::new(utralib::HW_AORAM_MEM),
            None,
            utralib::HW_AORAM_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map backup RAM range");
        BackupManager { bu_reg, bu_ram }
    }

    /// The RAM hash is complicated by an erratum where the first word of each bank of the BURAM
    /// can be corrupted on a reset (the banking is an internal implementation detail that was
    /// meant to be transparent to software).
    fn hash_ram(&self) -> u32 {
        // hash from the start of the hash region to the beginning of the erratum range
        let bu_ram_front_u32: &[u32] = &unsafe { self.bu_ram.as_slice() }
            [HASH_REGION_START / size_of::<u32>()..ERRATUM_RANGE_BYTES.start / size_of::<u32>()];
        let hash_front = murmur3_32(bu_ram_front_u32, 0x0);

        // hash from the end of the erratum range to the end of memory. Use the hash of the
        // first half as the seed.
        let bu_ram_back_u32: &[u32] =
            &unsafe { self.bu_ram.as_slice() }[ERRATUM_RANGE_BYTES.end / size_of::<u32>()..];
        murmur3_32(bu_ram_back_u32, hash_front)
    }

    /// Validity is computed using a hash of the backup RAM region less the "erase proof" slice.
    pub fn is_backup_valid(&self) -> bool {
        let bu_reg: &[u32] = &unsafe { self.bu_reg.as_slice() }[..utralib::utra::aobureg::AOBUREG_NUMREGS];
        self.hash_ram() == bu_reg[HASH_LOC]
    }

    pub fn is_zero(&self) -> bool {
        let bu_reg: &[u32] = &unsafe { self.bu_reg.as_slice() }[..utralib::utra::aobureg::AOBUREG_NUMREGS];
        bu_reg.iter().all(|&x| x == 0)
    }

    pub fn get_flags(&self) -> bao1x_api::BackupFlags {
        bao1x_api::BackupFlags::new_with_raw_value(unsafe { self.bu_reg.as_slice()[FLAGS_LOC] })
    }

    pub fn set_flags(&mut self, flag: bao1x_api::BackupFlags) {
        unsafe { self.bu_reg.as_slice_mut()[FLAGS_LOC] = flag.raw_value() };
    }

    pub fn make_valid(&mut self) {
        let hash = self.hash_ram();
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

    pub fn store_slice<T: Copy>(&mut self, input: &[T], byte_offset: usize) {
        let type_offset = byte_offset / size_of::<T>();
        // safety: safe because make_valid() is called after the slice is accessed
        let dest: &mut [T] = unsafe { &mut self.bu_ram_as_mut()[type_offset..type_offset + input.len()] };
        dest.copy_from_slice(&input);

        self.make_valid();
    }

    /// Safety: the caller needs to follow up with `make_valid()` for the backup registers to be recognized as
    /// valid
    pub unsafe fn store_slice_no_hash<T: Copy>(&mut self, input: &[T], byte_offset: usize) {
        let type_offset = byte_offset / size_of::<T>();
        // safety: safe because make_valid() is called after the slice is accessed
        let dest: &mut [T] = unsafe { &mut self.bu_ram_as_mut()[type_offset..type_offset + input.len()] };
        dest.copy_from_slice(&input);
    }

    pub fn get_slice<T>(&self, byte_range: core::ops::Range<usize>) -> &[T] {
        let type_offset = byte_range.start / size_of::<T>();
        let type_len = (byte_range.end - byte_range.start) / size_of::<T>();
        &self.bu_ram_as_slice()[type_offset..type_offset + type_len]
    }

    /// Returns the full array of the backup RAM, including the erase validation region. Required by
    /// the slice-based accessors above.
    pub fn bu_ram_as_slice<T>(&self) -> &[T] { unsafe { self.bu_ram.as_slice::<T>() } }

    /// Returns the full array of the backup RAM, including the erase validation region. Required
    /// by the slice-based accessors above.
    ///
    /// Safety: modifications to the backup RAM array need a follow-up call to `make_valid`
    /// in order for the boot check to pass.
    ///
    /// Could try to get clever and implement a `Drop` trait which includes a make_valid() call? maybe?
    pub unsafe fn bu_ram_as_mut<T>(&mut self) -> &mut [T] { self.bu_ram.as_slice_mut::<T>() }

    /// Returns only the *hashable* region of the backup RAM. The first 32 bytes are reserved for
    /// passing erase validation structures through the boot loader.
    ///
    /// Safety: modifications to the backup RAM array need a follow-up call to `make_valid`
    /// in order for the boot check to pass.
    ///
    /// Could try to get clever and implement a `Drop` trait which includes a make_valid() call? maybe?
    pub unsafe fn bu_hashable_ram_as_mut<T>(&mut self) -> &mut [T] {
        &mut self.bu_ram.as_slice_mut::<T>()[HASH_REGION_START..]
    }
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
