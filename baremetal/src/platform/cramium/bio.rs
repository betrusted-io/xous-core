use alloc::string::String;
use alloc::vec::Vec;

use cramium_hal::cache_flush;
use utralib::utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE;
use utralib::*;
use xous_bio_bdma::*;

use crate::{print, println};

/*
pub fn bdma_test(_args: &Vec<String>, seed: u32) -> usize {
    let concurrent = true;
    let clkmode = 0;

    const TEST_LEN: usize = 64;
    let mut passing = 0;
    if !concurrent {
        println!("DMA basic2w");
    } else {
        println!("DMA cpu+dma concurrent");
    }
    crate::println!("clkmode {}", clkmode);
    let mut bio_ss = BioSharedState::new();
    bio_ss.init();
    // must disable DMA filtering
    bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, 1);
    bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, 1);

    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(bm_dma_basic_code(), 0, BioCore::Core3);

    // setup clocking mode option
    bio_ss.bio.rmwf(SFR_CONFIG_CLOCKING_MODE, clkmode as u32);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    // bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x888);
    bio_ss.set_core_run_states([false, false, false, true]);

    let mut main_mem_src: [u32; TEST_LEN] = [0u32; TEST_LEN];
    let mut main_mem_dst: [u32; TEST_LEN] = [0u32; TEST_LEN];
    // just conjure some locations out of thin air. Yes, these are weird addresses in decimal, meant to
    // just poke into some not page aligned location in IFRAM.
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 8200) as *mut u32, TEST_LEN) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 10000) as *mut u32, TEST_LEN) };
    ifram_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, seed + 0, "m->m", concurrent);

    main_mem_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, seed + 0x40, "i0->m", concurrent);

    ifram_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, seed + 0x80, "m->i1", concurrent);

    main_mem_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, ifram_dst, seed + 0xC0, "i0->i1", concurrent);

    if !concurrent {
        println!("DMA basic done.");
    } else {
        println!("DMA cpu+dma concurrent done.");
    }
    if passing == 4 {
        println!("All passed!");
    } else {
        println!("Failed: {}/4 passsing", passing);
    }
    passing
}

fn basic_u32(
    bio_ss: &mut BioSharedState,
    src: &mut [u32],
    dst: &mut [u32],
    seed: u32,
    name: &'static str,
    concurrent: bool,
) -> usize {
    assert!(src.len() == dst.len());
    println!("  - {}", name);
    let mut tp = TestPattern::new(Some(seed));
    for d in src.iter_mut() {
        *d = tp.next();
    }
    for d in dst.iter_mut() {
        *d = tp.next();
    }
    let mut pass = 1;
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, src.as_ptr() as u32); // src address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, dst.as_ptr() as u32); // dst address
    if !concurrent {
        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, (src.len() * size_of::<u32>()) as u32); // bytes to move
        while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0x1 == 0 {}
        cache_flush();
        let mut errs = 0;
        for (i, &d) in src.iter().enumerate() {
            let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
            if rbk != d {
                if errs < 4 {
                    unsafe {
                        println!(
                            "{} @{:x}/{:x}, {:x} rb:{:x}",
                            name,
                            src.as_ptr().add(i) as usize,
                            dst.as_ptr().add(i) as usize,
                            d,
                            rbk
                        );
                    }
                } else {
                    print!("x");
                }
                errs += 1;
                pass = 0;
            }
        }
        if errs > 0 {
            println!("");
            // errs = 0;
        }
    } else {
        // this flushes any read data from the cache, so that the CPU copy is forced to fetch
        // the data from memory
        cache_flush();
        let len = src.len();
        // note this kicks off a copy of only the first half of the slice
        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, (src.len() * size_of::<u32>()) as u32 / 2);
        // the second half of the slice is copied by the CPU, simultaneously
        dst[len / 2..].copy_from_slice(&src[len / 2..]);
        cache_flush();
        // run it twice to generate more traffic
        dst[len / 2..].copy_from_slice(&src[len / 2..]);
        // ... and wait for DMA to finish, if it has not already
        while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0x1 == 0 {}
        cache_flush();
        for (i, &d) in src.iter().enumerate() {
            let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
            if rbk != d {
                println!("(c) {} DMA err @{}, {:x} rbk: {:x}", name, i, d, rbk);
                pass = 0;
            }
        }
    }
    pass
}

#[rustfmt::skip]
bio_code!(bm_dma_basic_code, BM_DMA_BASIC_START, BM_DMA_BASIC_END,
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "li x29, 0x1",      // clear event done flag - just before the last parameter arrives
    "mv a1, x16",       // wait for # of bytes to move

    "sw  x0, 0(a2)",    // make sure write pipeline is in a good state
    // "sw  x0, 0(a2)",    // make sure write pipeline is in a good state - maybe required due to hw issue in some clock modes, needs more testing

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lw  t0, 0(a3)",    // blocks until load responds
    "sw  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 4",   // 3 cycles
    "sw  t0, 0(a2)",    // blocks until store completes
    "addi a2, a2, 4",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
    "li x28, 0x1",      // flip event done flag
    "j 20b"
);
*/

pub struct TestPattern {
    x: u32,
}
impl TestPattern {
    pub fn new(seed: Option<u32>) -> Self { Self { x: seed.unwrap_or(0) } }

    /// from https://github.com/skeeto/hash-prospector
    pub fn next(&mut self) -> u32 {
        if self.x == 0 {
            self.x += 1;
        }
        self.x ^= self.x >> 17;
        self.x *= 0xed5ad4bb;
        self.x ^= self.x >> 11;
        self.x *= 0xac4c1b51;
        self.x ^= self.x >> 15;
        self.x *= 0x31848bab;
        self.x ^= self.x >> 14;
        return self.x;
    }
}

pub fn bdma_coincident_test(_args: &Vec<String>, seed: u32) -> usize {
    let clkmode = 0;
    const TEST_LEN: usize = 1024;

    let mut passing = 0;
    println!("DMA coincident mode {}", clkmode);
    let mut bio_ss = BioSharedState::new();
    bio_ss.init();
    // must disable DMA filtering
    bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, 1);
    bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, 1);

    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio_bdma::SFR_FIFO_CLR, 0xF);
    // setup clocking mode option
    bio_ss.bio.rmwf(SFR_CONFIG_CLOCKING_MODE, clkmode as u32);
    bio_ss.load_code(bm_dma_coincident_code(), 0, BioCore::Core0);
    bio_ss.load_code(bm_dma_coincident_code(), 0, BioCore::Core1);
    bio_ss.load_code(bm_dma_coincident_code(), 0, BioCore::Core2);
    bio_ss.load_code(bm_dma_coincident_code(), 0, BioCore::Core3);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    bio_ss.set_core_run_states([true, true, true, true]);
    //bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0xFFF);

    let mut main_mem_src: [u32; TEST_LEN] = [0u32; TEST_LEN];
    let mut main_mem_dst: [u32; TEST_LEN] = [0u32; TEST_LEN];
    // just conjure some locations out of thin air
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 8192) as *mut u32, TEST_LEN) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 32768) as *mut u32, TEST_LEN) };
    ifram_src.fill(0);
    ifram_dst.fill(0);

    passing += coincident_u32(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, seed + 0x200, "m->m");
    println!("m->m coincident");
    // try other memory banks
    passing += coincident_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, seed + 0x300, "i0->m");
    println!("0->m");
    passing += coincident_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, seed + 0x400, "m->i1");
    println!("m->1");
    passing += coincident_u32(&mut bio_ss, ifram_src, ifram_dst, seed + 0x500, "i0->i1");
    println!("0->1");

    println!("DMA coincident done.");
    passing
}

fn coincident_u32(
    bio_ss: &mut BioSharedState,
    src: &mut [u32],
    dst: &mut [u32],
    seed: u32,
    name: &'static str,
) -> usize {
    assert!(src.len() == dst.len());
    let mut tp = TestPattern::new(Some(seed));
    for d in src.iter_mut() {
        *d = tp.next();
    }
    for d in dst.iter_mut() {
        *d = tp.next();
    }
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, src.as_ptr() as u32); // src address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, dst.as_ptr() as u32); // dst address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, (src.len() * size_of::<u32>()) as u32 / 4); // bytes to move
    while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0xF_0000 != 0xF_0000 {} // one bit per core is set on completion of bits 8:12
    cache_flush();
    let mut pass = 1;
    let mut errs = 0;
    for (i, &d) in src.iter().enumerate() {
        let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
        if rbk != d {
            if errs < 4 {
                println!("{} err @{}, {:x} rbk: {:x}", name, i, d, rbk);
            } else {
                print!(".");
            }
            errs += 1;
            pass = 0;
        }
    }
    if errs >= 4 {
        println!("");
    }
    pass
}

#[rustfmt::skip]
bio_code!(bm_dma_coincident_code, BM_DMA_COINCIDENT_START, BM_DMA_COINCIDENT_END,
    "srli t0, x31, 30", // extract the core number
    "slli t0, t0, 2",   // multiply by 4
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "li x29, 0xF0000",    // clear the completion bits
    "mv a1, x16",       // wait for # of bytes to move / 4
    "add a3, t0, a3",   // offset by core index
    "add a2, t0, a2",
    "slli a1, a1, 2",   // shift end condition
    "add a4, a1, a3",   // a4 <- end condition based on source address increment
  "30:",
    "lw  t1, 0(a3)",    // blocks until load responds
    "sw  t1, 0(a2)",    // blocks until store completes
    "addi a3, a3, 16",  // 3 cycles
    "addi a2, a2, 16",  // 3 cycles
    "blt  a3, a4, 30b", // 5 cycles
    "srli t2, x31, 30", // extract the core number
    "addi t2, t2, 16",  // bit position to flip is 16 + core number
    "li   t1, 1",
    "sll  x28, t1, t2", // set the bit corresponding to the core number
    "j 20b"
);
