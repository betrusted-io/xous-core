use utralib::generated::utra::rp_pio;

use super::report_api;
use crate::*;

#[inline(always)]
pub fn pio_spi_write8_read8_blocking(pio_sm: &mut PioSm, src: &[u8], dst: &mut [u8]) {
    assert!(src.len() == dst.len(), "src and dst arrays are not the same length!");

    let mut src_iter = src.iter();
    let mut dst_iter_mut = dst.iter_mut().peekable();
    let mut tx_done = false;
    let mut rx_done = false;
    // this weirdness checks that the SPI machine stalls when RX FIFO is full, and no data is lost
    let mut rx_reached_full = false;
    loop {
        if !pio_sm.sm_txfifo_is_full() {
            if let Some(&s) = src_iter.next() {
                pio_sm.sm_txfifo_push_u8_msb(s);
            } else {
                tx_done = true;
            }
        }
        if !rx_reached_full && pio_sm.sm_rxfifo_is_full() {
            rx_reached_full = true;
            let level = pio_sm.sm_txfifo_level();
            while level != 0 && pio_sm.sm_txfifo_level() == level {
                // wait
            }
            for _ in 0..16 {
                // dummy reads to cause some delay to confirm RX full stalls the machine
                pio_sm.sm_txfifo_level();
            }
        }
        if rx_reached_full {
            if !pio_sm.sm_rxfifo_is_empty() {
                if let Some(d) = dst_iter_mut.next() {
                    *d = pio_sm.sm_rxfifo_pull_u8_lsb();
                }
            }
        }
        // always have to peek ahead at this, because
        // we won't ever reach this if we have to wait for the rxfifo
        // to be "not empty" before peeking at it (the last element
        // never generates a new pending element...
        if dst_iter_mut.peek().is_none() {
            rx_done = true;
        }
        if tx_done && rx_done {
            break;
        }
    }
}

pub fn spi_test_core(pio_sm: &mut PioSm) -> bool {
    report_api(0x0D10_05D1);

    const BUF_SIZE: usize = 20;
    let mut state: u16 = 0xAA;
    let mut tx_buf = [0u8; BUF_SIZE];
    let mut rx_buf = [0u8; BUF_SIZE];
    // init the TX buf
    for d in tx_buf.iter_mut() {
        state = crate::lfsr_next(state);
        *d = state as u8;
        report_api(*d as u32);
    }
    pio_spi_write8_read8_blocking(pio_sm, &tx_buf, &mut rx_buf);
    let mut pass = true;
    for (&s, &d) in tx_buf.iter().zip(rx_buf.iter()) {
        if s != d {
            report_api(0xDEAD_0000 | (s as u32) << 8 | ((d as u32) << 0));
            pass = false;
        }
    }
    report_api(0x600D_05D1);
    pass
}

#[inline(always)]
pub fn pio_spi_init(
    pio_sm: &mut PioSm,
    program: &LoadedProg,
    n_bits: usize,
    clkdiv: f32,
    cpol: bool,
    pin_sck: usize,
    pin_mosi: usize,
    pin_miso: usize,
) {
    pio_sm.sm_set_enabled(false);
    // this applies a default config to the PioSm object that is relevant to the program
    program.setup_default_config(pio_sm);

    pio_sm.config_set_out_pins(pin_mosi, 1);
    pio_sm.config_set_in_pins(pin_miso);
    pio_sm.config_set_sideset_pins(pin_sck);
    pio_sm.config_set_out_shift(false, true, n_bits);
    pio_sm.config_set_in_shift(false, true, n_bits);
    pio_sm.config_set_clkdiv(clkdiv);

    // MOSI, SCK output are low, MISO is input
    pio_sm.sm_set_pins_with_mask(0, (1 << pin_sck) | (1 << pin_mosi));
    pio_sm.sm_set_pindirs_with_mask(
        (1 << pin_sck) | (1 << pin_mosi),
        (1 << pin_sck) | (1 << pin_mosi) | (1 << pin_miso),
    );

    pio_sm.gpio_set_outover(pin_sck, cpol);

    // SPI is synchronous, so bypass input synchroniser to reduce input delay.
    pio_sm.pio.wo(rp_pio::SFR_SYNC_BYPASS, 1 << pin_miso);

    // reset this because prior tests might set this
    pio_sm.config_set_fifo_join(PioFifoJoin::None);

    // program origin should already be set by the loader. sm_init() also disables the engine.
    pio_sm.sm_init(program.start());
    pio_sm.sm_set_enabled(true);
}

pub fn spi_test() -> bool {
    const PIN_SCK: usize = 18;
    const PIN_MOSI: usize = 16;
    const PIN_MISO: usize = 16; // loopback

    report_api(0x0D10_05D1);

    let mut pio_ss = PioSharedState::new();
    let mut pio_sm = pio_ss.alloc_sm().unwrap();

    // spi_cpha0 example
    #[rustfmt::skip]
    let spi_cpha0_prog = pio_proc::pio_asm!(
        ".side_set 1",
        "out pins, 1 side 0 [1]",
        "in pins, 1  side 1 [1]",
    );
    // spi_cpha1 example
    #[rustfmt::skip]
    let spi_cpha1_prog = pio_proc::pio_asm!(
        ".side_set 1",
        "out x, 1    side 0", // Stall here on empty (keep SCK deasserted)
        "mov pins, x side 1 [1]", // Output data, assert SCK (mov pins uses OUT mapping)
        "in pins, 1  side 0" // Input data, deassert SCK
    );
    let prog_cpha0 = LoadedProg::load(spi_cpha0_prog.program, &mut pio_ss).unwrap();
    report_api(0x05D1_0000);
    let prog_cpha1 = LoadedProg::load(spi_cpha1_prog.program, &mut pio_ss).unwrap();
    report_api(0x05D1_0001);

    let clkdiv: f32 = 37.25;
    let mut passing = true;
    let mut cpol = false;
    pio_sm.pio.wo(rp_pio::SFR_IRQ0_INTE, pio_sm.sm_bitmask());
    pio_sm.pio.wo(rp_pio::SFR_IRQ1_INTE, (pio_sm.sm_bitmask()) << 4);
    loop {
        // pha = 1
        report_api(0x05D1_0002);
        pio_spi_init(
            &mut pio_sm,
            &prog_cpha0, // cpha set here
            8,
            clkdiv,
            cpol,
            PIN_SCK,
            PIN_MOSI,
            PIN_MISO,
        );
        report_api(0x05D1_0003);
        if spi_test_core(&mut pio_sm) == false {
            passing = false;
        };

        // pha = 0
        report_api(0x05D1_0004);
        pio_spi_init(
            &mut pio_sm,
            &prog_cpha1, // cpha set here
            8,
            clkdiv,
            cpol,
            PIN_SCK,
            PIN_MOSI,
            PIN_MISO,
        );
        report_api(0x05D1_0005);
        if spi_test_core(&mut pio_sm) == false {
            passing = false;
        };
        if cpol {
            break;
        }
        // switch to next cpol value for test
        cpol = true;
    }
    // cleanup external side effects for next test
    pio_sm.gpio_reset_overrides();
    pio_sm.pio.wo(rp_pio::SFR_IRQ0_INTE, 0);
    pio_sm.pio.wo(rp_pio::SFR_IRQ1_INTE, 0);
    pio_sm.pio.wo(rp_pio::SFR_SYNC_BYPASS, 0);

    if passing {
        report_api(0x05D1_600D);
    } else {
        report_api(0x05D1_DEAD);
    }
    assert!(passing);
    passing
}
