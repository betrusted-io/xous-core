use crate::*;

pub fn i2c_test() {
    print!("I2C tests\r");

    // clear prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = i2c_driver();
    print!("code length {}\r", code.len());
    bio_ss.load_code(code, 0);

    test_cfg.wo(utra::main::WDATA, crate::bio_tests::TEST_I2C_MASK);

    // configure & run the 0th machine
    // 400kHz clock -> 100kHz toggle rate
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x7D0_0000);

    // clear all events
    bio_ss.bio.wfo(utra::bio::SFR_EVENT_CLR_SFR_EVENT_CLR, 0xFFFF_FF);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio::SFR_FIFO_CLR, 0xF);
    // start core 0
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x111);

    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio::SFR_CONFIG, 0);

    // configure interrupts
    // T/RX2 is the bank of interest for triggering interrupts
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL4, 1); // corresponds to T/RXF2
    bio_ss.bio.wfo(utra::bio::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0b00_01_00_00); // level4 mask EQ
    bio_ss.bio.wfo(utra::bio::SFR_ETYPE_FIFO_EVENT_GT_MASK, 0b00_01_00_00); // level4 mask GT
    // IRQ 0 should issue the pulse
    bio_ss.bio.wo(utra::bio::SFR_IRQMASK_0, 1 << 2);

    // this is where the IRQ is wired to right now
    let irqarray18 = CSR::new(utra::irqarray18::HW_IRQARRAY18_BASE as *mut u32);

    // unadapted code below
    const PIN_SDA: u32 = 2;
    const PIN_SCL: u32 = 3;

    let mut rxbuf = [0u8];
    // expected pass or fails of reads, for automated regression testing
    // these are wired directly into the LiteX test harness inside the PioAdapter block
    let failing_address = 0x17 >> 1; // 0x17 is exactly what is inside the testbench code, shift right by one to disregard r/w bit
    let mut passing = true;
    for addr in 10..14 {
        print!("reading from {}\r", addr);

        let i2c_cmd = PIN_SDA << 27 | PIN_SCL << 22 | ((rxbuf.len() & 0xFF) as u32) << 8 | addr << 1 | 0x1;
        bio_ss.bio.wo(utra::bio::SFR_TXF0, i2c_cmd);

        let mut timeout = 0;
        while irqarray18.r(utra::irqarray18::EV_STATUS) & (1 << 2) == 0 {
            // wait
            timeout += 1;
            if (timeout % 1_000) == 0 {
                print!("  timeout: {}\r", timeout);
            }
        }
        // read the readback data
        for d in rxbuf.iter_mut() {
            *d = bio_ss.bio.r(utra::bio::SFR_RXF1) as u8;
        }
        // read the result code
        let result_code = bio_ss.bio.r(utra::bio::SFR_RXF2);
        print!("rbk {:x?}, result_code {:x}", rxbuf, result_code);

        if addr == failing_address {
            if result_code != 0x2_0000 {
                // expect two NACKs
                passing = false;
            }
        } else {
            if result_code != 0x2 {
                // expect 2 ACKs
                passing = false;
            }
        }
    }

    // turn off interrupts after the test, otherwise this interferes with later operations
    bio_ss.bio.wo(utra::bio::SFR_IRQMASK_0, 0);
    if passing {
        print!("===I2C tests PASS===\r");
    } else {
        print!("===I2C tests FAIL===\r");
    }
}

// An I2C test that gives us coverage of GPIO direction control
// I2C is initiated by writing a 32-bit word to x16 that has the following format:
//    bit[0..1]   - r/w. R=1, W=0
//    bit[1..8]   - device address
//    bit[8..17]  - bytes to read or write (0-256 is valid values; not bounds checked)
//    bit[17..22] - reserved
//    bit[22..27] - I/O pin for SCL
//    bit[27..32] - I/O pin for SDA
//
// Outgoing data is written into x17; incoming data is read from x17
//
// Host determines if I2C is done by IRQ & value appearing on x18 which contains ack/nack status
// Format is:
//    bit[0..9]   - # of words successfully written or read (up to 257: 1 address + 0-256 words)
//    bit[16..25] - # of words NACK'd
//
// Quantum is assumed to be 4x of final I2C clock rate
#[rustfmt::skip]
bio_code!(
    i2c_driver,
    I2C_DRIVER_START,
    I2C_DRIVER_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
  "90:", // machine 0 code
    "mv x1, x16",        // x1 <- initiation command
    "andi x2, x1, 1",    // x2 <- r/w bit. write if 0.
    "andi x3, x1, 0xff", // x3 <- word to send: initially, device address with r/w bit in place
    "mv x9, x1",         // x9 <- total bytes to transfer
    "srli x9, x9, 8",
    "andi x9, x9, 0x1ff",// no bounds check; max value should be 0x100 though
    "li x10, 1",         // x10 <- bytes to send (at least 1 for address cycle)
    "bne x2, x0, 27f",   // if read, don't add the byte count
    "add x10, x9, x10",  //   write: add bytes to send to total send count
  "27:", // reads leave x10 == 1
    "addi x9, x9, 1",    //   add 1 to x9, to account for address word
    "mv x15, x1",        // x15 <- SCL pin temp
    "srli x15, x15, 22",
    "andi x15, x15, 0x1f",
    "mv x16, x1",        // x16 <- SDA pin temp
    "srli x16, x16, 27",
    "andi x16, x16, 0x1f",
    "li x5, 1",          // setup GPIO masks
    "sll x5, x5, x15",   // x5 <- SCL bitmask
    "li x4, 1",
    "sll x4, x4, x16",   // x4 <- SDA bitmask
    "or x23, x4, x5",    // pre-set SCL/SDA to 0 for "output low"
    "or x25, x4, x5",    // set SCL, SDA to 1 (inputs)
    // setup done
    "li x7, 0",          // x7 <- ACK counter
    "li x8, 0",          // x8 <- NACK counter
    // START bit
    "mv x20, x0", // SCL HS
    "mv x24, x4",        // SDA <- 0 while SCL is high (start)
    // main loop
    // DATA bits
  "26:",
    "li x6, 8",          // x6 <- loop counter
    "mv x11, x0",        // x11 is read shift register; initialize with 0
  "23:",
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "beq x10, x0, 30f",  // go to 30f if read cycle
    // write code
    "mv x20, x0", // SCL L1
    "andi x14, x3, 0x80",// x14 (temp) <- extract MSB for sending
    "slli x3, x3, 1",    // shift data for next iteration
    "beq x14, x0, 20f",  // 20f: send 0
    "mv x25, x4",        // not taken SDA <- 1
    "j 21f",             // go past send 0
  "20:",
    "mv x24, x4",        // taken SDA <- 0
  "21:",
    "mv x20, x0", // SCL H0
    "mv x25, x5",        // SCL <- 1
    "mv x20, x0", // SCL H1
    "j 40f",             // jump past read code
  "30:",
    // read code
    "mv x25, x4",        // SDA is input
    "mv x20, x0", // SCL L1
    "mv x20, x0", // SCL H0
    "mv x25, x5",        // SCL <- 1
    "mv x20, x0", // SCL H1
    "mv x14, x21",       // x14 (temp) <- GPIO pins
    "and x14, x14, x4",  // mask just SDA
    "slli x11, x11, 1",  // x11 <<= x11
    "beq x14, x0, 40f",  // don't OR in a 1 if SDA is 0
    "ori x11, x11, 1",   //   x11 <- x11 | 1
  "40:",
    "addi x6, x6, -1",
    "bne x6, x0, 23b",   // loop back if we haven't shifted all the bits
    // ACK/NACK
    "mv x25, x4",        // SDA is input
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "mv x20, x0", // SCL L1
    "mv x25, x5",        // SCL <- 1
    "mv x20, x0", // SCL H0
    "mv x13, x21",       // read GPIO pins
    "and x13, x4, x13",  // mask just SDA
    "beq x13, x0, 24f",  // if ack, go to 24f
    "addi x8, x8, 1",    //   NACK += 1
    "j 25f",
  "24:",
    "addi x7, x7, 1",    //   ACK += 1
  "25:",
    "mv x20, x0", // SCL H1
    "addi x9, x9, -1",   // decrement total bytes to transfer
    "beq x10, x0, 29f",  // if x10 is already 0, don't decrement: commit read data instead
    "addi x10, x10, -1", //    decrement x10 (bytes to send)
    "beq x10, x0, 28f",  // if x10 is now 0, don't fetch new data to send
    "mv x3, x17",        // x3 <- x17 fetch outgoing from FIFO. Transfer will halt here until byte is available.
  "28:",
    "bne x9, x0, 26b",   // loop back to "DATA bits" if more bytes
    "j 50f",             //   fall through to STOP otherwise
  "29:",
    "mv x17, x11",       // x17 <- x11 store read data into x17 FIFO. Will halt if FIFO is full.
    "bne x9, x0, 26b",   // loop back to "DATA bits" if more bytes
    // STOP
  "50:",
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "mv x24, x4",        // SDA <- 0
    "mv x20, x0", // SCL L1
    "mv x20, x0", // SCL H1
    "mv x25, x5",        // SCL is input (SCL -> 1)
    "mv x20, x0", // SCL H2
    "mv x25, x4",        // SDA is input (SDA -> 1)
    // REPORT
    "slli x8, x8, 16",   // shift NACK count into place
    "add x18, x7, x8",   // x18 <- send ACK + NACK
    "j 90b",             // wait for next command
    // gutter unused machines
  "91:",
    "j 91b",
  "92:",
    "j 92b",
  "93:",
    "j 93b"
 );
