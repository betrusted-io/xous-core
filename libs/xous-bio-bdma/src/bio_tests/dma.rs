use core::mem::size_of;

use crate::*;

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

#[inline(always)]
fn cache_flush() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
        ".word 0x500F",
        "nop",
        "nop",
        "nop",
        "nop",
        "nop",
    );
    }
}

/// Single-core, simple bus copy between main memory and peripheral space segments
/// Less efficient than multi-core implementation, but uses less cores
///
/// The `concurrent` flag causes the CPU to do concurrent traffic during the test to
/// exercise contention on AXI bus.
pub fn dma_basic(concurrent: bool) -> usize {
    const TEST_LEN: usize = 64;
    let mut passing = 0;
    if !concurrent {
        print!("DMA basic\r");
    } else {
        print!("DMA cpu+dma concurrent\r");
    }
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_basic_code(), 0, BioCore::Core0);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x111);

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
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, 0, "Main->main", concurrent);

    main_mem_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x40, "ifram0->main", concurrent);

    ifram_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x80, "Main->ifram1", concurrent);

    main_mem_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, ifram_dst, 0xC0, "ifram0->ifram1", concurrent);

    if !concurrent {
        print!("DMA basic done.\r");
    } else {
        print!("DMA cpu+dma concurrent done.\r");
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
        for (i, &d) in src.iter().enumerate() {
            let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
            if rbk != d {
                print!("{} DMA err @{}, {:x} rbk: {:x}\r", name, i, d, rbk);
                pass = 0;
            }
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
                print!("(c) {} DMA err @{}, {:x} rbk: {:x}\r", name, i, d, rbk);
                pass = 0;
            }
        }
    }
    pass
}

#[rustfmt::skip]
bio_code!(dma_basic_code, DMA_BASIC_START, DMA_BASIC_END,
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "li x29, 0x1",      // clear event done flag - just before the last parameter arrives
    "mv a1, x16",       // wait for # of bytes to move

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lw  t0, 0(a3)",    // blocks until load responds
    "sw  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 4",   // 3 cycles
    "addi a2, a2, 4",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
    "li x28, 0x1",      // flip event done flag
    "j 20b"
);

// test byte-wide modifications
pub fn dma_bytes() -> usize {
    let mut passing = 0;
    print!("DMA bytes\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_bytes_code(), 0, BioCore::Core0);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x111);

    let mut main_mem_src: [u8; 64] = [0u8; 64];
    let mut main_mem_dst: [u8; 64] = [0u8; 64];
    // just conjure some locations out of thin air. Yes, these are weird addresses in decimal, meant to
    // just poke into some not page aligned location in IFRAM.
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 0x2468) as *mut u8, 64) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 0x1234) as *mut u8, 64) };
    ifram_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u8(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, 0x8800, "Main->main");
    print!("m->m\r");

    main_mem_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u8(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x8840, "ifram0->main");
    print!("0->m\r");

    ifram_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u8(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x8880, "Main->ifram1");
    print!("m->1\r");

    main_mem_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u8(&mut bio_ss, ifram_src, ifram_dst, 0x88C0, "ifram0->ifram1");
    print!("0->1\r");

    print!("DMA bytes done.\r");
    passing
}

fn basic_u8(
    bio_ss: &mut BioSharedState,
    src: &mut [u8],
    dst: &mut [u8],
    seed: u32,
    name: &'static str,
) -> usize {
    assert!(src.len() == dst.len());
    let mut tp = TestPattern::new(Some(seed));
    for d in src.chunks_mut(4) {
        d.copy_from_slice(&tp.next().to_le_bytes());
    }
    for d in dst.chunks_mut(4) {
        d.copy_from_slice(&tp.next().to_le_bytes());
    }
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, src.as_ptr() as u32); // src address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, dst.as_ptr() as u32); // dst address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, src.len() as u32); // bytes to move
    while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0x20 == 0 {}
    cache_flush();
    let mut pass = 1;
    for (i, &d) in src.iter().enumerate() {
        let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
        if rbk != d {
            print!("{} DMA err @{}, {:x} rbk: {:x}\r", name, i, d, rbk);
            pass = 0;
        }
    }
    pass
}

#[rustfmt::skip]
bio_code!(dma_bytes_code, DMA_BYTES_START, DMA_BYTES_END,
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "li x29, 0x20",     // clear event done flag - just before the last parameter arrives
    "mv a1, x16",       // wait for # of bytes to move

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lb  t0, 0(a3)",    // blocks until load responds
    "sb  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 1",   // 3 cycles
    "addi a2, a2, 1",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
    "li x28, 0x20",     // flip event done flag
    "j 20b"
);

// test half-word modifications
pub fn dma_u16() -> usize {
    let mut passing = 0;
    print!("DMA u16\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_u16_code(), 0, BioCore::Core3);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x888);

    let mut main_mem_src: [u16; 64] = [0u16; 64];
    let mut main_mem_dst: [u16; 64] = [0u16; 64];
    // just conjure some locations out of thin air. Yes, these are weird addresses in decimal, meant to
    // just poke into some not page aligned location in IFRAM.
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 0x3452) as *mut u16, 64) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 0x3452) as *mut u16, 64) };
    ifram_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u16(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, 0x1600, "Main->main");
    main_mem_src.fill(0);
    main_mem_dst.fill(0);

    passing += basic_u16(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x1640, "ifram0->main");
    ifram_src.fill(0);
    main_mem_dst.fill(0);

    passing += basic_u16(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x1680, "Main->ifram1");
    main_mem_src.fill(0);
    ifram_dst.fill(0);

    passing += basic_u16(&mut bio_ss, ifram_src, ifram_dst, 0x16C0, "ifram0->ifram1");
    print!("DMA u16 done.\r");
    passing
}

fn basic_u16(
    bio_ss: &mut BioSharedState,
    src: &mut [u16],
    dst: &mut [u16],
    seed: u32,
    name: &'static str,
) -> usize {
    assert!(src.len() == dst.len());
    let mut tp = TestPattern::new(Some(seed));
    for d in src.chunks_mut(2) {
        let w = tp.next().to_le_bytes();
        d.copy_from_slice(&[
            u16::from_le_bytes(w[..2].try_into().unwrap()),
            u16::from_le_bytes(w[2..].try_into().unwrap()),
        ]);
    }
    for d in dst.chunks_mut(2) {
        let w = tp.next().to_le_bytes();
        d.copy_from_slice(&[
            u16::from_le_bytes(w[..2].try_into().unwrap()),
            u16::from_le_bytes(w[2..].try_into().unwrap()),
        ]);
    }
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, src.as_ptr() as u32); // src address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, dst.as_ptr() as u32); // dst address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, (src.len() * size_of::<u16>()) as u32); // bytes to move
    while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0x00F0_0000 != 0x00A0_0000 {} // multi-bit patterns, at high end of valid range
    cache_flush();
    let mut pass = 1;
    for (i, &d) in src.iter().enumerate() {
        let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
        if rbk != d {
            print!("{} DMA err @{}, {:x} rbk: {:x}\r", name, i, d, rbk);
            pass = 0;
        }
    }
    pass
}

#[rustfmt::skip]
bio_code!(dma_u16_code, DMA_U16_START, DMA_U16_END,
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "mv a1, x16",       // wait for # of bytes to move
    "li x29, 0xA00000", // clear event done flag - just before the last parameter arrives
    "li x28, 0x200000", // partial flip of event done state
    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lh  t0, 0(a3)",    // blocks until load responds
    "sh  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 2",   // 3 cycles
    "addi a2, a2, 2",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
    "li x28, 0x800000", // flip event done flag
    "j 20b"
);

/// Multi-core DMA copy. More performant, but uses two cores for address generation in
/// parallel with the copy master.
pub fn dma_multicore() -> usize {
    let mut passing = 0;
    print!("DMA fast\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();

    // reset all the fifos
    bio_ss.bio.wo(utra::bio_bdma::SFR_FIFO_CLR, 0xF);

    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_mc_copy_code(), 0, BioCore::Core2);
    bio_ss.load_code(dma_mc_src_addr_code(), 0, BioCore::Core0);
    bio_ss.load_code(dma_mc_dst_addr_code(), 0, BioCore::Core1);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x777);

    let mut tp = TestPattern::new(Some(0x100));
    let mut main_mem_src: [u32; 16] = [0u32; 16];
    let mut main_mem_dst: [u32; 16] = [0u32; 16];
    for d in main_mem_src.iter_mut() {
        *d = tp.next();
    }
    for d in main_mem_dst.iter_mut() {
        *d = tp.next();
    }
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, main_mem_src.as_ptr() as u32); // src address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF2, (main_mem_src.len() * size_of::<u32>()) as u32); // bytes to move
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF3, main_mem_dst.as_ptr() as u32); // dst address
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF3, (main_mem_src.len() * size_of::<u32>()) as u32); // bytes to move
    while bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & 0xF00 != 0x500 {} // trying some creative bit patterns
    // wait for the fifo to clear, which means all copies are done
    while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) != 0 {}
    cache_flush();
    let mut pass = 1;
    for (i, &d) in main_mem_src.iter().enumerate() {
        let rbk = unsafe { main_mem_dst.as_ptr().add(i).read_volatile() };
        if rbk != d {
            print!("Main mem (fast loop) DMA err @{}, {:x} rbk: {:x}\r", i, d, rbk);
            pass = 0;
        }
    }
    passing += pass;

    print!("DMA fast loop done.\r");
    passing
}

#[rustfmt::skip]
bio_code!(dma_mc_copy_code, DMA_MC_COPY_START, DMA_MC_COPY_END,
  "20:",
    "lw a0, 0(x16)", // unrolled for more performance
    "sw a0, 0(x17)",
    "lw a0, 0(x16)",
    "sw a0, 0(x17)",
    "lw a0, 0(x16)",
    "sw a0, 0(x17)",
    "lw a0, 0(x16)",
    "sw a0, 0(x17)",
"j 20b"
);

#[rustfmt::skip]
bio_code!(dma_mc_src_addr_code, DMA_MC_SRC_ADDR_START, DMA_MC_SRC_ADDR_END,
  "20:",
    "mv a0, x18",  // src address on FIFO x18
    "li x29, 0x500", // clear done state
    "li x28, 0x400", // partial done
    "mv a1, x18",  // # bytes to copy on FIFO x18
    "add a2, a1, a0",
  "21:",
    "mv x16, a0",
    "addi a0, a0, 4",
    "bne a0, a2, 21b",
    "j 20b"
);

#[rustfmt::skip]
bio_code!(dma_mc_dst_addr_code, DMA_MC_DST_ADDR_START, DMA_MC_DST_ADDR_END,
  "20:",
    "mv a0, x19",  // dst address on FIFO x19
    "mv a1, x19",  // # bytes to copy on FIFO x19
    "add a2, a1, a0",
  "21:",
    "mv x17, a0",
    "addi a0, a0, 4",
    "bne a0, a2, 21b",
    "li x28, 0x500", // finish the done criteria in this core
    "j 20b"
);

/// Attempt to fire off all four engines at once, simultaneously, for maximum bus contention
pub fn dma_coincident() -> usize {
    let mut passing = 0;
    print!("DMA coincident\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // reset all the fifos
    bio_ss.bio.wo(utra::bio_bdma::SFR_FIFO_CLR, 0xF);

    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_coincident_code(), 0, BioCore::Core0);
    bio_ss.load_code(dma_coincident_code(), 0, BioCore::Core1);
    bio_ss.load_code(dma_coincident_code(), 0, BioCore::Core2);
    bio_ss.load_code(dma_coincident_code(), 0, BioCore::Core3);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0xFFF);

    let mut main_mem_src: [u32; 16 * 4] = [0u32; 16 * 4];
    let mut main_mem_dst: [u32; 16 * 4] = [0u32; 16 * 4];
    // just conjure some locations out of thin air
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 8192) as *mut u32, 16 * 4) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 32768) as *mut u32, 16 * 4) };
    ifram_src.fill(0);
    ifram_dst.fill(0);

    passing += coincident_u32(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, 0x200, "DMA coincident");
    print!("m->m\r");
    // try other memory banks
    passing += coincident_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x300, "ifram0->main");
    print!("0->m\r");
    passing += coincident_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x400, "main->ifram1");
    print!("m->1\r");
    passing += coincident_u32(&mut bio_ss, ifram_src, ifram_dst, 0x500, "ifram0->ifram1");
    print!("0->1\r");

    print!("DMA coincident done.\r");
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
    for (i, &d) in src.iter().enumerate() {
        let rbk = unsafe { dst.as_ptr().add(i).read_volatile() };
        if rbk != d {
            print!("{} err @{}, {:x} rbk: {:x}\r", name, i, d, rbk);
            pass = 0;
        }
    }
    pass
}

#[rustfmt::skip]
bio_code!(dma_coincident_code, DMA_COINCIDENT_START, DMA_COINCIDENT_END,
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

/// This test configures the irq-out lines to generate a pattern of interrupts by one core, that
/// are then fed back into the core via the dmareq lines. This interrupt pattern is
/// picked up by another core, and cleared. If the core is unable to read the mask bits
/// or if the request doesn't go through, the test will fail.
pub fn dmareq_test() -> usize {
    const BIO_IRQ_BASE: usize = 83 + 64; // event bit where our irqs are mapped to
    const BIO_IRQ_OFFSET: usize = 16; // subtract this offset to get the actual bit location

    let mut passing = 1;
    print!("DMA requests\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(dma_request_code(), 0, BioCore::Core0);
    bio_ss.load_code(dma_response_code(), 0, BioCore::Core1);

    // make sure the events are cleared
    bio_ss.bio.wo(utra::bio_bdma::SFR_EVENT_CLR, 0xFFFF_FFFF);
    // map irq requests to event set lines [3:0]
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_0, 0x1);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_1, 0x2);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_2, 0x4);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_3, 0x8);

    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
    // start the machines
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x333);

    // map DMA req lines corresponding to our irq lines
    // determine what bank and bit to set
    let bank = (BIO_IRQ_BASE - BIO_IRQ_OFFSET) / 32;
    let bit = (BIO_IRQ_BASE - BIO_IRQ_OFFSET) - bank * 32;
    // activate the event mapping
    unsafe {
        bio_ss
            .bio
            .base()
            .add(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5.offset() - bank)
            .write_volatile(0xF << bit)
    };

    let dmareq_bit = (BIO_IRQ_BASE - BIO_IRQ_OFFSET) / 8;
    print!("bank: {} bit: {} dmareq_bit {}\r", bank, bit, dmareq_bit);

    // check that our whole range maps to just one dmareq bit
    assert!(dmareq_bit == (BIO_IRQ_BASE - BIO_IRQ_OFFSET + 4) / 8);
    // send the sensitivity bit to the responder
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, (1 << dmareq_bit) as u32);
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, unsafe {
        // send the register to check for the bit mask
        bio_ss.bio.base().add(utra::bio_bdma::SFR_DMAREQ_STAT_SR_EVSTAT5.offset() - bank)
    } as u32);
    // send the bit offset to look for
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF1, bit as u32);

    for i in 1..15 {
        // send the bit pattern into the event-set via core 0
        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, i);

        let mut timeout = 0;
        while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) == 0 {
            timeout += 1;
            if timeout > 200 {
                print!("Timeout hit on {}", i);
                passing = 0;
                break;
            }
        }
        let result = bio_ss.bio.r(utra::bio_bdma::SFR_RXF2);
        if result != i {
            print!("Event expected {}, got {}\r", i, result);
            passing = 0;
        }
    }

    // unmap the irqs
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_0, 0x0);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_1, 0x0);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_2, 0x0);
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_3, 0x0);
    // unmap event mapping
    for i in 0..6 {
        unsafe {
            bio_ss.bio.base().add(utra::bio_bdma::SFR_DMAREQ_MAP_CR_EVMAP5.offset() - i).write_volatile(0)
        };
    }

    print!("DMA requests done.\r");
    passing
}

#[rustfmt::skip]
bio_code!(dma_request_code, DMA_REQUEST_START, DMA_REQUEST_END,
  "20:",
    "mv x28, x16",      // reflect FIFO0 into the event-set register
    "j 20b"
);

#[rustfmt::skip]
bio_code!(dma_response_code, DMA_RESPONSE_START, DMA_RESPONSE_END,
    "mv a0, x17",       // sensitivity bit
    "mv a1, x17",       // address to check for full status
    "mv a2, x17",       // bit shift for status
    "mv x27, a0",       // set sensitivity to bit as arriving on FIFO1
  "20:",
    "mv t0, x30",       // wait till an event bit is set
    "lw t1, 0(a1)",     // read the corresponding register that has our event bitmask
    "srl s0, t1, a2",   // shift the register to the right so the interesting bits are at 3:0
    "li t2, 0xF",
    "and t1, t2, s0",   // mask the result
    "mv x29, t1",       // clear the interrupt condition

    "li t0, 0",         // wait 64 cycles for the interrupt clear condition to propagate through the system
    "li t1, 64",
  "21:",
    "addi t0, t0, 1",
    "blt t0, t1, 21b",

    "mv x29, a0",       // clear the incoming event bit
    "mv x18, s0",       // send the result back for checking
  "j 20b"
);
