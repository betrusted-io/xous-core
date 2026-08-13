#![allow(dead_code)]

use core::convert::TryFrom;
use core::convert::TryInto;
use core::mem::size_of;

use utralib::generated::*;

use super::artyvexii::PT_LIMIT;

pub fn ramtests() {
    unsafe { caching_tests() };

    const BASE_ADDR: u32 = PT_LIMIT as u32;

    unsafe {
        crate::println!("bytestrobes");
        check_byte_strobes();

        // 'random' access test
        crate::println!("ra");
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u32, 512);
        ramtest_lfsr(&mut test_slice, 3);

        // now some basic memory read/write tests
        // entirely within cache access test
        // 256-entry by 32-bit slice at start of RAM
        crate::println!("basic r/w");
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u32, 256);
        ramtest_all(&mut test_slice, 4);
        // byte access test
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u8, 256);
        ramtest_fast(&mut test_slice, 5);
        // word access test
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u16, 512);
        ramtest_fast(&mut test_slice, 6); // 1ff00

        // outside cache test
        // 6144-entry by 32-bit slice at start of RAM - should cross outside cache boundary
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u32, 0x1800);
        ramtest_fast(&mut test_slice, 7); // c7f600

        // this passed, now that the AXI state machine is fixed.
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u32, 0x1800);
        ramtest_fast_specialcase1(&mut test_slice, 8); // c7f600

        // u64 access test
        let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u64, 0xC00);
        ramtest_fast(&mut test_slice, 9);

        // random size/access test
        // let mut test_slice = core::slice::from_raw_parts_mut(BASE_ADDR as *mut u8, 0x6000);
    }
}
pub unsafe fn check_byte_strobes() {
    let u8_test = utra::rgb::HW_RGB_BASE as *mut u8;
    let u16_test = utra::rgb::HW_RGB_BASE as *mut u16;

    // quick test to check byte and word write strobes on the
    unsafe {
        u8_test.write_volatile(0x31);
        u8_test.add(1).write_volatile(32);
        u8_test.add(2).write_volatile(33);
        u8_test.add(3).write_volatile(34);

        u16_test.write_volatile(0x44);
        u16_test.add(1).write_volatile(0x55);
    }
}
pub unsafe fn caching_tests() -> usize {
    // test of the 0x500F cache flush instruction - this requires manual inspection of the report values
    const CACHE_WAYS: usize = 4;
    const CACHE_SET_SIZE: usize = 4096 / size_of::<u32>();
    let test_slice = core::slice::from_raw_parts_mut(PT_LIMIT as *mut u32, CACHE_SET_SIZE * CACHE_WAYS);
    crate::println!("at {:x}", test_slice.as_ptr() as usize);
    // bottom of cache
    for set in 0..4 {
        (&mut test_slice[set * CACHE_SET_SIZE] as *mut u32).write_volatile(0x0011_1111 * (1 + set as u32));
        flush_phys_one(unsafe { test_slice.as_ptr().add(set * CACHE_SET_SIZE) } as usize);
    }
    // top of cache
    for set in 0..4 {
        (&mut test_slice[set * CACHE_SET_SIZE + CACHE_SET_SIZE - 1] as *mut u32)
            .write_volatile(0x1100_2222 * (1 + set as u32));
        flush_phys_one(unsafe { test_slice.as_ptr().add(set * CACHE_SET_SIZE + CACHE_SET_SIZE - 1) } as usize);
    }
    // read cached values - first iteration populates the cache; second iteration should be cached
    for _iter in 0..2 {
        for set in 0..4 {
            let a = (&mut test_slice[set * CACHE_SET_SIZE] as *mut u32).read_volatile();
            crate::println!("a: {:x}", a);
            let b = (&mut test_slice[set * CACHE_SET_SIZE + CACHE_SET_SIZE - 1] as *mut u32).read_volatile();
            crate::println!("b: {:x}", b);
        }
    }
    // flush cache
    crate::println!("flush");
    // bottom of cache
    for set in 0..4 {
        flush_phys_one(unsafe { test_slice.as_ptr().add(set * CACHE_SET_SIZE) as usize });
    }
    // top of cache
    for set in 0..4 {
        flush_phys_one(unsafe {
            test_slice.as_ptr().add(set * CACHE_SET_SIZE + CACHE_SET_SIZE - 1) as usize
        });
    }

    // read cached values - first iteration populates the cache; second iteration should be cached
    for _iter in 0..2 {
        for set in 0..4 {
            let a = (&mut test_slice[set * CACHE_SET_SIZE] as *mut u32).read_volatile();
            crate::println!("a: {:x}", a);
            let b = (&mut test_slice[set * CACHE_SET_SIZE + CACHE_SET_SIZE - 1] as *mut u32).read_volatile();
            crate::println!("b: {:x}", b);
        }
    }

    // check that caching is disabled for I/O regions
    let mut csrtest = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    let mut passing = 1;
    for i in 0..4 {
        csrtest.wfo(utra::csrtest::WTEST_WTEST, i);
        let val = csrtest.rf(utra::csrtest::RTEST_RTEST);
        if val != i + 0x1000_0000 {
            passing = 0;
        }
    }
    crate::println!("caching tests: {}", passing);
    passing
}

/// chunks through the entire bank of data
pub unsafe fn ramtest_all<T>(test_slice: &mut [T], test_index: u32) -> usize
where
    T: TryFrom<usize> + TryInto<u32> + Default + Copy,
{
    let mut sum: u32 = 0;
    for (index, d) in test_slice.iter_mut().enumerate() {
        // Convert the element into a `u32`, failing
        (d as *mut T).write_volatile(index.try_into().unwrap_or_default());
        sum += TryInto::<u32>::try_into(index).unwrap();
    }
    let mut checksum: u32 = 0;
    for d in test_slice.iter() {
        let a = (d as *const T).read_volatile().try_into().unwrap_or_default();
        checksum += a;
        // report_api(a);
    }

    if sum == checksum {
        crate::println!("ramtest {} passing: {:x}", test_index, checksum);
        1
    } else {
        crate::println!("ramtest failing ({}): {:x} vs {:x}", test_index, checksum, sum);
        0
    }
}

/// only touches two words on each cache line
/// this one tries to write the same word twice to two consecutive addresses
/// this causes the valid strobe to hit twice in a row. seems to pass.
pub unsafe fn ramtest_fast_specialcase1<T>(test_slice: &mut [T], test_index: u32) -> usize
where
    T: TryFrom<usize> + TryInto<u32> + Default + Copy,
{
    const CACHE_LINE_SIZE: usize = 32;
    let mut sum: u32 = 0;
    for (index, d) in test_slice.chunks_mut(CACHE_LINE_SIZE / size_of::<T>()).enumerate() {
        let idxp1 = index + 0;
        // unroll the loop to force b2b writes
        sum += TryInto::<u32>::try_into(index).unwrap();
        sum += TryInto::<u32>::try_into(idxp1).unwrap();
        // Convert the element into a `u32`, failing
        (d.as_mut_ptr() as *mut T).write_volatile(index.try_into().unwrap_or_default());
        // Convert the element into a `u32`, failing
        (d.as_mut_ptr().add(1) as *mut T).write_volatile(idxp1.try_into().unwrap_or_default());
    }
    let mut checksum: u32 = 0;
    for d in test_slice.chunks(CACHE_LINE_SIZE / size_of::<T>()) {
        checksum += (d.as_ptr() as *const T).read_volatile().try_into().unwrap_or_default();
        checksum += (d.as_ptr().add(1) as *const T).read_volatile().try_into().unwrap_or_default();
    }

    if sum == checksum {
        crate::println!("specialcase passing ({}): {:x}", test_index, checksum);
        1
    } else {
        crate::println!("specialcase failing ({}): {:x} vs {:x}", test_index, checksum, sum);
        0
    }
}

/// only touches two words on each cache line
pub unsafe fn ramtest_fast<T>(test_slice: &mut [T], test_index: u32) -> usize
where
    T: TryFrom<usize> + TryInto<u32> + Default + Copy,
{
    const CACHE_LINE_SIZE: usize = 32;
    let mut sum: u32 = 0;
    for (index, d) in test_slice.chunks_mut(CACHE_LINE_SIZE / size_of::<T>()).enumerate() {
        let idxp1 = index + 1;
        // unroll the loop to force b2b writes
        sum += TryInto::<u32>::try_into(index).unwrap();
        sum += TryInto::<u32>::try_into(idxp1).unwrap();
        // Convert the element into a `u32`, failing
        (d.as_mut_ptr() as *mut T).write_volatile(index.try_into().unwrap_or_default());
        // Convert the element into a `u32`, failing
        (d.as_mut_ptr().add(1) as *mut T).write_volatile(idxp1.try_into().unwrap_or_default());
    }
    let mut checksum: u32 = 0;
    for d in test_slice.chunks(CACHE_LINE_SIZE / size_of::<T>()) {
        let a = (d.as_ptr() as *const T).read_volatile().try_into().unwrap_or_default();
        let b = (d.as_ptr().add(1) as *const T).read_volatile().try_into().unwrap_or_default();
        checksum = checksum + a + b;
        // report_api(a);
        // report_api(b);
    }

    if sum == checksum {
        crate::println!("fast passing ({}): {:x}", test_index, checksum);
        1
    } else {
        crate::println!("fast failing ({}): {:x} vs {:x}", test_index, checksum, sum);
        0
    }
}

/// uses an LFSR to cycle through "random" locations. The slice length
/// should equal the (LFSR period+1), so that we guarantee that each entry
/// is visited once.
pub unsafe fn ramtest_lfsr<T>(test_slice: &mut [T], test_index: u32) -> usize
where
    T: TryFrom<usize> + TryInto<u32> + Default + Copy,
{
    if test_slice.len() != 512 {
        return 0;
    }
    let mut state: u16 = 1;
    let mut sum: u32 = 0;
    const MAX_STATES: usize = 511;
    (&mut test_slice[0] as *mut T).write_volatile(0.try_into().unwrap_or_default()); // the 0 index is never written to by this, initialize it to 0
    for i in 0..MAX_STATES {
        let wr_val = i * 3;
        (&mut test_slice[state as usize] as *mut T).write_volatile(wr_val.try_into().unwrap_or_default());
        sum += wr_val as u32;
        state = lfsr_next(state);
    }

    // flush cache
    crate::println!("cache flush");
    flush_phys_range(
        test_slice.as_ptr() as usize
            ..test_slice.as_ptr() as usize + test_slice.len() * core::mem::size_of::<T>(),
    );

    // we should be able to just iterate in-order and sum all the values, and get the same thing back as above
    let mut checksum: u32 = 0;
    for d in test_slice.iter() {
        let a = (d as *const T).read_volatile().try_into().unwrap_or_default();
        checksum += a;
        // report_api(a);
    }

    if sum == checksum {
        crate::println!("lfsr passing ({}): {:x}", test_index, checksum);
        1
    } else {
        crate::println!("lfsr failing ({}): {:x} vs {:x}", test_index, checksum, sum);
        0
    }
}

#[inline(always)]
pub fn flush_phys_range(region: core::ops::Range<usize>) {
    for i in region.step_by(512 / 8) {
        // unsafe { flush_block(i) };
        unsafe {
            core::arch::asm!(
                ".word 0x0025200f",
                in("a0") i,
                options(nostack)
            );
        }
    }
}

#[inline(always)]
pub fn flush_phys_one(phys_addr: usize) {
    unsafe {
        core::arch::asm!(
            ".word 0x0025200f",
            in("a0") phys_addr,
            options(nostack)
        );
    }
}

/// our desired test length is 512 entries, so pick an LFSR with a period of 2^9-1...
pub fn lfsr_next(state: u16) -> u16 {
    let bit = ((state >> 8) ^ (state >> 4)) & 1;

    ((state << 1) + bit) & 0x1_FF
}
