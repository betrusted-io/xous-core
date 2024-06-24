use crate::*;

const PIN_SDA: u32 = 2;
const PIN_SCL: u32 = 3;

pub fn i2c_test() {
    print!("I2C tests\r");

    // clear prior test config state
    let mut test_cfg = CSR::new(utra::csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(utra::csrtest::WTEST, 0);
    test_cfg.wo(utra::csrtest::WTEST, crate::bio_tests::TEST_I2C_MASK);

    // setup the machine & test
    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    let code = crate::i2c::i2c_driver();
    print!("code length {}\r", code.len());
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    // 400kHz clock -> 100kHz toggle rate = 0x7D0_0000 @ 800MHz rate FCLK
    // 1600kHz clock -> 400kHz toggle rate = 0x1F4_0000 @ 800MHz rate FCLK
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1F4_0000);

    // clear all events
    bio_ss.bio.wfo(utra::bio_bdma::SFR_EVENT_CLR_SFR_EVENT_CLR, 0xFFFF_FF);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio_bdma::SFR_FIFO_CLR, 0xF);
    // start core 0
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x111);

    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio_bdma::SFR_CONFIG, 0);

    // configure interrupts
    // T/RX2 is the bank of interest for triggering interrupts
    bio_ss.bio.wfo(utra::bio_bdma::SFR_ELEVEL_FIFO_EVENT_LEVEL4, 1); // corresponds to T/RXF2
    bio_ss.bio.wfo(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0b00_01_00_00); // level4 mask EQ
    bio_ss.bio.rmwf(utra::bio_bdma::SFR_ETYPE_FIFO_EVENT_GT_MASK, 0b00_01_00_00); // level4 mask GT
    // IRQ 0 should issue the pulse
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_0, 1 << 28);

    // this is where the IRQ is wired to right now
    let irqarray18 = CSR::new(utra::irqarray18::HW_IRQARRAY18_BASE as *mut u32);

    let mut rxbuf = [0u8];
    // expected pass or fails of reads, for automated regression testing
    // these are wired directly into the LiteX test harness inside the PioAdapter block
    let failing_address = 0x17 >> 1; // 0x17 is exactly what is inside the testbench code, shift right by one to disregard r/w bit
    let mut passing = true;
    for addr in 10..14 {
        print!("reading from {}\r", addr);

        // 1 is read, 0 is write
        let wr_byte = (addr << 1 | 0x1) as u8;
        let i2c_cmd =
            PIN_SDA << 27 | PIN_SCL << 22 | 1 << 8 | ((rxbuf.len() & 0x7F) as u32) << 15 | wr_byte as u32;
        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, i2c_cmd);

        let mut timeout = 0;
        while irqarray18.r(utra::irqarray18::EV_STATUS) & 1 == 0 {
            // wait
            timeout += 1;
            if (timeout % 1_000) == 0 {
                print!("  timeout: {}\r", timeout);
            }
        }
        // read the readback data
        for d in rxbuf.iter_mut() {
            *d = bio_ss.bio.r(utra::bio_bdma::SFR_RXF1) as u8;
        }
        // read the result code
        let result_code = bio_ss.bio.r(utra::bio_bdma::SFR_RXF2);
        if addr == failing_address {
            print!("rbk {:x?}, result_code {:x} (intentional fail)\r", rxbuf, result_code);
            if result_code != 0x2_0000 {
                // expect two NACKs
                passing = false;
            }
        } else {
            print!("rbk {:x?}, result_code {:x}\r", rxbuf, result_code);
            if result_code != 0x1_0001 {
                assert!(rxbuf[0] == wr_byte);
                // expect 1 ACK, 1 NACK (last read is always a NACK)
                passing = false;
            }
        }
    }

    // turn off interrupts after the test, otherwise this interferes with later operations
    bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_0, 0);
    if passing {
        print!("===I2C tests PASS===\r");
    } else {
        print!("===I2C tests FAIL===\r");
    }
}

/// This just generates some more complex I2C waveforms; we don't have a full I2C hardware
/// unit in the upper level test bench, so we use manual waveform checking.
pub fn complex_i2c_test() {
    print!("Complex I2C transactions\r");
    let mut bio_ss = BioSharedState::new();
    let mut i2c =
        unsafe { crate::i2c::BioI2C::new_exclusive(&mut bio_ss, PIN_SDA as u8, PIN_SCL as u8, 0, true) }
            .unwrap();
    // simulate a write-only transaction to device at 0x62, reg addr 0x10, data 0xaa, 0x55
    let test_addr = 0x62u8;
    let test_tx = [0x10, 0xAA, 0x55];
    print!("Write addr {:x}, data {:x?}", test_addr, test_tx);
    match i2c.txrx_blocking(test_addr, &test_tx, None, false) {
        Ok(_) => print!("Write test waveform generated (check in wave viewer)\r"),
        Err(_) => print!("write only test FAIL\r"),
    }
    // simulate a read transaction to device at 0x34, reg addr 0xC0, with two data
    let test_addr = 0x34u8;
    let test_tx = [0xc0];
    let mut test_rx = [0u8; 2];
    print!("Read addr {:x}, tx data {:x?} rx data {:x?}", test_addr, test_tx, test_rx);
    match i2c.txrx_blocking(test_addr, &test_tx, Some(&mut test_rx), false) {
        Ok(_) => print!("Read test waveform generated (check in wave viewer)\r"),
        Err(_) => print!("read test FAIL\r"),
    }
    print!("===Exit complex I2C transactions===\r");
}
