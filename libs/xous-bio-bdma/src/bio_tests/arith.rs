use crate::*;

pub fn stack_test() -> usize {
    println!("Stack test");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(stack_test0_code(), 0, BioCore::Core0);
    bio_ss.load_code(stack_test1_code(), 0, BioCore::Core1);
    bio_ss.load_code(stack_test2_code(), 0, BioCore::Core3);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x10_0000);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x5_8000);
    // bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000); // this machine not used in this test
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0xBBB);

    // pass two values in
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, 3);
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, 5);
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, 4);
    // wait for the computation to return
    let mut timeout = 0;
    const TIMEOUT: usize = 100_000;
    while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {
        timeout += 1;
        if timeout > TIMEOUT {
            println!("Timed out waiting for result (#1)");
            return 0;
        }
    }
    let result = bio_ss.bio.r(utra::bio_bdma::SFR_RXF3);
    let check = test_sum(test_sum(test_sum(3)));
    println!("Got {}", result);
    if result != check {
        println!("Computed {}, should be {}", result, check);
        0
    } else {
        timeout = 0;
        while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {
            timeout += 1;
            if timeout > TIMEOUT {
                println!("Timed out waiting for result (#2)");
                return 0;
            }
        }
        let result = bio_ss.bio.r(utra::bio_bdma::SFR_RXF3);
        let check = test_sum(test_sum(test_sum(5)));
        println!("Got {}", result);
        if result != check {
            println!("Computed {}, should be {}", result, check);
            0
        } else {
            timeout = 0;
            while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {
                timeout += 1;
                if timeout > TIMEOUT {
                    println!("Timed out waiting for result (#3)");
                    return 0;
                }
            }
            let result = bio_ss.bio.r(utra::bio_bdma::SFR_RXF3);
            let check = test_sum(test_sum(test_sum(4)));
            println!("Got {}", result);
            if result != check {
                println!("Computed {}, should be {}", result, check);
                0
            } else {
                println!("===stack test PASS===");
                1
            }
        }
    }
}

/// the recursive function implemented below
fn test_sum(a: u32) -> u32 { if a == 0 { 0 } else { a + test_sum(a - 1) } }

// the recursive function above, written out in RV32 assembly and BIO entry-points
#[rustfmt::skip]
bio_code!(stack_test0_code, STACK_TEST0_START, STACK_TEST0_END,
  // number to sum to comes into x16
  // compute sum = N + N-1 + N-2 + ... 0
  "20:",
    "mv   a0, x16", // get the argument
    "jal  ra, 30f",
    "mv   x17, a0", // return the value
    "j    20b", // go back for more
  "30:",
    "addi sp, sp, -8",
    "sw   ra, 4(sp)",
    "sw   a0, 0(sp)",
    "bne  a0, x0, 40f", // recurse
    "add  sp, sp, 8",
    "ret",              // if a0=0, return a0=0
  "40:",
    "addi a0, a0, -1",
    "jal  ra, 30b",
    "lw   t0, 0(sp)",
    "add  a0, t0, a0",
    "lw   ra, 4(sp)",
    "add  sp, sp, 8",
    "ret"
);
// cascade another computation on the previous result
#[rustfmt::skip]
bio_code!(stack_test1_code, STACK_TEST1_START, STACK_TEST1_END,
  // number to sum to comes into x16
  // compute sum = N + N-1 + N-2 + ... 0
  "20:",
    "mv   a0, x17", // get the argument
    "jal  ra, 30f",
    "mv   x18, a0", // return the value
    "j    20b", // go back for more
  "30:",
    "addi sp, sp, -8",
    "sw   ra, 4(sp)",
    "sw   a0, 0(sp)",
    "bne  a0, x0, 40f", // recurse
    "add  sp, sp, 8",
    "ret",              // if a0=0, return a0=0
  "40:",
    "addi a0, a0, -1",
    "jal  ra, 30b",
    "lw   t0, 0(sp)",
    "add  a0, t0, a0",
    "lw   ra, 4(sp)",
    "add  sp, sp, 8",
    "ret"
);
// ...and one more time
#[rustfmt::skip]
bio_code!(stack_test2_code, STACK_TEST2_START, STACK_TEST2_END,
  // number to sum to comes into x16
  // compute sum = N + N-1 + N-2 + ... 0
  "20:",
    "mv   a0, x18", // get the argument
    "jal  ra, 30f",
    "mv   x19, a0", // return the value
    "j    20b", // go back for more
  "30:",
    "addi sp, sp, -8",
    "sw   ra, 4(sp)",
    "sw   a0, 0(sp)",
    "bne  a0, x0, 40f", // recurse
    "add  sp, sp, 8",
    "ret",              // if a0=0, return a0=0
  "40:",
    "addi a0, a0, -1",
    "jal  ra, 30b",
    "lw   t0, 0(sp)",
    "add  a0, t0, a0",
    "lw   ra, 4(sp)",
    "add  sp, sp, 8",
    "ret"
);

#[cfg(feature = "bio-mul")]
/// Safety: Only safe if the target BIO has the multiply extension
pub unsafe fn mac_test() -> usize {
    println!("MAC test");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(mac_code(), 0, BioCore::Core0);

    // These actually "don't matter" because there are no synchronization instructions in the code
    // Everything runs at "full tilt"
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x0);
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x111);

    // a test vector
    let a: [i32; 7] = [1, -1, 3047, 0, 12, -92101, 12432];
    let b = 7;
    let check = mac(&a, b);

    // pass values to fifo; fifo is 8-deep, so this should fit without checking depth
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, a.len() as u32);
    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, b as u32);
    for v in a.iter() {
        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, *v as u32);
    }
    // wait for the computation to return
    while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {}
    let result = bio_ss.bio.r(utra::bio_bdma::SFR_RXF1) as i32;
    println!("Got {}", result);
    if result != check {
        println!("Computed {}, should be {}", result, check);
        println!("===MAC test FAIL===");
        0
    } else {
        println!("===MAC test PASS===");
        1
    }
}

/// the recursive function implemented below
#[cfg(feature = "bio-mul")]
fn mac(a: &[i32], b: i32) -> i32 {
    let mut r: i32 = 0;
    for &v in a.iter() {
        r += v * b;
    }
    r
}

#[rustfmt::skip]
#[cfg(feature = "bio-mul")]
bio_code!(mac_code, MAC_START, MAC_END,
  // first arg into x16 is the number of elements to MAC
  // second arg is the coefficient
  // remaining args are the vector
  // compute mac = a0 * b + a1 * b + a2 * b ...
  // return value is in x17
  "20:",
    "mv   a0, x16", // number of elements
    "mv   a1, x16", // coefficient
    "mv   a2, x0",  // initialize return value
    "jal  ra, 30f",
    "mv   x17, a2", // return the value
    "j    20b", // go back for more
  "30:",
    "bne  x0, a0, 31f", // test if end
    "ret",
  "31:",
    "addi a0, a0, -1",  // decrement arg counter
    "mv t1, x16",       // fetch vector value: note, we can't multiply directly from a FIFO because while the multi-cycle multiply runs, the FIFO keeps draining
    // "mul  t0, a1, t1", // multiply
    // mul x5, x11, x6, same as above. hand-assembled because Rust refuses to emit `mul` instructions for global_asm!
    ".word 0x026582b3",
    "add  a2, t0, a2",  // accumulate
    "j    30b"          // loop
);
