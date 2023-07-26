use crate::pio_generated::utra::rp_pio;
use crate::*;
use super::report_api;

pub fn i2c_init(
    pio_sm: &mut PioSm,
    program: &LoadedProg,
    pin_sda: usize,
    pin_scl: usize,
) {
    pio_sm.sm_set_enabled(false);
    program.setup_default_config(pio_sm);

    pio_sm.config_set_out_pins(pin_sda, 1);
    pio_sm.config_set_set_pins(pin_sda, 1);
    pio_sm.config_set_in_pins(pin_sda);
    pio_sm.config_set_sideset_pins(pin_scl);
    pio_sm.config_set_jmp_pin(pin_sda);

    pio_sm.config_set_out_shift(false, true, 16);
    pio_sm.config_set_in_shift(false, true, 8);
    let div: f32 = 800_000_000.0 / (32.0 * 1_000_000.0);
    pio_sm.config_set_clkdiv(div);
    // require: use external pull-up
    let both_pins = (1 << pin_sda) | (1 << pin_scl);
    pio_sm.sm_set_pins_with_mask(both_pins, both_pins);
    pio_sm.sm_set_pindirs_with_mask(both_pins, both_pins);
    // reset the overrides in case previous test had set them and not cleared them
    pio_sm.pio.wo(rp_pio::SFR_IO_OE_INV, 0);
    pio_sm.pio.wo(rp_pio::SFR_IO_O_INV, 0);
    pio_sm.pio.wo(rp_pio::SFR_IO_I_INV, 0);
    pio_sm.gpio_set_oeover(pin_sda, true);
    pio_sm.gpio_set_oeover(pin_scl, true);
    pio_sm.sm_set_pins_with_mask(0, both_pins);

    pio_sm.sm_irq0_source_enabled(PioIntSource::Sm, false);
    pio_sm.sm_irq1_source_enabled(PioIntSource::Sm, true);
    // reset this because prior tests might set this
    pio_sm.config_set_fifo_join(PioFifoJoin::None);

    pio_sm.sm_init(program.entry());
    pio_sm.sm_set_enabled(true);
}
pub fn i2c_check_error(pio_sm: &mut PioSm) -> bool {
    pio_sm.sm_interrupt_get(pio_sm.sm_index())
}
pub fn i2c_resume_after_error(pio_sm: &mut PioSm) {
    pio_sm.sm_drain_tx_fifo();
    pio_sm.sm_jump_to_wrap_bottom();
    // this will clear the IRQ set by the current SM, relying on the fact that sm's encoding is a bitmask
    pio_sm.pio.wfo(rp_pio::SFR_IRQ_SFR_IRQ, pio_sm.sm_bitmask());
    while i2c_check_error(pio_sm) {
        // wait for the IRQ to report cleared. This in not necessary on the RP2040, but because
        // our core can run much faster than the PIO we have to wait for the PIO to catch up to the core's state.
    }
}
pub fn i2c_rx_enable(pio_sm: &mut PioSm, en: bool) {
    let sm_offset = pio_sm.sm_to_stride_offset();
    unsafe {
        let baseval = pio_sm.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).read_volatile();
        let bitval = pio_sm.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, 1);
        pio_sm.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).write_volatile(
            baseval & !bitval
            | if en {bitval} else {0}
        );
    }
}
pub fn i2c_put16(pio_sm: &mut PioSm, data: u16) {
    while pio_sm.sm_txfifo_is_full() {
        // wait
    }
    pio_sm.sm_txfifo_push_u16_msb(data);
}
pub fn i2c_put_or_err(pio_sm: &mut PioSm, data: u16) {
    while pio_sm.sm_txfifo_is_full() {
        if i2c_check_error(pio_sm) {
            return
        }
    }
    if i2c_check_error(pio_sm) {
        return;
    }
    pio_sm.sm_txfifo_push_u16_msb(data);
}
pub fn i2c_get(pio_sm: &mut PioSm) -> u8 {
    pio_sm.sm_rxfifo_pull_u8_lsb() as u8
}
const PIO_I2C_ICOUNT_LSB: u16 = 10;
const PIO_I2C_FINAL_LSB: u16  = 9;
#[allow(dead_code)]
const PIO_I2C_DATA_LSB: u16   = 1;
const PIO_I2C_NAK_LSB: u16    = 0;
const I2C_SC0_SD0: usize = 0;
#[allow(dead_code)]
const I2C_SC0_SD1: usize = 1;
const I2C_SC1_SD0: usize = 2;
const I2C_SC1_SD1: usize = 3;
pub fn i2c_start(pio_sm: &mut PioSm, set_scl_sda_program_instructions: &[u16; 4]) {
    i2c_put_or_err(pio_sm, 1 << PIO_I2C_ICOUNT_LSB);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC1_SD0]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC0_SD0]);
}
pub fn i2c_stop(pio_sm: &mut PioSm, set_scl_sda_program_instructions: &[u16; 4]) {
    i2c_put_or_err(pio_sm, 2 << PIO_I2C_ICOUNT_LSB);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC0_SD0]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC1_SD0]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC1_SD1]);
}
#[allow(dead_code)]
pub fn i2c_repstart(pio_sm: &mut PioSm, set_scl_sda_program_instructions: &[u16; 4]) {
    i2c_put_or_err(pio_sm, 3 << PIO_I2C_ICOUNT_LSB);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC0_SD1]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC1_SD1]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC1_SD0]);
    i2c_put_or_err(pio_sm, set_scl_sda_program_instructions[I2C_SC0_SD0]);
}
pub fn i2c_wait_idle(pio_sm: &mut PioSm) {
    pio_sm.pio.wfo(rp_pio::SFR_FDEBUG_TXSTALL, pio_sm.sm_bitmask());
    while ((pio_sm.pio.rf(rp_pio::SFR_FDEBUG_TXSTALL) & pio_sm.sm_bitmask()) == 0)
    || i2c_check_error(pio_sm) {
        // busy loop
    }
}
/// returns false if there is an error; true if no error
#[allow(dead_code)]
pub fn i2c_write_blocking(pio_sm: &mut PioSm, set_scl_sda_program_instructions: &[u16; 4], addr: u8, txbuf: &[u8]) -> bool {
    i2c_start(pio_sm, set_scl_sda_program_instructions);
    i2c_rx_enable(pio_sm, false);
    i2c_put16(pio_sm, ((addr as u16) << 2) | 1);
    let mut txbuf_iter = txbuf.iter().peekable();
    loop {
        if i2c_check_error(pio_sm) {
            break;
        }
        if !pio_sm.sm_txfifo_is_full() {
            if let Some(&d) = txbuf_iter.next() {
                i2c_put_or_err(pio_sm,
                    (d as u16) << PIO_I2C_DATA_LSB |
                    if txbuf_iter.peek().is_none() { 1 } else { 0 }
                );
            } else {
                break;
            }
        }
    }
    i2c_stop(pio_sm, set_scl_sda_program_instructions);
    i2c_wait_idle(pio_sm);
    if i2c_check_error(pio_sm) {
        i2c_resume_after_error(pio_sm);
        i2c_stop(pio_sm, set_scl_sda_program_instructions);
        false
    } else {
        true
    }
}
/// returns false if there is an error; true if no error
pub fn i2c_read_blocking(pio_sm: &mut PioSm, set_scl_sda_program_instructions: &[u16; 4], addr: u8, rxbuf: &mut [u8]) -> bool {
    report_api(0x12C0_0000);

    i2c_start(pio_sm, set_scl_sda_program_instructions);
    report_api(0x12C0_0001);
    i2c_rx_enable(pio_sm, true);
    report_api(0x12C0_0002);
    while !pio_sm.sm_rxfifo_is_empty() {
        i2c_get(pio_sm);
    }
    report_api(0x12C0_0003);
    let addr_composed = ((addr as u16) << 2)
        | 2  // "read address"
        | 1 << PIO_I2C_NAK_LSB;
    report_api(0x12C0_0000 | addr_composed as u32);
    i2c_put16(pio_sm, addr_composed);
    report_api(0x12C0_0004);
    let mut first = true;
    let mut tx_remain = rxbuf.len();
    let mut len = rxbuf.len();
    let mut i = 0;
    while (tx_remain != 0) || (len != 0) && !i2c_check_error(pio_sm) {
        report_api(0x12C0_0000 + ((tx_remain as u32) << 8) | len as u32);
        if (tx_remain != 0) && !pio_sm.sm_txfifo_is_full() {
            tx_remain -= 1;
            i2c_put16(pio_sm,
                (0xff << 1)
                | if tx_remain != 0 { 0 } else {
                    1 << PIO_I2C_FINAL_LSB
                    | 1 << PIO_I2C_NAK_LSB
                }
            );
        }
        if !pio_sm.sm_rxfifo_is_empty() {
            if first {
                i2c_get(pio_sm);
                first = false;
            } else {
                len -= 1;
                rxbuf[i] = i2c_get(pio_sm);
                i += 1;
            }
        }
        if pio_sm.sm_irq1_status(Some(PioIntSource::Sm)) {
            report_api(0x12C0_1111);
            // detects NAK and aborts transaction
            i2c_resume_after_error(pio_sm);
            i2c_stop(pio_sm, set_scl_sda_program_instructions);
            return false;
        }
    }
    if i2c_check_error(pio_sm) {
        report_api(0x12C0_2222);
        report_api(0x12C0_0000 + rxbuf[0] as u32);
        i2c_resume_after_error(pio_sm);
        i2c_stop(pio_sm, set_scl_sda_program_instructions);
        false
    } else {
        report_api(0x12C0_0020);
        i2c_stop(pio_sm, set_scl_sda_program_instructions);
        report_api(0x12C0_0021);
        i2c_wait_idle(pio_sm);
        report_api(0x12C0_0000);
        report_api(0x12C0_0000 + rxbuf[0] as u32);
        true
    }
}
pub fn i2c_test() -> bool {
    const PIN_SDA: usize = 2;
    const PIN_SCL: usize = 3;

    report_api(0x0D10_012C);

    let mut pio_ss = PioSharedState::new();
    let mut pio_sm = pio_ss.alloc_sm().unwrap();
    pio_sm.sm_set_enabled(false); // stop the machine from running so we can initialize it

    let i2c_prog = pio_proc::pio_asm!(
        ".side_set 1 opt pindirs",

        // TX Encoding:
        // | 15:10 | 9     | 8:1  | 0   |
        // | Instr | Final | Data | NAK |
        //
        // If Instr has a value n > 0, then this FIFO word has no
        // data payload, and the next n + 1 words will be executed as instructions.
        // Otherwise, shift out the 8 data bits, followed by the ACK bit.
        //
        // The Instr mechanism allows stop/start/repstart sequences to be programmed
        // by the processor, and then carried out by the state machine at defined points
        // in the datastream.
        //
        // The "Final" field should be set for the final byte in a transfer.
        // This tells the state machine to ignore a NAK: if this field is not
        // set, then any NAK will cause the state machine to halt and interrupt.
        //
        // Autopull should be enabled, with a threshold of 16.
        // Autopush should be enabled, with a threshold of 8.
        // The TX FIFO should be accessed with halfword writes, to ensure
        // the data is immediately available in the OSR.
        //
        // Pin mapping:
        // - Input pin 0 is SDA, 1 is SCL (if clock stretching used)
        // - Jump pin is SDA
        // - Side-set pin 0 is SCL
        // - Set pin 0 is SDA
        // - OUT pin 0 is SDA
        // - SCL must be SDA + 1 (for wait mapping)
        //
        // The OE outputs should be inverted in the system IO controls!
        // (It's possible for the inversion to be done in this program,
        // but costs 2 instructions: 1 for inversion, and one to cope
        // with the side effect of the MOV on TX shift counter.)

        "do_nack:",
        "    jmp y-- entry_point",        // 0D 0099 Continue if NAK was expected
        "    irq wait 0 rel",             // 0E C030 Otherwise stop, ask for help

        "do_byte:",
        "    set x, 7",                   // 0F E027 Loop 8 times
        "bitloop:",
        "    out pindirs, 1         [7]", // 10 6781 Serialise write data (all-ones if reading)
        "    nop             side 1 [2]", // 11 BA42 SCL rising edge
        "    wait 1 pin, 1          [4]", // 12 24A1 Allow clock to be stretched
        "    in pins, 1             [7]", // 13 4701 Sample read data in middle of SCL pulse
        "    jmp x-- bitloop side 0 [7]", // 14 1750 SCL falling edge

        // Handle ACK pulse
        "    out pindirs, 1         [7]", // 15 6781 On reads, we provide the ACK.
        "    nop             side 1 [7]", // 16 BF42 SCL rising edge
        "    wait 1 pin, 1          [7]", // 17 27A1 Allow clock to be stretched
        "    jmp pin do_nack side 0 [2]", // 18 12CD Test SDA for ACK/NAK, fall through if ACK

        "public entry_point:",
        ".wrap_target",
        "    out x, 6                  ", // 19 6026 Unpack Instr count
        "    out y, 1                  ", // 1A 6041 Unpack the NAK ignore bit
        "    jmp !x do_byte            ", // 1B 002F Instr == 0, this is a data record.
        "    out null, 32              ", // 1C 6060 Instr > 0, remainder of this OSR is invalid
        "do_exec:                      ",
        "    out exec, 16              ", // 1D 60F0 Execute one instruction per FIFO word
        "    jmp x-- do_exec           ", // 1E 005D Repeat n + 1 times
        ".wrap",
    );
    let ep = i2c_prog.public_defines.entry_point as usize;
    // report_api(i2c_prog.program.side_set.bits() as u32);
    let prog_i2c = LoadedProg::load_with_entrypoint(i2c_prog.program, ep, &mut pio_ss).unwrap();
    i2c_init(&mut pio_sm, &prog_i2c, PIN_SDA, PIN_SCL);
    report_api(0x012C_3333);

    let i2c_cmds_raw = pio_proc::pio_asm!(
        ".side_set 1 opt",
        // Assemble a table of instructions which software can select from, and pass
        // into the FIFO, to issue START/STOP/RSTART. This isn't intended to be run as
        // a complete program.
        "    set pindirs, 0 side 0 [7] ", // F780 SCL = 0, SDA = 0"
        "    set pindirs, 1 side 0 [7] ", // F781 SCL = 0, SDA = 1",
        "    set pindirs, 0 side 1 [7] ", // FF80 SCL = 1, SDA = 0",
        "    set pindirs, 1 side 1 [7] ", // FF81 SCL = 1, SDA = 1",
    ).program.code;
    let mut i2c_cmds = [0u16; 4];
    i2c_cmds.copy_from_slice(&i2c_cmds_raw[..4]);
    // print the compiled program for debug purposes
    for i in 0..4 {
        report_api(0x012C_0000 + i2c_cmds[i] as u32);
    }

    let mut rxbuf = [0u8];
    // expected pass or fails of reads, for automated regression testing
    // these are wired directly into the LiteX test harness inside the PioAdapter block
    let failing_address = 0x17 >> 1; // 0x17 is exactly what is inside the testbench code, shift right by one to disregard r/w bit
    let mut passing = true;
    for addr in 10..14 {
        report_api(0x012C_0000 + addr as u32);
        if i2c_read_blocking(&mut pio_sm, &i2c_cmds, addr, &mut rxbuf) {
            if addr == failing_address {
                passing = false;
            }
            report_api(0x012C_600D);
        } else {
            if addr != failing_address {
                passing = false;
            }
            report_api(0x012C_DEAD);
        }
        crate::pio_tests::units::delay(256);
    }
    report_api(0x012C_1111);

    // turn off interrupts after the test, otherwise this interferes with later operations
    pio_sm.sm_irq0_source_enabled(PioIntSource::Sm, false);
    pio_sm.sm_irq1_source_enabled(PioIntSource::Sm, false);

    assert!(passing);
    passing
}
