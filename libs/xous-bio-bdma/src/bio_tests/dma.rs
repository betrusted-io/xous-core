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
pub fn dma_basic() -> usize {
    let mut passing = 0;
    print!("DMA basic\r");
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

    let mut main_mem_src: [u32; 16] = [0u32; 16];
    let mut main_mem_dst: [u32; 16] = [0u32; 16];
    // just conjure some locations out of thin air. Yes, these are weird addresses in decimal, meant to
    // just poke into some not page aligned location in IFRAM.
    let ifram_src =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 8200) as *mut u32, 16) };
    let ifram_dst =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM1_MEM + 10000) as *mut u32, 16) };
    ifram_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, &mut main_mem_dst, 0, "Main->main");

    main_mem_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x40, "ifram0->main");

    ifram_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x80, "Main->ifram1");

    main_mem_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u32(&mut bio_ss, ifram_src, ifram_dst, 0xC0, "ifram0->ifram1");

    print!("DMA basic done.\r");
    passing
}

fn basic_u32(
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
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, (src.len() * size_of::<u32>()) as u32); // bytes to move
    print!("{} copy delay\r", name);
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
bio_code!(dma_basic_code, DMA_BASIC_START, DMA_BASIC_END,
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
    "mv a1, x16",       // wait for # of bytes to move

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lw  t0, 0(a3)",    // blocks until load responds
    "sw  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 4",   // 3 cycles
    "addi a2, a2, 4",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
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

    main_mem_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u8(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x8840, "ifram0->main");

    ifram_src.fill(0);
    main_mem_dst.fill(0);
    passing += basic_u8(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x8880, "Main->ifram1");

    main_mem_src.fill(0);
    ifram_dst.fill(0);
    passing += basic_u8(&mut bio_ss, ifram_src, ifram_dst, 0x88C0, "ifram0->ifram1");

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
    print!("{} copy delay\r", name);
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
    "mv a1, x16",       // wait for # of bytes to move

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lb  t0, 0(a3)",    // blocks until load responds
    "sb  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 1",   // 3 cycles
    "addi a2, a2, 1",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
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
    print!("{} copy delay\r", name);
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

    "add a4, a1, a3",   // a4 <- end condition based on source address increment

  "30:",
    "lh  t0, 0(a3)",    // blocks until load responds
    "sh  t0, 0(a2)",    // blocks until store completes
    "addi a3, a3, 2",   // 3 cycles
    "addi a2, a2, 2",   // 3 cycles
    "bne  a3, a4, 30b", // 5 cycles
    "j 20b"
);

/// Multi-core DMA copy. More performant, but uses two cores for address generation in
/// parallel with the copy master.
pub fn dma_multicore() -> usize {
    let mut passing = 0;
    print!("DMA basic\r");
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
    print!("Main memory copy delay (fast loop)\r");
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

    // try other memory banks
    passing += coincident_u32(&mut bio_ss, ifram_src, &mut main_mem_dst, 0x300, "ifram0->main");
    passing += coincident_u32(&mut bio_ss, &mut main_mem_src, ifram_dst, 0x400, "main->ifram1");
    passing += coincident_u32(&mut bio_ss, ifram_src, ifram_dst, 0x500, "ifram0->ifram1");

    print!("DMA coincident done\r");
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
    print!("{} copy delay\r", name);
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
    "srli t0, x31, 30", // extract the offset
    "slli t0, t0, 2",   // multiply by 4
  "20:",
    "mv a3, x18",       // src address
    "mv a2, x17",       // dst address
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
    "j 20b"
);
