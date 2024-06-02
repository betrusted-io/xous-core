use utralib::utra::bio::{
    SFR_ETYPE_FIFO_EVENT_EQ_MASK, SFR_ETYPE_FIFO_EVENT_GT_MASK, SFR_ETYPE_FIFO_EVENT_LT_MASK,
};

use super::TEST_INVERT_MASK;
use crate::*;

// this test requires manual inspection of the outputs
// the GPIO pins should toggle with 0x11, 0x12, 0x13...
// at the specified quantum rate of the machine.
pub fn hello_world() {
    print!("hello world test\r");
    let mut bio_ss = BioSharedState::new();
    let simple_test_code = hello_world_code();
    // copy code to reset vector for 0th machine
    bio_ss.load_code(simple_test_code, 0);

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x20_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x111);
    print!("===hello world PASS===\r");
}
#[rustfmt::skip]
bio_code!(hello_world_code, HELLO_START, HELLO_END,
    "add  x1, zero, 0x10",
    "0:",
    "add  x1, x1, 0x1",
    "mv   x21, x1",
    "mv   x20, zero",
    "j 0b",
    "nop"
);

// this test requires manual inspection of the outputs
// the GPIO pins should toggle with the following pattern:
// 0x41312111, 0x42322212, 0x43332313, etc.
// and they should be in sync-lock, no ragged transitions
pub fn hello_multiverse() {
    print!("multiverse\r");
    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = multiverse_code();
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV2, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV3, 0x20_0000);
    // snap GPIO outputs to the quantum
    bio_ss.bio.wo(
        utra::bio::SFR_CONFIG,
        bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, 1)
            | bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, 1), /* arbitrary choice, they
                                                                               * should all be the same */
    );
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xfff);
    print!("===multiverse PASS===\r");
}
#[rustfmt::skip]
bio_code!(multiverse_code, MULTIVERSE_START, MULTIVERSE_END,
    // Reset vectors for each core are aligned to 4-byte boundaries
    // As long as the jump target is <2kiB from reset, this will emit
    // a C-instruction, so it needs padding with a NOP. Unfortunately,
    // I can't seem to figure out a way to force the assembler to always
    // encode as uncompressed, so, you have to be aware of the jump destination
    // for the assembler output to line up according to your expectation :(
    //
    // using 'c.j' syntax for the jump causes the assembler to emit an error,
    // but the code still compiles, so...avoiding that for now. might be a bug,
    // but I am very not interested in fixing that today.
    //
    // Also note that labels can only be numbers from 0-99, and, due to an llvm
    // bug, labels made exclusively of 0 or 1 should be avoided because they get
    // interpreted as binary numbers. dat's some jank in the tank!!
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
    // mach 0 code
    "90:",
    // x26 sets the GPIO mask
    "li   x2, 0xFF",    // load constants into r0-15 bank first
    "mv   x26, x2",     // it's not legal to do anything other than mv to x26
    "add  x1, zero, 0x10",
    "4:",
    "add  x1, x1, 0x1",
    // x21 write clobbers the GPIO bits, ANDed with mask in x26
    "mv   x21, x1",
    // x20 write causes core to wait until next sync quantum
    "mv   x20, zero",
    "j 4b",
    // mach 1 code
    "91:",
    "li   x2, 0xFF00",
    "mv   x26, x2",
    "add  x1, zero, 0x20",
    "5:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 8",
    "mv   x20, zero",
    "j 5b",
    // mach 2 code
    "92:",
    "li   x2, 0xFF0000",
    "mv   x26, x2",
    "add  x1, zero, 0x30",
    "6:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 16",
    "mv   x20, zero",
    "j 6b",
    // mach 3 code
    "93:",
    "li   x2, 0xFF000000",
    "mv   x26, x2",
    "add  x1, zero, 0x40",
    "7:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 24",
    "mv   x20, zero",
    "j 7b"
);

// this test requires manual checking of gpio outputs
// GPIO pins should have the form 0x100n800m
// where n = 2*m. The output is not meant to be fully in sync,
// it will be "ragged" as the output snapping is not turned on.
// so 0x10008000, 0x10048002, 0x10088004, etc...
// but with a glitch before major transitions. The output could
// be sync'd locked, but we leave it off for this test so we have
// a demo of how things look when it's off.
pub fn fifo_basic() {
    print!("FIFO basic\r");
    // clear any prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = fifo_basic_code();
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    // / 16. clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x23_BE00);
    bio_ss.bio.wo(utra::bio::SFR_QDIV3, 0x23_BE00);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x33_1200);
    bio_ss.bio.wo(utra::bio::SFR_QDIV2, 0x33_1200);
    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio::SFR_CONFIG, 0);
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xfff);
    print!("===FIFO basic PASS===\r");
}
#[rustfmt::skip]
bio_code!(fifo_basic_code, FIFO_BASIC_START, FIFO_BASIC_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
    // mach 0 code
    "90:",
    "li x2, 0xFFFF",
    "mv x26, x2",
    "li x1, 0x10000000",
    "11:",
    "mv x16, x1",
    "mv x21, x17",
    // pass to mach 3 to update the loop counter
    "mv x19, x1",
    "mv x20, zero",
    "mv x1, x19",
    "j 11b",
    // mach 1 code
    "91:",
    "li x2, 0xFFFF0000",
    "mv x26, x2",
    "li x1, 0x8000",
    "21:",
    "mv x17, x1",
    "mv x21, x16",
    // pass to mach 2 to update the loop counter
    "mv x18, x1",
    "mv x20, zero",
    "mv x1, x18",
    "j 21b",
    // mach 2 code
    "92:",
    "addi x18, x18, 2", // increment the value in fifo by 2
    "mv x20, zero",
    "j 92b",
    // mach 3 code
    "93:",
    "li x2, 0x40000",
    "23:",
    "add x19, x19, x2", // increment the value in fifo by 0x4_0000
    "mv x20, zero",
    "j 23b",
    "nop"
);

// this test contains an automated check of the readback, but, in a nutshell,
// the test writes the values "0xF1F0_000m", where m is 0-15 to the GPIO output
// These outputs come from the *host*, and the test runs slow enough that host
// should stall waiting for device to propagate values at least once.
//
// Concurrently, another machine greedily reads the GPIO inputs, at a sync-locked
// rate relative to the outputs, until the FIFO is full and backpressure stops
// reads. The host then drains the FIFO, so some of the output indices should be
// skipped and only the final value is retrieved with future reads.
//
// GPIO outputs are run without snapping in this case, because there is just one
// machine updating outputs and no need to do that.
pub fn host_fifo_tests() {
    print!("Host FIFO tests\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = fifo_host_bitbang();
    bio_ss.load_code(code, 0);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio::SFR_FIFO_CLR, 0xF);

    // configure & run the 0th machine
    // clock it slowly, so the fifo builds up back pressure
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x400_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x400_0000);
    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio::SFR_CONFIG, 0);

    // invert readbacks via I/O
    test_cfg.wo(utra::main::WDATA, TEST_INVERT_MASK);

    // start cores 1 & 2
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x333);

    // clock some values into the bitbang fifo
    let mut stalled = false;
    for i in 0..16 {
        bio_ss.bio.wo(utra::bio::SFR_TXF0, i + 0xF1F0_0000);
        while bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) >= 8 {
            stalled = true;
        }
    }
    assert!(stalled);
    // wait for the write FIFO to drain
    while bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) != 0 {}

    // read back some fifo values, and check that back-pressure worked
    for i in 0..16 {
        let rbk = bio_ss.bio.r(utra::bio::SFR_RXF1);
        // we get indices 0-9: we can capture up to 8+1 entries before backpressure stops captures
        // and there is 1 extra value stuck in the CPU itself at the time of the stall.
        //
        // finally, we're pegged at 15, because, backpressure caused us to miss the rest of
        // the entries, and we are stuck at the final written value of the output test
        if i <= 9 {
            assert!(rbk == !(0xF1F0_0000 + i));
        } else {
            assert!(rbk == !(0xF1F0_0000 + 15));
        }
        print!("backpressure {:x}\r", rbk);
    }

    fn get_gpio_via_core(bio_ss: &mut BioSharedState) -> u32 {
        // get the GPIO value by triggering core 1 via event bit 1
        bio_ss.bio.wo(utra::bio::SFR_EVENT_SET, 1);
        while bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {
            // wait for the core to have reported the GPIO value
        }
        bio_ss.bio.r(utra::bio::SFR_RXF1)
    }

    // load next test
    // clear inversions, etc on readbacks via I/O
    test_cfg.wo(utra::main::WDATA, 0);
    // stop machine & load code
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = fifo_host_bitbang_level_trig();
    bio_ss.load_code(code, 0);

    // clear all events
    bio_ss.bio.wfo(utra::bio::SFR_EVENT_CLR_SFR_EVENT_CLR, 0xFFFF_FF);
    // set level trigger to 4
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL1, 4);
    // set polarities: >= 4 to flip
    bio_ss.bio.wfo(utra::bio::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0b00_00_00_10);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_LT_MASK, 0b00_00_00_00);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_GT_MASK, 0b00_00_00_10);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio::SFR_FIFO_CLR, 0xF);

    // start cores 1, 2, 3
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x777);

    // confirm the core booted
    let f1_val = get_gpio_via_core(&mut bio_ss);
    print!("core booted {:x}\r", f1_val);
    assert!(f1_val == 0xfeedface);
    // ensure fifo levels are where we think they are
    assert!(bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) == 0);
    assert!(bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0);
    assert!(bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) == 0);

    // put 7 items into the output data fifo
    let mut final_val: u32 = 0;
    for i in 0..7 {
        final_val = 0xf1f0_1000 + i;
        bio_ss.bio.wo(utra::bio::SFR_TXF0, final_val);
    }
    while bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) != 0 {
        // wait for fifo to drain
    }
    let pause_val = get_gpio_via_core(&mut bio_ss);
    print!("pause_val {:x}\r", pause_val);
    assert!(pause_val == final_val);
    // drop one more value in and confirm it appears
    let stop_val = 0xACE0_BACE;
    bio_ss.bio.wo(utra::bio::SFR_TXF0, stop_val);
    let stop_confirm_val = get_gpio_via_core(&mut bio_ss);
    print!("stop_confirm_val {:x}\r", stop_confirm_val);
    assert!(stop_val == stop_confirm_val);

    // fifo2 should have the entire log of all values in it. make sure that's the case
    for i in 0..7 {
        let f2_val = bio_ss.bio.r(utra::bio::SFR_RXF2);
        print!("f2_val {:x}\r", f2_val);
        assert!(f2_val == 0xf1f0_1000 + i);
    }
    let stop_check = bio_ss.bio.r(utra::bio::SFR_RXF2);
    print!("stop_check {:x}\r", stop_check);
    assert!(stop_check == stop_val);
    print!("===Host FIFO PASS===\r");
}
#[rustfmt::skip]
bio_code!(fifo_host_bitbang, FIFO_HOST_BITBANG_START, FIFO_HOST_BITBANG_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "90:",
    "mv x21, x16",
    "mv x20, zero",
    "j 90b",
    "91:",
    "mv x20, zero",
    "mv x17, x21",
    "j 91b"
);

// core 0:
//  - sets GPIO to 0xFEEDFACE on boot
//  - sets to trigger on fifo0 level, channel B
//  - waits for the fifo0 to meet its full-enough condition
//  - moves fifo content -> gpio as fast as possible
//  - notifies core 2 via event bit 2 every fifo move
//  - waits for ack on event bit 3
//  - restores fifo trigger mask
// core 1: should only capture the data on host command (so the buffered fifo entries will be missed)
//  - waits on event bit 1 (0x1 mask)
//  - takes gpio in and writes it to fifo1
// core 2: should capture *all* the data written to core 0
//  - waits on event bit 2 (0x2 mask)
//  - takes gpio in and writes it to fifo2
//  - acks core 0 on event bit 3
#[rustfmt::skip]
bio_code!(
    fifo_host_bitbang_level_trig,
    FIFO_HOST_BITBANG_LEVEL_TRIG_START,
    FIFO_HOST_BITBANG_LEVEL_TRIG_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "90:", // machine 0
    "li x2, 0xfeedface",
    "mv x21, x2",        // init gpio to the "i'm here" sentinel
    "li x1, 0x02000000", // event trigger fifo0, trigger channel B
    "li x5, 0x4",        // event mask for event bit 3
    "mv x27, x1",        // set event mask
    "mv x3, x30",        // wait on event trigger by reading x30 -- code will halt until fifo trigger condition is met
    "li x4, 0x2",        // trigger for channel used by machine 2
    "mv x27, x5",        // set event mask for the ack from machine 2, indicating gpio pin is sampled
    "20:",
    "mv x21, x16",       // fifo0 -> gpio out
    "mv x28, x4",        // set the bit that machine 2 is listening to
    "mv x3, x30",        // wait for ack that gpio was sampled
    "mv x29, x3",        // clear the trigger
    "j 20b",
    "91:", // machine 1
    "li x1, 0x1",        // event bit 0
    "mv x27, x1",        // set trigger mask
    "21:",
    "mv x2, x30",        // wait for event trigger
    "and x2, x2, x1",    // mask event result
    "mv x29, x2",        // clear the event trigger
    "mv x17, x21",       // gpio in -> fifo1
    "j 21b",
    "92:", // machine 2
    "li x2, 0x2",        // event bit 1
    "mv x27, x2",        // set trigger mask
    "li x4, 0x4",        // ack trigger mask
    "22:",
    "mv x3, x30",        // wait for event
    "and x3, x2, x2",    // mask event
    "mv x29, x3",        // clear trigger
    "mv x18, x21",       // gpio in -> fifo2
    "mv x28, x4",        // set event based on ack trigger mask (x4)
    "j 22b"
);

#[derive(Clone, Copy)]
struct FifoLevelTestConfig {
    tx_reg: crate::Register,
    rx_reg: crate::Register,
    levels: [crate::Field; 2],
    event_masks: [u32; 2],
}

// This can be done without any code running on the machines; the host can
// set and observe all fifo levels and triggers directly.
pub fn fifo_level_tests() {
    print!("FIFO level comprehensive\r");
    // clear prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    // let code = fifo_level_tests_code();
    // bio_ss.load_code(code, 0);

    // configure fifo trigger levels
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL0, 0);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL1, 9);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL2, 1);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL3, 8);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL4, 2);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL5, 7);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL6, 4);
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL7, 4);
    // configure the polarities
    bio_ss.bio.wfo(utra::bio::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0b11_00_11_11);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_LT_MASK, 0b11_01_01_00);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_GT_MASK, 0b10_10_10_00);

    /*
    The structure of the FIFO events is that there are two event level configurations
    per FIFO, structured as fifo [N] gets event level [N*2, N*2+1].

    Each event level could trigger on equals, less than, greater than, or any combination
    of the three.
     */
    let fifo_test_configs: [FifoLevelTestConfig; 4] = [
        FifoLevelTestConfig {
            tx_reg: utra::bio::SFR_TXF0,
            rx_reg: utra::bio::SFR_RXF0,
            levels: [utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL0, utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL1],
            event_masks: [0x1, 0x2],
        },
        FifoLevelTestConfig {
            tx_reg: utra::bio::SFR_TXF1,
            rx_reg: utra::bio::SFR_RXF1,
            levels: [utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL2, utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL3],
            event_masks: [0x4, 0x8],
        },
        FifoLevelTestConfig {
            tx_reg: utra::bio::SFR_TXF2,
            rx_reg: utra::bio::SFR_RXF2,
            levels: [utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL4, utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL5],
            event_masks: [0x10, 0x20],
        },
        FifoLevelTestConfig {
            tx_reg: utra::bio::SFR_TXF3,
            rx_reg: utra::bio::SFR_RXF3,
            levels: [utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL6, utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL7],
            event_masks: [0x40, 0x80],
        },
    ];
    const FIFO_MAX: u32 = 9;
    let mut rx_checks = [0u32; FIFO_MAX as usize];
    let mut tx_state = 0x1;
    let irq_masks: [Register; 4] = [
        utra::bio::SFR_IRQMASK_0,
        utra::bio::SFR_IRQMASK_1,
        utra::bio::SFR_IRQMASK_2,
        utra::bio::SFR_IRQMASK_3,
    ];
    let irqarray18 = CSR::new(utra::irqarray18::HW_IRQARRAY18_BASE as *mut u32);

    for (bank, config) in fifo_test_configs.iter().enumerate() {
        let irq_mask_reg = irq_masks[bank];
        let irq_mask = (1 << bank) as u32;
        print!("irq_mask {:x}\r", irq_mask);
        // we want to check that less than, equals, and greater than triggers work individually
        // then we want to check that lt+eq and gt+eq work together
        // lt+gt trigger doesn't make sense, we just don't check that
        for (&level, &mask) in config.levels.iter().zip(config.event_masks.iter()) {
            bio_ss.bio.wo(irq_mask_reg, mask << 24);
            // reset all the fifos
            bio_ss.bio.wo(utra::bio::SFR_FIFO_CLR, 0xF);
            for test_level in 0..FIFO_MAX {
                print!("test_level {:x} bank {:x}\r", test_level, bank);
                // test eq at level
                bio_ss.bio.wfo(level, test_level);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_EQ_MASK, mask);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_LT_MASK, 0);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_GT_MASK, 0);
                // fill
                for check_level in 0..FIFO_MAX {
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    // report_api(ev_check);
                    if check_level == test_level {
                        assert!(ev_check & mask == mask);
                        // report_api(irqarray18.r(utra::irqarray18::EV_STATUS));
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        // report_api(irqarray18.r(utra::irqarray18::EV_STATUS));
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                    bio_ss.bio.wo(config.tx_reg, tx_state);
                    // report_api(tx_state);
                    rx_checks[check_level as usize] = tx_state;
                    tx_state = crate::lfsr_next_u32(tx_state);
                }
                // drain
                // report_api(0xdddd_dddd);
                for check_level in 0..FIFO_MAX {
                    let rx = bio_ss.bio.r(config.rx_reg);
                    // report_api(rx);
                    assert!(rx == rx_checks[check_level as usize]);
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if FIFO_MAX - check_level - 1 == test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                }

                // test lt at level
                bio_ss.bio.wfo(level, test_level);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_LT_MASK, mask);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_GT_MASK, 0);
                // fill
                for check_level in 0..FIFO_MAX {
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if check_level < test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                    bio_ss.bio.wo(config.tx_reg, tx_state);
                    rx_checks[check_level as usize] = tx_state;
                    tx_state = crate::lfsr_next_u32(tx_state);
                }
                // drain
                for check_level in 0..FIFO_MAX {
                    let rx = bio_ss.bio.r(config.rx_reg);
                    assert!(rx == rx_checks[check_level as usize]);
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if FIFO_MAX - check_level - 1 < test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                }

                // test gt at level
                bio_ss.bio.wfo(level, test_level);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_LT_MASK, 0);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_GT_MASK, mask);
                // fill
                for check_level in 0..FIFO_MAX {
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if check_level > test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                    bio_ss.bio.wo(config.tx_reg, tx_state);
                    rx_checks[check_level as usize] = tx_state;
                    tx_state = crate::lfsr_next_u32(tx_state);
                }
                // drain
                for check_level in 0..FIFO_MAX {
                    let rx = bio_ss.bio.r(config.rx_reg);
                    assert!(rx == rx_checks[check_level as usize]);
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if FIFO_MAX - check_level - 1 > test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                }

                // test lt eq at level
                bio_ss.bio.wfo(level, test_level);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_EQ_MASK, mask);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_LT_MASK, mask);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_GT_MASK, 0);
                // fill
                for check_level in 0..FIFO_MAX {
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if check_level <= test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                    bio_ss.bio.wo(config.tx_reg, tx_state);
                    rx_checks[check_level as usize] = tx_state;
                    tx_state = crate::lfsr_next_u32(tx_state);
                }
                // drain
                for check_level in 0..FIFO_MAX {
                    let rx = bio_ss.bio.r(config.rx_reg);
                    assert!(rx == rx_checks[check_level as usize]);
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if FIFO_MAX - check_level - 1 <= test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                }

                // test gt eq at level
                bio_ss.bio.wfo(level, test_level);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_EQ_MASK, mask);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_LT_MASK, 0);
                bio_ss.bio.rmwf(SFR_ETYPE_FIFO_EVENT_GT_MASK, mask);
                // fill
                for check_level in 0..FIFO_MAX {
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if check_level >= test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                    bio_ss.bio.wo(config.tx_reg, tx_state);
                    rx_checks[check_level as usize] = tx_state;
                    tx_state = crate::lfsr_next_u32(tx_state);
                }
                // drain
                for check_level in 0..FIFO_MAX {
                    let rx = bio_ss.bio.r(config.rx_reg);
                    assert!(rx == rx_checks[check_level as usize]);
                    let ev_check = bio_ss.bio.r(utra::bio::SFR_EVENT_STATUS) >> 24;
                    if FIFO_MAX - check_level - 1 >= test_level {
                        assert!(ev_check & mask == mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask == irq_mask);
                    } else {
                        assert!(ev_check & mask != mask);
                        assert!(irqarray18.r(utra::irqarray18::EV_STATUS) & irq_mask != irq_mask);
                    }
                }
            }
            bio_ss.bio.wo(irq_mask_reg, 0);
        }
    }
    print!("===FIFO level comprehensive PASS===\r")
}

pub fn aclk_tests() {
    print!("ACLK test\r");
    // clear any prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = aclk_code();
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0xA_0000);
    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio::SFR_CONFIG, 0);

    // start machine 1
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x222);
    while bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) < 7 {
        print!("waiting {}\r", bio_ss.bio.rf(utra::bio::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1));
        // wait
    }
    let mut results = [0u32; 7];
    for d in results.iter_mut() {
        *d = bio_ss.bio.r(utra::bio::SFR_RXF1) & 0x3FFF_FFFF;
    }
    for (i, r) in results.iter().enumerate() {
        print!("{}: {} cycles\r", i, r);
    }
    assert!(results[1] - results[0] == 3);
    assert!(results[2] - results[1] == 3);
    assert!(results[3] - results[2] == 6);
    assert!(results[4] - results[3] == 3);

    assert!(results[6] - results[5] == 10); // related to the clock divider
    print!("===ACLK test PASS===\r");
}

#[rustfmt::skip]
bio_code!(aclk_code, ACLK_START, ACLK_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
    // mach 0 code
    "90:",
    "j 90b",
    // mach 1 code
    "91:",
    "mv x17, x31",
    "mv x17, x31",
    "mv x17, x31",
    "nop",
    "mv x17, x31",
    "mv x17, x31",
    "mv x20, x0",
    "mv x17, x31",
    "mv x20, x0",
    "mv x17, x31",
    "40:",
    "j 40b",
    // mach 2 code
    "92:",
    "j 92b",
    // mach 3 code
    "93:",
    "j 93b"
);
