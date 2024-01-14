use pio::RP2040_MAX_PROGRAM_SIZE;
use utralib::utra::rp_pio::{self, SFR_DBG_CFGINFO, SFR_FDEBUG, SFR_FLEVEL, SFR_FSTAT};

use super::report_api;
use crate::*;

/// Test the sticky out bits
pub fn sticky_test() {
    /* Test case from https://forums.raspberrypi.com/viewtopic.php?t=313962

    Reading the waveforms: bits 24-26 correspond to the number in the test result
    Bit 27 indicates A/B: 0 means A, 1 means B
    Bit 28 is the side-set enable bit

    No sticky, but B using enable bit
    Cycle 1 :   A writes A1                                    : result = A1
    Cycle 2:    A writes A2, B writes B2 (without enable bit)  : result = A2
    Cycle 3:    A writes A3, B writes B3 (with enable bit)     : result = B3
    Cycle 4:    A writes A4                                    : result = A4
    Cycle 5:    A writes A5, B writes B5 (with enable bit)     : result = B5
    Cycle 5:    B writes B6 (without enable bit)               : result = B5

    With sticky set on both state machines (i.e. it is as if you did the OUT write on every cycle)
    Cycle 1 :   A writes A1                                    : result = A1
    Cycle 2:    A writes A2, B writes B2 (without enable bit)  : result = A2
    Cycle 3:    A writes A3, B writes B3 (with enable bit)     : result = B3
    Cycle 4:    A writes A4  (B rewrites B3)                   : result = B3
    Cycle 5:    A writes A5, B writes B5 (with enable bit)     : result = B5
    Cycle 5:   (A rewrites A5), B writes B6 (without enable bit) : result = A5
     */
    report_api(0x51C2_0000);

    let mut pio_ss = PioSharedState::new();
    report_api(0x51C2_0001);
    pio_ss.clear_instruction_memory();
    report_api(0x51C2_0002);
    let mut sm_a = unsafe { pio_ss.force_alloc_sm(1).unwrap() };
    report_api(0x51C2_0003);
    let mut sm_b = unsafe { pio_ss.force_alloc_sm(2).unwrap() };
    report_api(0x51C2_0004);
    sm_a.sm_set_enabled(false);
    report_api(0x51C2_0005);
    sm_b.sm_set_enabled(false);
    report_api(0x51C2_0006);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "set pins, 1",
        "set pins, 2",
        "set pins, 3",
        "set pins, 4",
        "set pins, 5",
        "nop"
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    #[rustfmt::skip]
    let b_code = pio_proc::pio_asm!(
        // bit 4 indicates enable; bit 3 is the "B" machine flag. bits 2:0 are the payload
        "nop",
        "set pins, 0x0A", // without enable
        "set pins, 0x1B", // with enable
        "nop",
        "set pins, 0x1D", // with enable
        "set pins, 0x0E", // without enable
    );
    // note: this loads using sm_a so we can share the "used" vector state, but the code is global across all
    // SM's
    let b_prog = LoadedProg::load(b_code.program, &mut pio_ss).unwrap();

    report_api(0x51C2_0007);
    a_prog.setup_default_config(&mut sm_a);
    b_prog.setup_default_config(&mut sm_b);

    sm_a.config_set_set_pins(24, 5);
    sm_b.config_set_set_pins(24, 5);

    sm_a.config_set_sideset(0, false, false);
    sm_b.config_set_sideset(0, false, false);

    sm_a.config_set_clkdiv(4.0);
    sm_b.config_set_clkdiv(4.0);

    sm_a.config_set_out_special(false, false, 0); // A has no special enabling
    sm_b.config_set_out_special(false, true, 28); // B uses output enable

    sm_a.sm_init(a_prog.entry());
    sm_b.sm_init(b_prog.entry());

    report_api(0x51C2_0003);

    // use sm_a's PIO object to set state for both a & b here
    // restart dividers and machines so they are synchronized
    sm_a.pio.wo(
        rp_pio::SFR_CTRL,
        sm_a.pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_RESTART, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, sm_b.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_RESTART, sm_b.sm_bitmask()),
    );
    while (sm_a.pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
        // wait for the bits to self-reset to acknowledge that the command has executed
    }
    // now set both running at the same time
    report_api(0x51C2_1111);
    sm_a.pio.wo(
        rp_pio::SFR_CTRL,
        sm_a.pio.ms(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_EN, sm_b.sm_bitmask()),
    );
    // wait for it to run
    for i in 0..16 {
        report_api(0x51C2_0000 + i as u32);
    }
    // disable the machines
    sm_a.pio.wo(rp_pio::SFR_CTRL, 0);

    report_api(0x51C2_2222);

    // now turn on the sticky bit
    sm_a.config_set_out_special(true, false, 0);
    sm_b.config_set_out_special(true, true, 28);
    // change clkdiv just to hit another corner case
    sm_a.config_set_clkdiv(1.0);
    sm_b.config_set_clkdiv(1.0);
    // commit config changes
    sm_a.sm_init(a_prog.entry());
    sm_b.sm_init(b_prog.entry());

    // restart dividers and machines so they are synchronized
    sm_a.pio.wo(
        rp_pio::SFR_CTRL,
        sm_a.pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_RESTART, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, sm_b.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_RESTART, sm_b.sm_bitmask()),
    );
    while (sm_a.pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
        // wait for the bits to self-reset to acknowledge that the command has executed
    }
    // now set both running at the same time
    report_api(0x51C2_3333);
    sm_a.pio.wo(
        rp_pio::SFR_CTRL,
        sm_a.pio.ms(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask())
            | sm_a.pio.ms(rp_pio::SFR_CTRL_EN, sm_b.sm_bitmask()),
    );
    // wait for it to run
    for i in 0..16 {
        report_api(0x51C2_1000 + i as u32);
    }

    // disable the machines and cleanup
    sm_a.pio.wo(rp_pio::SFR_CTRL, 0);
    // clear the sticky bits
    sm_a.config_set_out_special(false, false, 0);
    sm_b.config_set_out_special(false, false, 0);
    sm_a.sm_init(a_prog.entry());
    sm_b.sm_init(b_prog.entry());
    // clear the instruction memory
    pio_ss.clear_instruction_memory();

    // NOTE: this test requires manual inspection of the output waveforms for pass/fail.
    report_api(0x51C2_600d);
}

pub fn delay(count: usize) {
    let mut target = [0u32; 1];
    for i in 0..count * 2 {
        unsafe { target.as_mut_ptr().write_volatile(i as u32) }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

fn wait_addr_or_fail(sm: &PioSm, addr: usize, timeout: Option<usize>) {
    let target = timeout.unwrap_or(1000);
    let mut timeout = 0;
    while sm.sm_address() != addr {
        timeout += 1;
        if timeout > target {
            assert!(false); // failed to stop on out exec
        }
    }
}

fn wait_rx_or_fail(sm: &mut PioSm, rxval: u32, mask: Option<u32>, timeout: Option<usize>) {
    let mask = mask.unwrap_or(0xFFFF_FFFF);
    let target = timeout.unwrap_or(1000);
    let mut timeout = 0;
    while sm.sm_rxfifo_is_empty() {
        // wait for the "in" response to come back
        timeout += 1;
        if timeout > target {
            assert!(false);
        }
    }
    let checkval = sm.sm_rxfifo_pull_u32();
    report_api(checkval);
    assert!(checkval & mask == rxval);
}

fn wait_gpio_or_fail(ss: &PioSharedState, pinval: u32, mask: Option<u32>, timeout: Option<usize>) {
    #[cfg(feature = "rp2040")]
    let io_mask = 0x3FFF_FFFF;
    #[cfg(not(feature = "rp2040"))]
    let io_mask = 0xFFFF_FFFF;

    let mask = mask.unwrap_or(io_mask);
    let target = timeout.unwrap_or(1000);
    let mut timeout = 0;
    loop {
        let outval = ss.pio.r(rp_pio::SFR_DBG_PADOUT);
        if (outval & mask) == (pinval & mask) {
            report_api(outval);
            break;
        }
        report_api(outval);
        timeout += 1;
        if timeout > target {
            assert!(false); // failed to acquire pinval
        }
    }
}
/// this routine will wait until at least the specified irq index is set
fn wait_irq_or_fail(sm: &PioSm, irq_index: usize, timeout: Option<usize>) {
    let target = timeout.unwrap_or(1000);
    let mut timeout = 0;
    while (sm.pio.rf(rp_pio::SFR_IRQ_SFR_IRQ) & (1 << irq_index)) == 0 {
        timeout += 1;
        if timeout > target {
            assert!(false);
        }
    }
}
/// this routine expects exactly just this one irq to be set; any other set is a failure
fn wait_irq_exactly_or_fail(sm: &PioSm, irq_index: usize, timeout: Option<usize>) {
    let target = timeout.unwrap_or(1000);
    let mut timeout = 0;
    while sm.pio.rf(rp_pio::SFR_IRQ_SFR_IRQ) != (1 << irq_index) {
        timeout += 1;
        if timeout > target {
            assert!(false);
        }
    }
}

/// corner cases
pub fn corner_cases() {
    report_api(0xF1F0_0000);
    #[cfg(feature = "rp2040")]
    let io_mask = 0x3FFF_FFFF;
    #[cfg(not(feature = "rp2040"))]
    let io_mask = 0xFFFF_FFFF;

    report_api(0xcc00_0000);

    // chained fifo depth corner case --------------------------------------
    let mut pio_ss = PioSharedState::new();
    let mut sm_a = pio_ss.alloc_sm().unwrap();
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "out y, 0", // 32 is coded as 0. If you put 32 in, this changes the "y" source to "null"!
        "in  y, 0", // 32 is coded as 0
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_out_pins(0, 32);
    sm_a.config_set_clkdiv(5.3);
    sm_a.config_set_out_shift(false, true, 32);
    sm_a.config_set_in_shift(true, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.sm_set_enabled(true);

    let mut entries = 0;
    while !sm_a.sm_txfifo_is_full() {
        entries += 1;
        sm_a.sm_txfifo_push_u32(0xCC00_0000 + entries);
    }
    report_api(0xcc00_0000 + entries);
    assert!(entries == 10);
    entries = 0;
    while !sm_a.sm_rxfifo_is_empty() {
        entries += 1;
        let check_data = sm_a.sm_rxfifo_pull_u32();
        report_api(check_data);
        assert!(check_data & 0xFFFF == entries);
    }
    pio_ss.clear_instruction_memory();

    report_api(0xcc00_1111);
    // exec corner case: EXEC instruction should side-effect the PC -----------------------
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "set x, 17",          // 18
        "exec_loop:",         // normally you'd want out exec, 32 to keep things simple but out exec, 16 will cause two instructions to run per pull of the fifo
        "  out exec, 16",     // 19 the intent is the "exec" instruction here should be mov x, y inv + "out exec, 16" (for next loop to stall)
        "  in y, 0",          // 1A
        "  jmp x-- exec_loop",// 1B
        "  wait 1 irq 4 rel", // 1C this should "gutter" execution here
        "exec_pc_test:",      //
        "  set x, 31",        // 1D
        "  mov pins, ::x",    // 1E (bit reversed x)
        "gutter:",            //
        "  jmp gutter",       // 1F
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_out_pins(0, 32);
    sm_a.config_set_clkdiv(2.5);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.sm_set_enabled(true);

    // wait for execution to get to the out instruction
    wait_addr_or_fail(&sm_a, 0x19, Some(100)); // manual match to wait target
    let exec_exec32 = pio_proc::pio_asm!("out exec, 0").program.code[0];
    let exec_exec16 = pio_proc::pio_asm!("out exec, 16").program.code[0];
    let mov_xy_inv = pio_proc::pio_asm!("mov y, !x").program.code[0];

    // test that exec can exec "out, exec" instructions. this has to be verified looking
    // at the waveforms
    sm_a.sm_txfifo_push_u32(exec_exec32 as u32 | (exec_exec32 as u32) << 16);
    sm_a.sm_txfifo_push_u32(exec_exec16 as u32 | (exec_exec16 as u32) << 16);
    sm_a.sm_txfifo_push_u32(exec_exec32 as u32 | (exec_exec32 as u32) << 16);
    sm_a.sm_txfifo_push_u32(exec_exec16 as u32 | (exec_exec16 as u32) << 16);

    while sm_a.sm_txfifo_is_full() {
        // wait until the pushed instructions have self-exec'd
    }
    // this will trigger the necessary instruction to get out of self-exec hell
    // the upper 16-bits exec_exec16 is needed to ensure the loop stalls the next
    // time around; this is because we made it an "out exec, 16", so if we leave
    // the top bits 0, it will just exec a jmp to 0, which is essentially a nop.
    sm_a.sm_txfifo_push_u32(mov_xy_inv as u32 | (exec_exec16 as u32) << 16);

    wait_rx_or_fail(&mut sm_a, !17u32, None, None);

    // it should have looped back to the top again, waiting for another exec. this
    // time check that exec of an out, PC works
    let exec_outpc32 = pio_proc::pio_asm!("out pc, 16").program.code[0];
    // this should execute the bottom 16 bits, shift it right, then take the upper 16 bits as args to the PC
    sm_a.sm_txfifo_push_u32(exec_outpc32 as u32 | 0x1d_0000);
    // this should now put the value 0xF800_0000 onto the output pins
    wait_gpio_or_fail(&pio_ss, 0xf800_0000, None, Some(1000));

    // reset the machine and test that a JMP in the OUT EXEC works
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());

    // wait for execution to get to the out instruction
    wait_addr_or_fail(&sm_a, 0x19, Some(100));
    // this instruction will decrement x from 17->16 and jump
    let exec_outpc32 = pio_proc::pio_asm!("jmp x--, 0x1e").program.code[0];
    sm_a.sm_txfifo_push_u32(exec_outpc32 as u32);
    // this should now put the value 0x0800_0000 onto the output pins (16 reversed)
    wait_gpio_or_fail(&pio_ss, 0x0800_0000, None, Some(1000));

    sm_a.sm_set_enabled(false);
    pio_ss.clear_instruction_memory();
    report_api(0xcc00_2222);
    // exec corner case: exec can clear stalled IRQ -----------------------
    let mut sm_b = pio_ss.alloc_sm().unwrap(); // run this on SM_B so we can test `rel` irqs
    #[rustfmt::skip]
    let b_code = pio_proc::pio_asm!(
        "set x, 12",          // 0x1d put an initial value in x
        "wait 1 irq 6 rel",   // 0x1e this should stall <--- wait target
        "in  x, 0",           // 0x1f this will push 12 into Rx FIFO, indicating success
    );
    let b_prog = LoadedProg::load(b_code.program, &mut pio_ss).unwrap();
    sm_b.sm_set_enabled(false);
    b_prog.setup_default_config(&mut sm_b);
    sm_b.config_set_out_pins(0, 32);
    sm_b.config_set_clkdiv(1.5);
    sm_b.config_set_out_shift(true, true, 32);
    sm_b.config_set_in_shift(false, true, 32);
    sm_b.sm_init(b_prog.entry());
    sm_b.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_b.sm_set_enabled(true);

    wait_addr_or_fail(&sm_b, 0x1e, Some(100)); // manual match to wait target
    let exec_set_irq6 = pio_proc::pio_asm!("irq set 6 rel").program.code[0];
    sm_b.sm_exec(exec_set_irq6);
    wait_rx_or_fail(&mut sm_b, 12, None, None);

    sm_b.sm_set_enabled(false);
    pio_ss.clear_instruction_memory();

    // exec corner case: exec can clear stalled IRQ --------------------------------
    // exec corner case: exec can break out of stalled instruction with JMP --------
    #[rustfmt::skip]
    let b_code = pio_proc::pio_asm!(
        "top:",
        "  set y, 12",          // 0x1a put an initial value in y
        "wait_target:",
        "  wait 1 irq 6 rel",   // 0x1b this should stall <--- wait target
        "  in  y, 0",           // 0x1c this will push 12 into Rx FIFO, indicating success
        "  jmp top",            // 0x1d loop back, so the only way we get to bypass is with an exec
        "bypass:",
        "  mov x, !y",          // 0x1e invert y
        "  in  x, 0",           // 0x1f pushes !y into Rx FIFO
    );
    let b_prog = LoadedProg::load(b_code.program, &mut pio_ss).unwrap();
    // check multiple clkdiv cases to capture corners in the irq_stb logic
    let divs = [1.0f32, 3.5f32, 4.0f32];
    let mut divstate = 0x3333u32;
    divs.map(|clkdiv: f32| {
        report_api(0xcc00_0000 + divstate);
        divstate += 0x1111;
        sm_b.sm_set_enabled(false);
        b_prog.setup_default_config(&mut sm_b);
        sm_b.config_set_out_pins(0, 32);
        sm_b.config_set_clkdiv(clkdiv);
        sm_b.config_set_out_shift(true, true, 32);
        sm_b.config_set_in_shift(false, true, 32);
        sm_b.sm_init(b_prog.entry());
        sm_b.sm_clear_fifos(); // ensure the fifos are cleared for this test
        sm_b.sm_set_enabled(true);

        wait_addr_or_fail(&sm_b, 0x1b, Some(100)); // manual match to wait target
        let exec_set_irq6 = pio_proc::pio_asm!("irq set 6 rel").program.code[0];
        sm_b.sm_exec(exec_set_irq6);
        // exactly one entry should go into the Rx fifo. If the irq does not self-clear after exec, we will
        // get more than one, and the next check fails.
        wait_rx_or_fail(&mut sm_b, 12, None, None);

        // program will loop back to top, and will be stuck at wait again
        wait_addr_or_fail(&sm_b, 0x1b, Some(100)); // manual match to wait target
        let exec_jmp_bypass = pio_proc::pio_asm!("jmp y--, 0x1e").program.code[0];
        sm_b.sm_exec(exec_jmp_bypass);
        wait_rx_or_fail(&mut sm_b, !11, None, None);
    });

    // exec corner case: OUT EXEC can clear stalled IRQ -------------------
    pio_ss.clear_instruction_memory();
    report_api(0xcc00_6666);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "top:",
        "  set y, 5",           // 0x19 put an initial value in y
        "wait_target:",
        "  out exec, 32",       // 0x1a stall on out exec <---- stall target
        "  wait 1 irq 2 rel",   // 0x1b this should stall unless the `out exec` had an `irq` instruction in it
        "  in  y, 0",           // 0x1c this will push 5 into Rx FIFO, indicating success
        "  jmp top",            // 0x1d loop back, so the only way we get to bypass is with an exec
        "bypass:",
        "  mov x, !y",          // 0x1e invert y
        "  in  x, 0",           // 0x1f pushes !y into Rx FIFO
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(3.0);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.sm_set_enabled(true);

    wait_addr_or_fail(&sm_a, 0x1a, Some(100)); // manual match to wait target
    let exec_set_irq2 = pio_proc::pio_asm!("irq set 2 rel").program.code[0];
    sm_a.sm_txfifo_push_u32(exec_set_irq2 as u32);
    wait_rx_or_fail(&mut sm_a, 5, None, None);

    // exec corner case: writing over stalled instruction breaks stall ----------
    wait_addr_or_fail(&sm_a, 0x1a, Some(100)); // manual match to wait target
    let nop = pio_proc::pio_asm!("mov y, y").program.code[0];
    sm_a.sm_txfifo_push_u32(nop as u32); // break the out exec stall, but get stuck at "wait 1 irq 2 rel"
    wait_addr_or_fail(&sm_a, 0x1b, Some(100)); // manual match to wait target
    let set_y_31 = pio_proc::pio_asm!("set y, 31").program.code[0];
    // slot this over the blocked instruction: 0x1b == 27
    pio_ss.pio.wfo(rp_pio::SFR_INSTR_MEM27_INSTR, set_y_31 as u32);
    wait_rx_or_fail(&mut sm_a, 31, None, None);

    // exec corner case: EXEC succeeds even when SM is disabled -----------------
    sm_a.sm_set_enabled(false);
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.sm_exec(pio_proc::pio_asm!("set x, 12").program.code[0]);
    sm_a.sm_exec(pio_proc::pio_asm!("in  x, 0").program.code[0]);
    sm_a.sm_exec(pio_proc::pio_asm!("mov y, !x").program.code[0]);
    sm_a.sm_exec(pio_proc::pio_asm!("in  y, 0").program.code[0]);
    sm_a.sm_exec(pio_proc::pio_asm!("mov x, ::y").program.code[0]);
    sm_a.sm_exec(pio_proc::pio_asm!("in  x, 0").program.code[0]);

    wait_rx_or_fail(&mut sm_a, 12, None, None);
    wait_rx_or_fail(&mut sm_a, !12, None, None);
    wait_rx_or_fail(&mut sm_a, 0xCFFF_FFFF, None, None);

    // IO corner case: side-set happens when SM is stalled --------------------
    pio_ss.clear_instruction_memory();
    report_api(0xcc00_7777);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        ".side_set 1 opt"
        "top:",
        "  set y, 9           side 1", // 0x19 put an initial value in y
        "wait_target:",
        "  out exec, 32       side 0", // 0x1a stall on out exec <---- stall target
        "  wait 1 irq 5       side 1", // 0x1b also stalls
        "  in  y, 0           side 0", // 0x1c this will push 9 into Rx FIFO, indicating success
        "  jmp top",                   // 0x1d loop back, so the only way we get to bypass is with an exec
        "bypass:",
        "  mov x, !y",                 // 0x1e invert y
        "  in  x, 0",                  // 0x1f pushes !y into Rx FIFO
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(7.15);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.config_set_out_pins(4, 1);
    sm_a.config_set_sideset_pins(4); // one bit at bit 4 should flip up and down
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
    sm_a.sm_set_enabled(true);

    wait_addr_or_fail(&sm_a, 0x1a, None);
    assert!((sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & 0x10) == 0); // side-set on out exec 32 should have gone through
    sm_a.sm_txfifo_push_u32(pio_proc::pio_asm!("nop").program.code[0] as u32);
    wait_addr_or_fail(&sm_a, 0x1b, None);
    assert!((sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & 0x10) != 0); // side-set on out exec 32 should have gone through
    sm_a.sm_exec(pio_proc::pio_asm!("irq set 5").program.code[0]);
    wait_addr_or_fail(&sm_a, 0x1a, None);
    assert!((sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & 0x10) == 0);
    wait_rx_or_fail(&mut sm_a, 9, None, None);

    // IO corner case: simultaneous side-set and OUT/SET of the same pin gives precedence to side-set -----
    pio_ss.clear_instruction_memory();
    report_api(0xcc00_8888);
    // this one will toggle pins via a side-set
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        ".side_set 1 opt"
        "  mov osr, null",           // 0x1b load OSR with 0
        "  out pins, 2    side 1",   // 0x1c write 0b00 to the output pins - but side set should override to 1
        "  wait 1 irq 0",            // 0x1d cause a stall
        "  set pins, 3    side 0",   // 0x1e write 0b11 to the output pins - but side set should override to 0
        "  wait 1 irq 0",            // 0x1f cause a stall
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(1.15);
    sm_a.config_set_out_shift(false, false, 2);
    sm_a.config_set_out_pins(0, 2);
    sm_a.config_set_set_pins(0, 2);
    sm_a.config_set_sideset_pins(0);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_set_enabled(true);

    for _ in 0..10 {
        wait_addr_or_fail(&sm_a, 0x1d, None);
        assert!((sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & 0b11) == 0b01);
        sm_a.sm_exec(pio_proc::pio_asm!("irq set 0").program.code[0]);
        wait_addr_or_fail(&sm_a, 0x1f, None);
        assert!((sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & 0b11) == 0b10);
        sm_a.sm_exec(pio_proc::pio_asm!("irq set 0").program.code[0]);
    }

    // IO corner case: pin modulus wrapping ----------------------------------
    // 1. OUT_BASE / OUT_COUNT wrap around
    // 2. SET_BASE / SET_COUNT wrap around
    // 3. SIDESET_BASE / SIDESET_COUNT wrap around
    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    report_api(0xcc00_9999);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        ".side_set 5",
        "  out pins, 16   side 0x1A", // 0x1d
        "  set pins, 0x1f side 0x1A", // 0x1e
        "  out pins, 16   side 0x05", // 0x1f
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(1.15);
    sm_a.config_set_out_shift(true, true, 16);
    sm_a.config_set_out_pins(24, 16); // should wrap to lower 8 bits
    sm_a.config_set_set_pins(28, 5); // should wrap 1 bit over
    sm_a.config_set_sideset_pins(30); // should wrap 3 bits over
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();
    sm_a.sm_set_enabled(true);

    let set_mask = rot_left(0x1f, 28);
    let sideset_mask = rot_left(0x1f, 30);
    let base_mask = rot_left(0xffff, 24);
    let mut model;

    wait_addr_or_fail(&sm_a, 0x1d, None);
    sm_a.sm_txfifo_push_u32(0);
    model = rot_left(0 & 0xFFFF, 24);
    model &= !set_mask;
    model |= rot_left(0x1f, 28);
    model &= !sideset_mask;
    model |= rot_left(0x05, 30);
    model &= io_mask;
    wait_addr_or_fail(&sm_a, 0x1f, None);
    let rbk = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & base_mask;
    report_api(rbk);
    report_api(model);
    assert!(rbk == model);

    sm_a.sm_txfifo_push_u32(0xcccc);
    wait_addr_or_fail(&sm_a, 0x1d, None);
    model = rot_left(0xcccc & 0xFFFF, 24);
    model &= !sideset_mask;
    model |= rot_left(0x05, 30);
    model &= !sideset_mask;
    model |= rot_left(0x1a, 30);
    model &= io_mask;
    let rbk = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT) & base_mask;
    report_api(rbk);
    report_api(model);
    assert!(rbk == model);

    // IO corner case: pin modulus wrapping ----------------------------------
    // 4. Input pins wrap around modulo 32 off of IN_BASE
    // 5. WAIT with pin source wraps modulo 32 + in_pin_index
    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    report_api(0xcc00_aaaa);
    // set all the output
    #[cfg(feature = "rp2040")]
    sm_a.sm_set_pindirs_with_mask(0xffff_ffff, 0xffff_ffff);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "  out pins, 0",
        "  in  pins, 24",
        "  out pins, 0",
        "  set y, 10",
        "  wait 1 pin 30", // clear this wait with an exec
        "  in  y, 0",  // puts 10 into the rx fifo to indicate done-ness
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(32.0); // give time for data to go out and back again
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 24);
    sm_a.config_set_out_pins(0, 32);
    sm_a.config_set_in_pins(9);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();
    sm_a.sm_set_enabled(true);

    sm_a.sm_txfifo_push_u32(0xaa12_3455);
    // the differences from the pushed is due to the test bench harnessing, some gpio pins are not looped back
    // or are modified by the test bench for other tests! this value is just read back manually from the
    // waveforms
    #[cfg(not(feature = "rp2040"))]
    let model = rot_left(0x2a12_345D, 32 - 9) & 0xFF_FFFF;
    #[cfg(feature = "rp2040")]
    let model = rot_left(0xaa12_3455 & io_mask & !(1 << 25), 32 - 9) & 0xFF_FFFF; // top 2 bits don't exist on rp2040, bit 25 is for LED
    report_api(model);
    wait_rx_or_fail(&mut sm_a, model, None, None);

    // clear the output pins to 0
    sm_a.sm_txfifo_push_u32(0x0);
    // we should now be stuck on "wait 1 pin 30"
    wait_addr_or_fail(&sm_a, 0x1e, None);
    sm_a.sm_exec(pio_proc::pio_asm!("out pins 0").program.code[0]);
    sm_a.sm_txfifo_push_u32(1 << ((30 + 9) % 32)); // this targeted set should clear the wait only if the wait is wrapping the inputs correctly
    wait_rx_or_fail(&mut sm_a, 10, None, None);

    // autopush/pull cases ----------------------------------------------------
    // 1. auto p/p does not happen while SM is disabled
    // 2. auto pull happens on an EXEC, even when SM is disabled a. case of EXEC is an instruction that would
    //    do a pull, but stalled instruction does not do pull b. case of EXEC is an instruction that is NOP,
    //    but stalled instruction does do pull
    // 3. OUT with empty OSR but nonempty TX FIFO should not set TX stall flag, + 1-cycle stall
    // 4. EXEC of OUT 32 to disabled SM, with empty OSR + filled FIFO consumes two words from FIFO
    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    report_api(0xcc00_bbbb);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "  out x, 0",
        "  in  x, 0",   // loop back for testing
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(3.0);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();

    // sm is halted right now, but would do a pull if it were turned on
    for i in 0..4 {
        sm_a.sm_txfifo_push_u32(0xcc00_0000 + i);
    }
    // after four writes, the tx fifo should be full. if auto-pull were on, it would have
    // pulled one entry and we wouldn't be full.
    report_api(0xcc01_bbbb);
    assert!(sm_a.sm_txfifo_is_full());

    // this should cause an auto-pull, lowering the fifo level by 1
    sm_a.sm_exec(pio_proc::pio_asm!("nop").program.code[0]);
    report_api(0xcc02_bbbb);
    assert!(sm_a.sm_txfifo_level() == 3);

    // reset the machine
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();
    // machine is still disabled
    for i in 0..4 {
        sm_a.sm_txfifo_push_u32(0xcc01_0000 + i);
    }
    // this should cause an auto-pull, lowering the fifo level by *2*
    sm_a.sm_exec(pio_proc::pio_asm!("out x, 0").program.code[0]);
    report_api(0xcc03_bbbb);
    assert!(sm_a.sm_txfifo_level() == 2);

    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    report_api(0xcc04_bbbb);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "  wait 1 irq 0",  // this will do a wait
        "  in  x, 0",
        "  out x, 0",
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(3.0);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();
    pio_ss.pio.wfo(rp_pio::SFR_FDEBUG_TXSTALL, 0xF); // clear the txstall register

    // machine is not enabled
    for i in 0..2 {
        sm_a.sm_txfifo_push_u32(0xcc02_0000 + i);
    }
    report_api(0xcc05_bbbb);
    assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXSTALL) == 0);
    // case of OUT on instruction that does not do a pull, doing a pull when the machine is disabled
    sm_a.sm_exec(pio_proc::pio_asm!("out x, 0").program.code[0]);
    report_api(0xcc06_bbbb);
    assert!(sm_a.sm_txfifo_level() == 0); // reduces by *2*, so we should have 0 items in the fifo
    report_api(0xcc07_bbbb);
    assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXSTALL) == 0);
    sm_a.sm_set_enabled(true);
    // should run ahead and ....
    sm_a.sm_exec(pio_proc::pio_asm!("irq set 0").program.code[0]);
    report_api(0xcc08_bbbb);
    assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXSTALL) == 0);
    sm_a.sm_exec(pio_proc::pio_asm!("irq set 0").program.code[0]);
    // check that we got the two items we expected at this point
    wait_rx_or_fail(&mut sm_a, 0xcc02_0000, None, None);
    wait_rx_or_fail(&mut sm_a, 0xcc02_0001, None, None);
    // confirm that TXSTALL is now set
    assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXSTALL) == sm_a.sm_bitmask());

    report_api(0xcc08_cccc);
    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "  out x, 0",
        "  in  x, 0",   // loop back for testing
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(13.0);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(false, true, 32);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos();

    // sm is halted right now, but would do a pull if it were turned on
    for i in 0..4 {
        sm_a.sm_txfifo_push_u32(0xcc00_0000 + i);
    }
    sm_a.sm_set_enabled(true);
    // manual test to confirm OUT with empty OSR and nonempty TX FIFO experiences a
    // 1-cycle stall as there's no bypass of FIFO through OSR: check with waveform browser here.

    report_api(0xcc00_600d);
}
fn rot_left(word: u32, count: u32) -> u32 { (word << count) | ((word >> (32 - count)) & ((1 << count) - 1)) }

pub fn instruction_tests() {
    report_api(0x1c5f_0000);
    let mut pio_ss = PioSharedState::new();
    let mut sm_a = pio_ss.alloc_sm().unwrap();
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);

    // check mov, irq, wait corners ------------------------------------------------------
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "irq set 7",      // just to make sure this is behaving correctly (staring at waveforms)
        "out x, 0",
        "in  x, 0",
        "out x, 0",
        "in  x, 0",
        "irq wait 1",     // 0x19  this only moves on when cleared
        "wait 1 irq 1",   // 0x1a  so this should *also* stall
        "mov y, status",  // 0x1b
        "in  y, 0",       // 0x1c
        "set x, 30",      // 0x1d  stick myself in a loop to here
        "irq clear 7",    // 0x1e  just to make sure this is behaving correctly (staring at waveforms)
        "mov pc, x",      // 0x1f  should stick in a tight loop at the end
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    // if this is set to <2.0 the "retrograde PC" check at the bottom needs to be commented out, because the
    // PC moves too fast for the APB to reliably sample also, this should not be an even multiple,
    // otherwise we can end up sampling the exact same phase over and over again.
    sm_a.config_set_clkdiv(5.0);
    sm_a.config_set_out_shift(true, true, 32);
    sm_a.config_set_in_shift(true, true, 32);
    sm_a.config_set_mov_status(MovStatusType::StatusTxLessThan, 0);
    sm_a.sm_init(a_prog.entry());
    assert!(sm_a.sm_index() == 0); // make sure we got this SM because we're hard coding the STATUS change value in the loop below to this index
    let mut status_sel = MovStatusType::StatusTxLessThan;
    let mut level = 0;
    loop {
        sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test
        pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 0xFF); // clear all irqs for this test
        pio_ss.pio.wo(
            rp_pio::SFR_SM0_EXECCTRL,
            pio_ss.pio.zf(
                rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL,
                pio_ss.pio.zf(rp_pio::SFR_SM0_EXECCTRL_STATUS_N, pio_ss.pio.r(rp_pio::SFR_SM0_EXECCTRL)),
            ) | pio_ss.pio.ms(rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, status_sel as u32)
                | pio_ss.pio.ms(rp_pio::SFR_SM0_EXECCTRL_STATUS_N, level as u32),
        );
        let expected_value = if 2 < level { 0xFFFF_FFFF } else { 0 };

        // reset the PC
        let mut a = pio::Assembler::<32>::new();
        let mut initial_label = a.label_at_offset(a_prog.entry() as u8);
        a.jmp(pio::JmpCondition::Always, &mut initial_label);
        let p = a.assemble_program();
        sm_a.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
        pio_ss.pio.wfo(rp_pio::SFR_CTRL_RESTART, sm_a.sm_bitmask());
        sm_a.sm_set_enabled(true);

        for i in 1..=5 {
            // put 5 entries in; the first two should go to the Rx fifo, 1 should be in the state machine, 2
            // remaining in the FIFO
            sm_a.sm_txfifo_push_u32(0x1c5f_0000 + i);
        }
        // we should now have a level == 2 for both
        report_api(0x1c5f_1000);
        assert!(sm_a.sm_txfifo_level() == 2);
        report_api(0x1c5f_2000);
        assert!(sm_a.sm_rxfifo_level() == 2);
        // wait until irq 1 is asserted
        wait_irq_or_fail(&sm_a, 1, None);
        report_api(0x1c5f_3000);
        //wait_addr_or_fail(&sm_a, 0x19, None);
        // clear irq1
        pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 1);
        report_api(0x1c5f_3001);
        //wait_addr_or_fail(&sm_a, 0x1a, None);
        // set irq1
        pio_ss.pio.wfo(rp_pio::SFR_IRQ_FORCE_SFR_IRQ_FORCE, 1 << 1);
        report_api(0x1c5f_4000);
        assert!(sm_a.sm_rxfifo_level() == 3);
        wait_rx_or_fail(&mut sm_a, 0x1c5f_0001, None, None);
        wait_rx_or_fail(&mut sm_a, 0x1c5f_0002, None, None);
        report_api(expected_value);
        wait_rx_or_fail(&mut sm_a, expected_value, None, None);
        // retrograde movement of address would indicate we're in the loop (this might not work perfectly on
        // real hardware due to synchronizers) commented out, because this test is too sensitive to
        // external clock config wait_addr_or_fail(&sm_a, 31, None);
        // wait_addr_or_fail(&sm_a, 30, None);

        level += 1;
        if level > 4 {
            if status_sel == MovStatusType::StatusRxLessThan {
                break;
            } else {
                status_sel = MovStatusType::StatusRxLessThan;
                level = 0;
            }
        }
        sm_a.sm_set_enabled(false);
    }

    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);
    report_api(0x1c5f_1111);
    // all the jumps --------------------------------------------------------------
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "  set x, 3",
        "test_notx:",
        "  jmp !x, notx_done",
        "  irq wait 0",    // we should have to clear irq 0 3 times
        "  jmp x-- test_notx",
        "notx_done:",
        "  set y, 3",
        "test_noty:",
        "  jmp !y  noty_done",
        "  irq wait 1",    // we should have to clear irq 1 3 times
        "  jmp y-- test_noty",
        "noty_done:",
        "  set x, 7",
        "test_noteq:",
        "  set y, 7",
        "  jmp x!=y noteq_done",
        "  irq wait 2",    // we should have to clear irq 2 once
        "  jmp x-- test_noteq",
        "noteq_done:",
        "  pull noblock",    // fifo is empty; this should put X=6 into the OSR and fill it to 32 bits
        "osr_empty:",
        "  out isr, 16",     // this will shift 16 bits into the ISR, shift_right = true so it goes 0, then 6
        "  jmp !osre, osr_empty",  // loops back and gets another 16 bits
        "  irq wait 3",      // we should have to clear irq 3 once
        "  jmp finish",
        "  jmp test_notx",   // this should never be run
        "finish: ",
        "  push iffull noblock", // rx fifo should get the value 6
        "  irq wait 4",      // done once irq4 is set
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(2.0); // if this is set to 1.0 the "retrograde PC" check at the bottom needs to be commented out, because the PC moves too fast for the APB to reliably sample
    sm_a.config_set_out_shift(false, false, 32);
    sm_a.config_set_in_shift(false, false, 16); // match to push iffull noblock expectations
    sm_a.sm_init(a_prog.entry());
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 0xFF); // clear all irqs for this test
    sm_a.sm_set_enabled(true);

    // wait for 3 irq0's
    for _ in 0..3 {
        wait_irq_exactly_or_fail(&sm_a, 0, None);
        pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 0);
    }
    // wait for 3 irq1's
    for _ in 0..3 {
        wait_irq_exactly_or_fail(&sm_a, 1, None);
        pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 1);
    }
    // wait for 1 irq 2
    wait_irq_exactly_or_fail(&sm_a, 2, None);
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 2);
    // wait for 1 irq 3
    wait_irq_exactly_or_fail(&sm_a, 3, None);
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 3);
    // wait for 1 irq 4
    wait_irq_exactly_or_fail(&sm_a, 4, None);
    wait_rx_or_fail(&mut sm_a, 6, None, None);

    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);
    report_api(0x1c5f_2222);
    // all the ISRs and OSRs (left shifting) -------------------------------------------------
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "start:",
        "pull ifempty block",   // 09 get a value from the Tx FIFO -> OSR
        "set x, 3",             // 0a prime the x value
        "in osr, 2",            // 0b shift bottom 2 bits from the OSR into the IN
        "in x, 3",              // 0c add a `011` pattern
        "in isr, 5",            // 0d now double the pattern in the buffer
        "push iffull block",    // 0e this should "do nothing", because the ISR is not full
        "in null, 8",           // 0f this tops off the ISR
        "push iffull block",    // 10 this should push the result into the Rx FIFO
        "pull ifempty block",   // 11 this should not pull, because the ifempty condition is not met
        "out null, 18",         // 12 setup the condition for the pull to happen
        "pull ifempty block",   // 13 this *should* pull
        "out isr, 16",          // 14 put top 16 bits into the ISR that were pulled in
        "in null, 16",          // 15 shift those 16 bits to the left
        "push iffull block",    // 16 return the shifted value back for inspection
        "push block",           // 17 ISR isn't full, but push data anyways to fill up the rxfifo
        "push block",           // 18 ISR isn't full, but push data anyways to fill up the rxfifo
        "pull block",           // 19 put more data into the OSR, this time, a PC value. should be 0x1c
        "out pc, 5",            // 1a shift top 5 bits into the PC.
        "jmp start",            // 1b something that loops us back to the top if the out pc didn't do its thing
        "set y, 13",            // 1c a value for reporting success
        "mov isr, y",           // 1d put y into the ISR
        "push block",           // 1e this should stall, until the Rx FIFO is drained. check that RXSTALL is set
        "irq wait 7",           // 1f indicate done by hanging on the irq
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(2.0); // if this is set to 1.0 the "retrograde PC" check at the bottom needs to be commented out, because the PC moves too fast for the APB to reliably sample
    sm_a.config_set_out_shift(false, false, 18);
    sm_a.config_set_in_shift(false, false, 18);
    sm_a.sm_init(a_prog.entry());
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 0xFF); // clear all irqs for this test
    pio_ss.pio.wo(rp_pio::SFR_FDEBUG, 0xFFFF_FFFF); // clear all the stalls
    sm_a.sm_set_enabled(true);

    report_api(0x1c5f_2220);
    wait_addr_or_fail(&sm_a, 0x9, None);
    report_api(sm_a.pio.rf(rp_pio::SFR_FDEBUG_RXSTALL));
    assert!(sm_a.pio.rf(rp_pio::SFR_FDEBUG_RXSTALL) == 0);
    sm_a.sm_txfifo_push_u32(0x0000_0001); // inject '01' pattern for the expected outcome
    sm_a.sm_txfifo_push_u32(0x1234_dead); // inject a pattern for the "out isr" test
    sm_a.sm_txfifo_push_u32(0x1c << 27); // position the final PC jump value at the right location
    let mut expected = 0b01;
    expected = (expected << 3) | 0b11;
    expected = expected | (expected << 5);
    expected <<= 8;
    report_api(0x1c5f_2221);

    let expected2 = 0x1234_0000;
    // wait until we block again on ISR not being full
    report_api(0x1c5f_2225);

    wait_addr_or_fail(&sm_a, 0x1e, None);
    report_api(0x1c5f_2228);
    assert!(sm_a.pio.rf(rp_pio::SFR_FDEBUG_RXSTALL) != 0);
    report_api(0x1c5f_2229);
    assert!(sm_a.pio.rf(rp_pio::SFR_IRQ_SFR_IRQ) == 0);

    // pull values out for checking
    report_api(0x1c5f_222a);
    report_api(expected);
    wait_rx_or_fail(&mut sm_a, expected, None, None);
    report_api(expected2);
    wait_rx_or_fail(&mut sm_a, expected2, None, None);
    wait_rx_or_fail(&mut sm_a, 0, None, None);
    wait_rx_or_fail(&mut sm_a, 0, None, None);
    wait_rx_or_fail(&mut sm_a, 13, None, None);

    // now wait for the final IRQ
    report_api(0x1c5f_222b);
    wait_irq_or_fail(&sm_a, 7, None);

    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);
    report_api(0x1c5f_3333);
    // all the MOVs (plus right shifting) -------------------------------------------------
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "start:",
        "pull block",
        "mov isr, !osr",
        "push block",   // this copies what's in the tx fifo to the rx fifo
        "set x, 3",
        "mov y, !x",
        "in x, 3",
        "in y, 3",
        "push block",
        "in x, 3",
        "mov isr, isr",  // resets ISR count with out erasing values (check in waveform)
        "pull block",
        "out y 6",
        "mov osr, osr",  // resets OSR count without erasing values (check in waveform)
        "mov x, ::y",
        "mov isr, x",
        "mov osr, isr",
        "mov pins, osr",
        "irq wait 0",
        "mov pins, !pins",
        "irq wait 1",
        "set x, 11",
        "pull noblock",  // this should put X (11) into OSR
        "mov isr, osr",
        "push block",
        "irq wait 2",
        "exec_loop:",
        "pull block",
        "mov exec, osr",
        "jmp exec_loop",
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(2.0); // if this is set to 1.0 the "retrograde PC" check at the bottom needs to be commented out, because the PC moves too fast for the APB to reliably sample
    sm_a.config_set_out_shift(true, false, 6);
    sm_a.config_set_in_shift(true, false, 6);
    sm_a.config_set_out_pins(0, 32);
    sm_a.config_set_in_pins(16);
    sm_a.sm_init(a_prog.entry());
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 0xFF); // clear all irqs for this test
    pio_ss.pio.wo(rp_pio::SFR_FDEBUG, 0xFFFF_FFFF); // clear all the stalls
    sm_a.sm_set_enabled(true);

    sm_a.sm_txfifo_push_u32(0x1234_abcd);
    sm_a.sm_txfifo_push_u32(0b101_110 | 0xFF00_0000); // shifting right, so we'll take these LSBs; or something else at the top so we can detect if it shifted the other way
    wait_irq_exactly_or_fail(&sm_a, 0, None);
    wait_rx_or_fail(&mut sm_a, !0x1234_abcd, None, None);
    wait_rx_or_fail(&mut sm_a, 0b100_011 << (32 - 6), None, None);
    report_api(0x1c5f_3334);
    #[cfg(not(feature = "rp2040"))]
    let gpio_val = 0b011_101 << (32 - 6);
    #[cfg(feature = "rp2040")]
    let gpio_val = (0b011_101 << (32 - 6)) & !(1 << 25) & 0x3FFF_FFFF;
    report_api(gpio_val);
    wait_gpio_or_fail(&pio_ss, gpio_val, None, None);
    // clear the IRQ wait
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 0);

    // pins <- pins mov test
    report_api(0x1c5f_3335);
    wait_irq_exactly_or_fail(&sm_a, 1, None);
    #[cfg(not(feature = "rp2040"))]
    {
        let rbk_val = !rot_left(gpio_val | 0xC, 16); // 0xC is or'd in due to the i2c loopback pins
        wait_gpio_or_fail(&pio_ss, rbk_val, Some(!0x8000_000C), None); // ignore the pins not looped back by the test bench (2,3,31)
    }
    #[cfg(feature = "rp2040")]
    {
        let rbk_val = !rot_left(gpio_val & 0x3FFF_FFFF | 3, 16); // lowest 2 bits read back back as 11
        report_api(rbk_val);
        wait_gpio_or_fail(&pio_ss, rbk_val, Some(0x3DFF_FFFF), None); // mask out top bit + LED bit
    }
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 1);

    // pull noblock of empty Tx gives X test
    wait_irq_exactly_or_fail(&sm_a, 2, None);
    wait_rx_or_fail(&mut sm_a, 11, None, None);
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 2);

    // this will effectively exec this instruction
    sm_a.sm_txfifo_push_u32(pio_proc::pio_asm!("irq set 5").program.code[0] as u32);
    wait_irq_exactly_or_fail(&sm_a, 5, None);
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 5);
    // confirm we can do it again
    sm_a.sm_txfifo_push_u32(pio_proc::pio_asm!("irq set 6").program.code[0] as u32);
    wait_irq_exactly_or_fail(&sm_a, 6, None);
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 1 << 6);

    // check that decrements wrap around correctly --------------------------------------
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);
    report_api(0x1c5f_4444);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "start:",
        "  set x, 0",
        "  jmp x--, dec1",
        "dec1:",
        "  mov isr, x",
        "  push block",
        "  jmp x--, dec2",
        "dec2:",
        "  mov isr, x",
        "  push block",
        "  mov y, !x",
        "  jmp y--, inc1",
        "inc1:",
        "  mov x, !y",
        "  mov isr, x",
        "  push block",
        "  jmp y--, inc2",
        "inc2:",
        "  mov x, !y",
        "  mov isr, x",
        "  push block",
        "  jmp y--, inc3",
        "inc3:",
        "  mov x, !y",
        "  mov isr, x",
        "  push block",
        "  irq wait 1",
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_clkdiv(2.0);
    sm_a.config_set_out_shift(true, false, 32);
    sm_a.config_set_in_shift(true, false, 32);
    sm_a.sm_init(a_prog.entry());
    pio_ss.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, 0xFF); // clear all irqs for this test
    sm_a.sm_set_enabled(true);

    wait_rx_or_fail(&mut sm_a, u32::from_le_bytes((-1i32).to_le_bytes()), None, None);
    wait_rx_or_fail(&mut sm_a, u32::from_le_bytes((-2i32).to_le_bytes()), None, None);
    wait_rx_or_fail(&mut sm_a, u32::from_le_bytes((-1i32).to_le_bytes()), None, None);
    wait_rx_or_fail(&mut sm_a, u32::from_le_bytes((0i32).to_le_bytes()), None, None);
    wait_rx_or_fail(&mut sm_a, u32::from_le_bytes((1i32).to_le_bytes()), None, None);

    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);

    report_api(0x1c5f_600d);
}

/// test that stalled imm instructions are restarted on restart
pub fn restart_imm_test() {
    report_api(0x0133_0000);

    let mut pio_ss = PioSharedState::new();
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);

    let mut sm_a = pio_ss.alloc_sm().unwrap();
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "set pins, 1",
        "set pins, 2",
        "set pins, 3",
        "set pins, 4",
        "set pins, 5",
        "nop"
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_set_pins(24, 5);
    sm_a.config_set_out_pins(24, 5);
    sm_a.config_set_sideset(0, false, false);
    sm_a.config_set_clkdiv(8.25);
    sm_a.config_set_out_shift(false, true, 16);
    sm_a.sm_init(a_prog.entry());
    // run the loop on A
    report_api(0x0133_1111);
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
    delay(50);
    assert!(sm_a.pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 0);

    let mut a = pio::Assembler::<32>::new();
    a.out(pio::OutDestination::PINS, 16);
    let p = a.assemble_program();

    // this should stall the state machine
    sm_a.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
    report_api(0x0133_2222);
    delay(50);
    assert!(sm_a.pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 1);

    // this should clear the stall
    sm_a.pio.rmwf(rp_pio::SFR_CTRL_RESTART, sm_a.sm_bitmask());
    while (sm_a.pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
        // wait for the bits to self-reset to acknowledge that the command has executed
    }
    report_api(0x0133_3333);
    delay(50);
    assert!(sm_a.pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 0);

    // this should stall the state machine again
    sm_a.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
    report_api(0x0133_4444);
    delay(50);
    assert!(sm_a.pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 1);

    // this should also clear the stall by resolving the halt condition with a tx_fifo push
    sm_a.sm_txfifo_push_u16_msb(0xFFFF);
    report_api(0x0133_5555);
    delay(50);
    assert!(sm_a.pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 0);

    sm_a.pio.wo(rp_pio::SFR_CTRL, 0);
    pio_ss.clear_instruction_memory();
    report_api(0x0133_600d);
}

pub fn fifo_join_test() -> bool {
    report_api(0xF1F0_0000);
    #[cfg(feature = "rp2040")]
    let io_mask = 0x3FFF_FFFF;
    #[cfg(not(feature = "rp2040"))]
    let io_mask = 0xFFFF_FFFF;

    let mut pio_ss = PioSharedState::new();
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);

    // test TX fifo with non-join. Simple program that just copies the TX fifo content to pins, then stalls.
    let mut sm_a = pio_ss.alloc_sm().unwrap();
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "out pins, 32",
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    pio_ss.pio.wo(rp_pio::SFR_IRQ0_INTE, 0); // clear these in case a previous test set them
    pio_ss.pio.wo(rp_pio::SFR_IRQ1_INTE, 0);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_out_pins(0, 32);
    #[cfg(not(feature = "rp2040"))]
    sm_a.config_set_clkdiv(192.0); // slow down the machine so we can read out the values after writing them...
    #[cfg(feature = "rp2040")]
    sm_a.config_set_clkdiv(32768.0);
    sm_a.config_set_out_shift(false, true, 0);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_irq0_source_enabled(PioIntSource::TxNotFull, true);

    report_api(0xF1F0_1111);
    // load up the TX fifo, count how many entries it takes until it is full
    // note: full test requires manual inspection of waveform to confirm GPIO out has the expected report
    // value.
    let mut entries = 0;
    while !sm_a.sm_txfifo_is_full() {
        entries += 1;
        sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
    }
    assert!(entries == 4);
    let mut passing = true;
    report_api(0xF1F0_1000 + entries);
    // push the FIFO data out, and try to compare using PIO capture (clkdiv set slow so we can do this...)
    let mut last_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
    let mut detected = 0;
    // run the machine
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
    while detected < entries {
        let latest_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
        if latest_val != last_val {
            report_api(last_val);
            report_api(latest_val);
            detected += 1;
            if latest_val != ((0xF1F0_0000 + detected) & io_mask) {
                passing = false;
            }
            last_val = latest_val;
        }
    }

    // this should set Join TX and also halt the engine
    sm_a.config_set_fifo_join(PioFifoJoin::JoinTx);
    sm_a.sm_init(a_prog.entry());

    // repeat, this time measuring the depth of the FIFO with join
    let mut entries = 0;
    while !sm_a.sm_txfifo_is_full() {
        entries += 1;
        sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
    }
    report_api(0xF1F0_2000 + entries);
    assert!(entries == 8);
    // should push the FIFO out
    last_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
    detected = 0;
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
    while detected < entries {
        let latest_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
        if latest_val != last_val {
            detected += 1;
            if latest_val != ((0xF1F0_0000 + detected) & io_mask) {
                passing = false;
            }
            last_val = latest_val;
        }
    }

    // this should reset join TX and also halt the engine
    sm_a.config_set_fifo_join(PioFifoJoin::None);
    sm_a.sm_init(a_prog.entry());

    // ----------- now test with "margin" on the FIFOs. ----------
    #[cfg(not(feature = "rp2040"))]
    {
        assert!(sm_a.sm_get_tx_fifo_margin() == 0);
        sm_a.sm_set_tx_fifo_margin(1);
        assert!(sm_a.sm_get_tx_fifo_margin() == 1);
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::TxNotFull)));

        // repeat, this time measuring the depth of the FIFO with margin
        let mut entries = 0;
        // loop looks at the raw interrupt value, the asserts look at the feedback INTS value, so we have
        // coverage of both views
        while (pio_ss.pio.rf(rp_pio::SFR_INTR_INTR_TXNFULL) & sm_a.sm_bitmask()) != 0 {
            entries += 1;
            sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
        }
        assert!(entries == 3);
        assert!(sm_a.sm_txfifo_level() == 3); // should have space for one more item.
        assert!(sm_a.sm_txfifo_is_full() == false); // the actual "full" signal should not be asserted.
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::TxNotFull)) == false);
        report_api(0xF1F0_2100 + entries);
        // push one more entry in, to simulate the DMA overrun case
        entries += 1;
        sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
        assert!(sm_a.sm_txfifo_level() == 4);
        assert!(sm_a.sm_txfifo_is_full() == true);
        assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXOVER) == 0); // should not indicate overflow

        // push the FIFO data out, and try to compare using PIO capture (clkdiv set slow so we can do this...)
        let mut last_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
        let mut detected = 0;
        // run the machine
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
        while detected < entries {
            let latest_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
            if latest_val != last_val {
                detected += 1;
                if latest_val != ((0xF1F0_0000 + detected) & io_mask) {
                    passing = false;
                }
                last_val = latest_val;
            }
        }
        report_api(0xF1F0_2100 + if passing { 1 } else { 0 });
        sm_a.sm_set_tx_fifo_margin(0);

        // this should reset join TX and also halt the engine
        sm_a.config_set_fifo_join(PioFifoJoin::JoinTx);
        sm_a.sm_init(a_prog.entry());

        // ----------- now test with "margin" on the FIFOs. ----------
        assert!(sm_a.sm_get_tx_fifo_margin() == 0);
        sm_a.sm_set_tx_fifo_margin(1);
        assert!(sm_a.sm_get_tx_fifo_margin() == 1);
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::TxNotFull)));

        // repeat, this time measuring the depth of the FIFO with margin
        let mut entries = 0;
        // loop looks at the raw interrupt value, the asserts look at the feedback INTS value, so we have
        // coverage of both views
        while (sm_a.pio.rf(rp_pio::SFR_INTR_INTR_TXNFULL) & sm_a.sm_bitmask()) != 0 {
            entries += 1;
            sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
        }
        report_api(0xF1F0_2200 + entries);
        assert!(entries == 7);
        assert!(sm_a.sm_rxfifo_level() == 3); // should have space for one more item.
        assert!(sm_a.sm_txfifo_level() == 4); // this one should be full
        assert!(sm_a.sm_txfifo_is_full() == false); // the actual "full" signal should not be asserted.
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::TxNotFull)) == false);
        // push one more entry in, to simulate the DMA overrun case
        entries += 1;
        sm_a.sm_txfifo_push_u32(0xF1F0_0000 + entries);
        assert!(sm_a.sm_txfifo_level() == 4);
        assert!(sm_a.sm_rxfifo_level() == 4);
        assert!(sm_a.sm_txfifo_is_full() == true);
        assert!(pio_ss.pio.rf(rp_pio::SFR_FDEBUG_TXOVER) == 0); // should not indicate overflow

        // push the FIFO data out, and try to compare using PIO capture (clkdiv set slow so we can do this...)
        let mut last_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
        let mut detected = 0;
        // run the machine
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
        while detected < entries {
            let latest_val = sm_a.pio.r(rp_pio::SFR_DBG_PADOUT);
            if latest_val != last_val {
                detected += 1;
                if latest_val != ((0xF1F0_0000 + detected) & io_mask) {
                    passing = false;
                }
                last_val = latest_val;
            }
        }
    }
    sm_a.sm_irq0_source_enabled(PioIntSource::TxNotFull, false);
    #[cfg(not(feature = "rp2040"))]
    sm_a.sm_set_tx_fifo_margin(0);
    sm_a.sm_irq0_source_enabled(PioIntSource::RxNotEmpty, true);

    // a program for testing IN
    #[rustfmt::skip]
    let b_code = pio_proc::pio_asm!(
        "   set x, 16",
        "loop: ",
        "   in x, 32",
        "   push block",
        "   jmp x--, loop",
    );
    let b_prog = LoadedProg::load(b_code.program, &mut pio_ss).unwrap();

    // setup for rx test
    sm_a.sm_set_enabled(false);
    b_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_fifo_join(PioFifoJoin::None);
    sm_a.config_set_clkdiv(16.0);

    sm_a.sm_init(b_prog.entry());
    // start the program running
    report_api(0xF1F0_3333);
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
    while !sm_a.sm_rxfifo_is_full() {
        // just wait until the rx fifo fill sup
    }
    // stop filling it
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, 0);
    entries = 0;
    let mut expected = 16;
    while !sm_a.sm_rxfifo_is_empty() {
        let val = sm_a.sm_rxfifo_pull_u32();
        if val != expected {
            passing = false;
        }
        report_api(0xF1F0_0000 + val);
        entries += 1;
        expected -= 1;
    }
    report_api(0xF1F0_3000 + entries);
    assert!(entries == 4);

    // now join
    sm_a.config_set_fifo_join(PioFifoJoin::JoinRx);
    sm_a.sm_init(b_prog.entry());
    // start the program running
    report_api(0xF1F0_4444);
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
    while !sm_a.sm_rxfifo_is_full() {
        // just wait until the rx fifo fills up
    }
    // stop filling it
    sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, 0);
    entries = 0;
    expected = 16;
    while !sm_a.sm_rxfifo_is_empty() {
        let val = sm_a.sm_rxfifo_pull_u32();
        if val != (expected & io_mask) {
            passing = false;
        }
        report_api(0xF1F0_0000 + val);
        entries += 1;
        expected -= 1;
    }
    assert!(entries == 8);
    report_api(0xF1F0_4000 + entries);

    // no join, but with margin
    #[cfg(not(feature = "rp2040"))]
    {
        sm_a.config_set_fifo_join(PioFifoJoin::None);
        sm_a.sm_init(b_prog.entry());

        // now test with "margin" on the FIFOs.
        assert!(sm_a.sm_get_rx_fifo_margin() == 0);
        sm_a.sm_set_rx_fifo_margin(1);
        assert!(sm_a.sm_get_rx_fifo_margin() == 1);
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == false);

        // start the program running
        report_api(0xF1F0_4555);
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
        while !sm_a.sm_rxfifo_is_full() {
            // just wait until the rx fifo fills up
        }
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == true);
        // stop filling it
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, 0);
        entries = 0;
        expected = 16;
        while (pio_ss.pio.rf(rp_pio::SFR_INTR_INTR_RXNEMPTY) & sm_a.sm_bitmask()) != 0 {
            let val = sm_a.sm_rxfifo_pull_u32();
            if val != (expected & io_mask) {
                passing = false;
            }
            report_api(0xF1F0_0000 + val);
            entries += 1;
            expected -= 1;
        }
        assert!(entries == 3);
        assert!(sm_a.sm_rxfifo_level() == 1); // should be exactly one entry left
        assert!(sm_a.sm_rxfifo_is_empty() == false); // the actual "empty" signal should not be asserted.
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == false);
        report_api(0xF1F0_4100 + entries);
        sm_a.sm_set_rx_fifo_margin(0);

        // join, but with margin
        sm_a.config_set_fifo_join(PioFifoJoin::JoinRx);
        sm_a.sm_init(b_prog.entry());

        // now test with "margin" on the FIFOs.
        assert!(sm_a.sm_get_rx_fifo_margin() == 0);
        sm_a.sm_set_rx_fifo_margin(1);
        assert!(sm_a.sm_get_rx_fifo_margin() == 1);
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == false);

        // start the program running
        report_api(0xF1F0_4666);
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, sm_a.sm_bitmask());
        while !sm_a.sm_rxfifo_is_full() {
            // just wait until the rx fifo fills up
        }
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == true);
        // stop filling it
        sm_a.pio.wfo(rp_pio::SFR_CTRL_EN, 0);
        entries = 0;
        expected = 16;
        while (pio_ss.pio.rf(rp_pio::SFR_INTR_INTR_RXNEMPTY) & sm_a.sm_bitmask()) != 0 {
            let val = sm_a.sm_rxfifo_pull_u32();
            if val != (expected & io_mask) {
                passing = false;
            }
            report_api(0xF1F0_0000 + val);
            entries += 1;
            expected -= 1;
        }
        report_api(0xF1F0_4200 + entries);
        assert!(entries == 7);
        assert!(sm_a.sm_rxfifo_level() == 1); // this one should have one entry left
        assert!(sm_a.sm_txfifo_level() == 0); // should be empty
        assert!(sm_a.sm_rxfifo_is_empty() == false); // the actual "empty" signal should not be asserted.
        assert!(sm_a.sm_irq0_status(Some(PioIntSource::RxNotEmpty)) == false);
    }

    // clean up
    sm_a.sm_irq0_source_enabled(PioIntSource::RxNotEmpty, false);
    #[cfg(not(feature = "rp2040"))]
    sm_a.sm_set_rx_fifo_margin(0);
    pio_ss.clear_instruction_memory();

    if passing {
        report_api(0xF1F0_600D);
    } else {
        report_api(0xF1F0_DEAD);
    }
    assert!(passing); // stop the test bench if there was a failure
    passing
}

/// A test designed to exercise as much of the APB register interface as we can.
///
/// The test sets up four SM's to run simultaneously. SM3 does the "master sync"
/// with an IRQ instruction. After that point, all four should update their respective
/// GPIO pins simultaneously, and then wait for a `1` on GPIO 31.
///
/// The value sent to the GPIO pins should be pre-loaded into the TX fifo before
/// the test runs. The loop also puts the value of a loop counter into the RX fifo
/// as the loop runs, so these can be read out and checked for correctness. The loop
/// counter for each test starts at a different offset, so, we can be sure there is
/// no cross-wiring of registers or FIFOS by checking the offsets.
///
/// The interlocking of the test means we can also read the FIFO empty/full bits and
/// do asserts on them throughout the test.
///
/// The input/output registers have to be configured carefully, because each of the
/// code loops is constructed slightly differently to exercise different corner cases
/// of the input/output configurations. The pin mapping is as follows:
///
/// GPIO#   Input    Output
/// 0..4             SM0 TX fifo readout LSBs
/// 4..8             SM1 TX fifo readout LSBs
/// 8..12            SM2 TX fifo readout LSBs
/// 12..16           SM3 TX fifo readout LSBs
/// 14..16           SM3 sideset -- deliberately conflicting with SM3 TX fifo readout to test sideset > out
/// 16..18           SM0 sideset
/// 18..20           SM1 sideset via pindirs
/// 20..22           SM2 sideset
/// 31               synchronizing GPIO input

pub fn register_tests() {
    const REGTEST_DIV: f32 = 2.5;
    report_api(0x1336_0000);

    let mut pio_ss = PioSharedState::new();
    pio_ss.clear_instruction_memory();
    pio_ss.pio.rmwf(rp_pio::SFR_CTRL_EN, 0);

    let mut sm_a = pio_ss.alloc_sm().unwrap();
    sm_a.pio.wo(rp_pio::SFR_CTRL, 0xFF0); // reset all state machines to a known state.
    sm_a.pio.wo(rp_pio::SFR_FDEBUG, 0xFFFF_FFFF); // clear all the FIFO debug registers

    #[cfg(not(feature="rp2040"))]
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        ".side_set 2 opt",
        "   set x, 24",                 // 18
        "loop: ",
        "   in x, 0        side 3 [2]", // 19 0 maps to 32
        "   push block            [1]", // 1A puts X into the output FIFO
        "   wait 1 irq 1   side 2",     // 1B wait until IRQ0 is set to 1
        "   out pins, 32   side 1",     // 1C now push OSR onto the GPIO pins
        "   wait 1 gpio 31 side 0",     // 1D wait until GPIO 31 is 1
        "   wait 0 gpio 31 side 1",     // 1E wait until GPIO 31 is 0
        "   jmp x--, loop",             // 1F
    );
    #[cfg(feature="rp2040")]
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        ".side_set 2 opt",
        "   set x, 24",                 // 18
        "loop: ",
        "   in x, 0        side 3 [2]", // 19 0 maps to 32
        "   push block            [1]", // 1A puts X into the output FIFO
        "   wait 1 irq 1   side 2",     // 1B wait until IRQ0 is set to 1
        "   out pins, 32   side 1",     // 1C now push OSR onto the GPIO pins
        "   wait 1 gpio 28 side 0",     // 1D wait until GPIO 28 is 1
        "   wait 0 gpio 28 side 1",     // 1E wait until GPIO 28 is 0
        "   jmp x--, loop",             // 1F
    );
    report_api(a_code.program.side_set.bits() as u32);
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_out_pins(0, 4);
    sm_a.config_set_in_pins(16); // should have no impact on "wait" because it is absolutely specified
    sm_a.config_set_out_shift(false, true, 0);
    sm_a.config_set_sideset_pins(16);
    sm_a.config_set_clkdiv(REGTEST_DIV);
    #[cfg(not(feature = "rp2040"))]
    sm_a.config_set_set_pins(31, 1); // special case, A is used to set GPIO 31 to resume machines on wait
    #[cfg(feature = "rp2040")]
    sm_a.config_set_set_pins(28, 1); // special case, A is used to set GPIO 31 to resume machines on wait
    sm_a.sm_init(a_prog.entry());
    #[rustfmt::skip]
    let b_code = pio_proc::pio_asm!(
        ".side_set 2 opt pindirs",
        "   set y, 16 [1]",             // 10 start with some variable delay to prove syncing works
        "loop: ",
        "   in y, 0        side 3 [1]", // 11 0 maps to 32
        "   push block            [1]", // 12 puts Y into the output FIFO
        "   wait 1 irq 1   side 2",     // 13 wait until IRQ0 is set to 1
        "   out pins, 32   side 1",     // 14 now push OSR onto the GPIO pins
        "   wait 1 pin 0   side 0",     // 15 wait until the mapped input pin is 1. Map this to GPIO 31.
        "   wait 0 pin 0   side 1",     // 16 wait until the mapped input pin is 0. Map this to GPIO 31.
        "   jmp y--, loop",             // 17
    );
    let mut sm_b = pio_ss.alloc_sm().unwrap();
    let b_prog = LoadedProg::load(b_code.program, &mut pio_ss).unwrap();
    sm_b.sm_set_enabled(false);
    b_prog.setup_default_config(&mut sm_b);
    sm_b.config_set_out_pins(4, 4);
    #[cfg(not(feature = "rp2040"))]
    sm_b.config_set_in_pins(31); // maps pin 0 to GPIO 31
    #[cfg(feature = "rp2040")]
    sm_b.config_set_in_pins(28); // maps pin 0 to GPIO 28
    sm_b.config_set_out_shift(false, true, 0);
    sm_b.config_set_sideset_pins(18);
    sm_b.config_set_clkdiv(REGTEST_DIV);
    sm_b.sm_init(b_prog.entry());

    #[cfg(not(feature="rp2040"))]
    #[rustfmt::skip]
    let c_code = pio_proc::pio_asm!(
        ".side_set 2 opt",
        "   set x, 8 [2]",
        "loop: ",
        "   in x, 0        side 3 [1]", // 0 maps to 32
        "   push block            [2]", // puts X into the output FIFO
        "   wait 1 irq 1   side 2",     // wait until IRQ0 is set to 1
        "   out pins, 32   side 1",     // now push OSR onto the GPIO pins.
        "   wait 1 gpio 31 side 0",     // wait until GPIO 31 is 1
        "   wait 0 gpio 31 side 1",     // wait until GPIO 31 is 0
        "   jmp x--, loop",
    );
    #[cfg(feature="rp2040")]
    #[rustfmt::skip]
    let c_code = pio_proc::pio_asm!(
        ".side_set 2 opt",
        "   set x, 8 [2]",
        "loop: ",
        "   in x, 0        side 3 [1]", // 0 maps to 32
        "   push block            [2]", // puts X into the output FIFO
        "   wait 1 irq 1   side 2",     // wait until IRQ0 is set to 1
        "   out pins, 32   side 1",     // now push OSR onto the GPIO pins.
        "   wait 1 gpio 28 side 0",     // wait until GPIO 28 is 1
        "   wait 0 gpio 28 side 1",     // wait until GPIO 28 is 0
        "   jmp x--, loop",
    );
    let mut sm_c = pio_ss.alloc_sm().unwrap();
    let c_prog = LoadedProg::load(c_code.program, &mut pio_ss).unwrap();
    sm_c.sm_set_enabled(false);
    c_prog.setup_default_config(&mut sm_c);
    sm_c.config_set_out_pins(8, 4);
    sm_c.config_set_in_pins(24); // should not matter because absolute GPIO is used
    sm_c.config_set_out_shift(false, true, 0);
    sm_c.config_set_sideset_pins(20);
    sm_c.config_set_clkdiv(REGTEST_DIV);
    sm_c.sm_init(c_prog.entry());
    #[rustfmt::skip]
    let d_code = pio_proc::pio_asm!(
        ".side_set 2 opt",
        "   set y, 2  [3]",
        "loop: ",
        "   in y, 0        side 3 [2]", // 0 maps to 32
        "   push block",                // puts Y into the output FIFO
        "   irq set 1      side 2",     // set the IRQ, so all the machines sync here
        "   out pins, 32   side 1",     // now push OSR onto the GPIO pins. This one's side set interferes with GPIO mappings deliberately.
        "   wait 1 pin 7   side 0",     // wait until the mapped input pin is 1. Map this to GPIO 31.
        "   wait 0 pin 7   side 0",     // wait until the mapped input pin is 0. Map this to GPIO 31.
        "   jmp y--, loop",
    );
    let mut sm_d = pio_ss.alloc_sm().unwrap();
    let d_prog = LoadedProg::load(d_code.program, &mut pio_ss).unwrap();
    sm_d.sm_set_enabled(false);
    d_prog.setup_default_config(&mut sm_d);
    sm_d.config_set_out_pins(12, 4);
    #[cfg(not(feature = "rp2040"))]
    sm_d.config_set_in_pins(24); // maps pin 7 to GPIO 31
    #[cfg(feature = "rp2040")]
    sm_d.config_set_in_pins(21); // maps pin 7 to GPIO 28
    sm_d.config_set_out_shift(false, true, 0);
    sm_d.config_set_sideset_pins(14); // deliberate conflict with out_pins
    sm_d.config_set_clkdiv(REGTEST_DIV);
    sm_d.sm_init(d_prog.entry());

    // enable interrupts for readback on IRQ0
    sm_a.pio.wo(rp_pio::SFR_IRQ0_INTE, 0xFFF);

    report_api(0x1336_0001);

    // confirm that the FIFOs are all in expected states. Hard-coded as expected values for efficiency.
    assert!(sm_a.pio.r(SFR_FDEBUG) == 0);
    assert!(sm_a.pio.r(SFR_FLEVEL) == 0);
    assert!(sm_a.pio.r(SFR_FSTAT) == 0x0F00_0F00);
    assert!(sm_a.pio.r(SFR_DBG_CFGINFO) == 0x0020_0404); // hard-coded number, check that it's correct

    // dump the loaded instructions. Tests all the INSTR registers for readback.
    // must be re-generated every time programs are updated
    #[cfg(not(feature = "rp2040"))]
    let expected_instrs: [u16; 32] = [
        0xe342, 0x5e40, 0x8020, 0xd801, 0x7400, 0x30a7, 0x3027, 0x0081, 0xe228, 0x5d20, 0x8220, 0x38c1,
        0x7400, 0x309f, 0x341f, 0x0049, 0xe150, 0x5d40, 0x8120, 0x38c1, 0x7400, 0x30a0, 0x3420, 0x0091,
        0xe038, 0x5e20, 0x8120, 0x38c1, 0x7400, 0x309f, 0x341f, 0x0059,
    ];
    for i in 0..RP2040_MAX_PROGRAM_SIZE {
        let rbk = unsafe { sm_a.pio.base().add(rp_pio::SFR_INSTR_MEM0.offset() + i).read_volatile() };
        report_api(rbk + ((i as u32) << 24));
        #[cfg(not(feature = "rp2040"))]
        assert!(rbk as u16 == expected_instrs[i]); // on RP2040 you can't read back the instructions
    }
    report_api(0x1336_0002);

    // load the TX fifos with output data we expect to see on the GPIO pins
    let tx_vals: [[u32; 4]; 4] =
        [[0x3, 0xC, 0x6, 0x0], [0xA, 0x5, 0x0, 0xF], [0xC, 0x0, 0x1, 0x2], [0x3, 0x2, 0x1, 0x0]];
    // load the FIFO values, and check that the levels & flags change as we expected.
    let mut sm_array = [sm_a, sm_b, sm_c, sm_d];
    for (sm_index, sm) in sm_array.iter_mut().enumerate() {
        /* report_api(
            (sm_index as u32) << 16 |
            if sm.sm_txfifo_is_empty() {0x8000} else {0x0} |
            sm.sm_txfifo_level() as u32
        ); */
        assert!(sm.sm_txfifo_is_empty() == true);
        assert!(sm.sm_txfifo_level() == 0);
        // TXNFULL should be asserted
        assert!((sm.pio.r(rp_pio::SFR_IRQ0_INTS) >> 4) & sm.sm_bitmask() != 0);
        // RXNEMPTY should be de-asserted
        assert!((sm.pio.r(rp_pio::SFR_IRQ0_INTS) >> 0) & sm.sm_bitmask() == 0);
        for (index, &word) in tx_vals[sm_index].iter().enumerate() {
            sm.sm_txfifo_push_u32(word);
            // report_api(0x1336_0031);
            assert!(sm.sm_txfifo_is_empty() == false);
            // report_api(0x1336_0000 + sm.sm_txfifo_level() as u32);
            assert!(sm.sm_txfifo_level() == index + 1);
            // report_api(0x1336_0033);
        }
        // report_api(0x1336_0004);
        assert!(sm.sm_txfifo_is_full() == true);
        // TXNFULL should be de-asserted
        assert!((sm.pio.r(rp_pio::SFR_IRQ0_INTS) >> 4) & sm.sm_bitmask() == 0);
        // push an extra value and confirm that we cause an overflow
        sm.sm_txfifo_push_u8_msb(0x7); // Note: this number does not appear in the loaded set
        // confirm that we see the overflow flag; then clear it, and confirm it's cleared.
        report_api(0x1336_0005);
        assert!(sm.pio.rf(rp_pio::SFR_FDEBUG_TXOVER) == sm.sm_bitmask());
        sm.pio.wfo(rp_pio::SFR_FDEBUG_TXOVER, sm.sm_bitmask());
        report_api(0x1336_0006);
        assert!(sm.pio.rf(rp_pio::SFR_FDEBUG_TXOVER) == 0);
    }

    // prepare instruction to flip OE on bit 31 to move the state machine forward
    // requires testbench to wire that bit back in as an input on GPIO 31 for the test to complete!!
    let set_bit31_oe = pio_proc::pio_asm!("set pindirs, 1").program.code[0];
    // program that clears the same bit
    let clear_bit31_oe = pio_proc::pio_asm!("set pindirs, 0").program.code[0];
    // prepare an instruction that stalls
    let p_wait = pio_proc::pio_asm!("wait 1 irq 0").program.code[0];
    // prepare an instruction that can set & clear the *pin*, not just the pindir. For RP2040 HW test, because
    // we can't hack the OE to IN as a loopback like we can in a verilog model.
    #[cfg(feature = "rp2040")]
    let pinset = pio_proc::pio_asm!("set pins, 1").program.code[0];
    #[cfg(feature = "rp2040")]
    let pinclear = pio_proc::pio_asm!("set pins, 0").program.code[0];

    // check that the RX FIFOs have the correct levels
    report_api(0x1336_0007);
    for sm in sm_array.iter_mut() {
        assert!(sm.sm_rxfifo_is_empty());
        assert!(sm.sm_rxfifo_level() == 0);
    }

    // start the machines running
    sm_array[0].pio.wo(
        rp_pio::SFR_CTRL,
        sm_array[0].pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, 0xF)
            | sm_array[0].pio.ms(rp_pio::SFR_CTRL_EN, 0xF),
    ); // sync the clocks; the clock free-runs after the div is setup, and the divs are set up at arbitrary points in time
    while (sm_array[0].pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
        // wait for the bits to self-reset to acknowledge that the command has executed
    }
    report_api(0x1336_0008);

    let mut waiting_for: usize = 0;
    let mut iters = 0;
    loop {
        if waiting_for >= tx_vals[0].len() {
            report_api(0x1336_000B);
            break;
        }
        report_api(waiting_for as u32);
        // assembled the expected value
        let mut expected = 0;
        for (index, vals) in tx_vals.iter().enumerate() {
            expected |= (vals[waiting_for] & 0xF) << (index as u32 * 4);
        }
        // compensate for sideset override on SM3 (doesn't apply, because the sideset is out of phase with
        // out) expected &= 0x3FFF;
        // expected |= 0x4000; // "side 1" should be executed on SM3 on bits 14-16, overriding any TX fifo
        // value

        let outputs = sm_array[0].pio.r(rp_pio::SFR_DBG_PADOUT);
        report_api(0x1336_0000 | (outputs & 0xFFFF));
        report_api(expected);
        report_api(sm_array[0].sm_address() as u32);
        report_api(sm_array[1].sm_address() as u32);
        report_api(sm_array[2].sm_address() as u32);
        report_api(sm_array[3].sm_address() as u32);
        if expected == (outputs & 0xFFFF) {
            // got it, moving forward
            waiting_for += 1;
            report_api(0x0000_1336 | ((waiting_for as u32) << 16)); // report waiting_for

            // check that RX fifos have the right number of entries
            for sm in sm_array.iter_mut() {
                assert!(sm.sm_rxfifo_level() == waiting_for);
            }
            // no "exec" is in progress, so the stall bit should not be set
            report_api(0x1336_0007);
            assert!(sm_array[0].pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 0);
            assert!(sm_array[1].pio.rf(rp_pio::SFR_SM1_EXECCTRL_EXEC_STALLED_RO1) == 0);
            assert!(sm_array[2].pio.rf(rp_pio::SFR_SM2_EXECCTRL_EXEC_STALLED_RO2) == 0);
            assert!(sm_array[3].pio.rf(rp_pio::SFR_SM3_EXECCTRL_EXEC_STALLED_RO3) == 0);

            // read the address of the PC, and confirm the instruction is correct
            report_api(0x1336_0008);
            #[cfg(not(feature = "rp2040"))] // can't read instructions on actual rp2040
            {
                assert!(
                    expected_instrs[sm_array[0].pio.rf(rp_pio::SFR_SM0_ADDR_PC) as usize]
                        == sm_array[0].pio.rf(rp_pio::SFR_SM0_INSTR_IMM_INSTR) as u16
                );
                assert!(
                    expected_instrs[sm_array[1].pio.rf(rp_pio::SFR_SM1_ADDR_PC) as usize]
                        == sm_array[1].pio.rf(rp_pio::SFR_SM1_INSTR_IMM_INSTR) as u16
                );
                assert!(
                    expected_instrs[sm_array[2].pio.rf(rp_pio::SFR_SM2_ADDR_PC) as usize]
                        == sm_array[2].pio.rf(rp_pio::SFR_SM2_INSTR_IMM_INSTR) as u16
                );
                assert!(
                    expected_instrs[sm_array[3].pio.rf(rp_pio::SFR_SM3_ADDR_PC) as usize]
                        == sm_array[3].pio.rf(rp_pio::SFR_SM3_INSTR_IMM_INSTR) as u16
                );
            }

            // execute the "program" that flips the OE bit, which should get us to the next iteration
            report_api(0x1336_0009);
            // exec an instruction that can't complete
            sm_array[0].sm_exec(p_wait);
            // confirm that the stall bit is set
            report_api(0x1336_000A);
            assert!(sm_array[0].pio.rf(rp_pio::SFR_SM0_EXECCTRL_EXEC_STALLED_RO0) == 1);

            // now exec the instruction that should clear the wait condition on an input pin by flipping bit
            // 31 via a side-set operation this requires the testbench to reflect that bit back
            // correctly!
            #[cfg(feature = "rp2040")]
            sm_array[0].sm_exec(pinset);
            sm_array[0].sm_exec(set_bit31_oe);
            #[cfg(feature = "rp2040")]
            sm_array[0].sm_exec(pinclear);
            sm_array[0].sm_exec(clear_bit31_oe);
            delay(4); // give some time for the state machines to run to the stall point (necessary for fast pclk case)
        }
        iters += 1;
        if iters > 20 {
            assert!(false);
        }
    }
    // stop the machine from running, so we can test RX fifo underflow, etc.
    report_api(0x1336_000C);
    sm_array[0].pio.wfo(rp_pio::SFR_CTRL_EN, 0);

    // since the program ran one extra iteration out the bottom of the loop, we should have overflowed the RX
    // fifo, etc.
    report_api(sm_array[0].pio.r(rp_pio::SFR_FDEBUG));
    assert!(sm_array[0].pio.r(rp_pio::SFR_FDEBUG) == 0xF); // stalls should be asserted
    sm_array[0].pio.wfo(rp_pio::SFR_FDEBUG_RXSTALL, 0xF); // clear the stall
    assert!(sm_array[0].pio.r(rp_pio::SFR_FDEBUG) == 0); // confirm it is cleared

    // read back the FIFOs and check that the correct values were committed
    report_api(0x1336_000D);
    let loop_ivs = [24u8, 16u8, 8u8, 2u8];
    let mut loop_counters = [0u8; 4];
    loop_counters.copy_from_slice(&loop_ivs);
    for expected_fifo_level in (1..=4).rev() {
        report_api(0x1336_001D | (expected_fifo_level as u32) << 8);
        for (sm_index, sm) in sm_array.iter_mut().enumerate() {
            // RXNEMPTY should be asserted
            assert!((sm.pio.r(rp_pio::SFR_IRQ0_INTS) >> 0) & sm.sm_bitmask() != 0);

            // check that the fifo level is correct
            assert!(sm.sm_rxfifo_level() == expected_fifo_level);
            // check that the index matched
            let rxval = sm.sm_rxfifo_pull_u8_lsb();
            report_api(0x1336_001D | (rxval as u32) << 8);
            assert!(rxval == loop_counters[sm_index]);
        }
        // update the expected indices
        report_api(0x1336_002D | (expected_fifo_level as u32) << 8);
        for (index, loop_counter) in loop_counters.iter_mut().enumerate() {
            if *loop_counter != 0 {
                *loop_counter -= 1;
            } else {
                *loop_counter = loop_ivs[index];
            }
        }
    }
    // check that the RX fifos are empty and no underflow, we should be "just nice"
    report_api(0x1336_000E);
    assert!(sm_array[0].pio.rf(rp_pio::SFR_FDEBUG_RXUNDER) == 0);
    let mut expected_underflows = 0;
    for sm in sm_array.iter_mut() {
        assert!(sm.sm_rxfifo_level() == 0);
        // now do an extra pull
        let _ = sm.sm_rxfifo_pull_u8_lsb();
        assert!(sm.sm_rxfifo_level() == 0);
        expected_underflows |= sm.sm_bitmask();
        assert!(sm.pio.rf(rp_pio::SFR_FDEBUG_RXUNDER) == expected_underflows);
    }
    // clear all the FIFOs and check default states
    // we also clear the RXUNDER bit progressively and confirm that it can be incrementally cleared
    // (this checks the action register implementation is bit-wise and not register-wide)
    report_api(0x1336_000F);
    for sm in sm_array.iter_mut() {
        sm.sm_clear_fifos();
        assert!(sm.sm_rxfifo_level() == 0);
        assert!(sm.sm_txfifo_level() == 0);
        assert!(sm.sm_rxfifo_is_empty());
        assert!(sm.sm_txfifo_is_empty());
        sm.pio.wfo(rp_pio::SFR_FDEBUG_RXUNDER, sm.sm_bitmask());
        expected_underflows &= !(sm.sm_bitmask());
        assert!(sm.pio.rf(rp_pio::SFR_FDEBUG_RXUNDER) == expected_underflows);
    }

    report_api(0x1336_600d);
}
