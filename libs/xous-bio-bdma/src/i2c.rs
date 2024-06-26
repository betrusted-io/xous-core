use core::fmt::Error;

use crate::*;

pub const MAX_TX_LEN: usize = 126;
pub const MAX_RX_LEN: usize = 127;
const FIFO_HIGH_WATER_MARK: u32 = 7;
pub struct BioI2C<'a> {
    pin_sda: u8,
    pin_scl: u8,
    irq_no: u8,
    bio_ss: &'a mut BioSharedState,
}
impl<'a> BioI2C<'a> {
    /// This is unsafe because it will clear all of the memory and state in the BIO.
    /// If a cooperative machine is required a new routine needs to be coded.
    ///
    /// The routine requires the user to specify an IRQ channel so it knows where to poll,
    /// but it does not install an interrupt handler.
    ///
    /// An interrupt-driven version of the routine would need to enable interrupts from e.g
    /// IRQARRAY18
    pub unsafe fn new_exclusive(
        bio_ss: &'a mut BioSharedState,
        sda: u8,
        scl: u8,
        irq_no: u8,
        hs: bool,
    ) -> Option<Self> {
        if sda > 31 || scl > 31 {
            return None;
        }
        if irq_no > 3 {
            return None;
        }
        // stop all the machines, so that code can be loaded
        bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
        let code = i2c_driver();
        print!("I2C code length: {} bytes\r", code.len());
        bio_ss.load_code(code, 0, BioCore::Core0);

        if hs {
            // 1600kHz clock -> 400kHz toggle rate = 0x1F4_0000 @ 800MHz rate FCLK
            bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1F4_0000);
        } else {
            // 400kHz clock -> 100kHz toggle rate = 0x7D0_0000 @ 800MHz rate FCLK
            bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x7D0_0000);
        }

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

        bio_ss.bio.wo(utra::bio_bdma::SFR_IRQMASK_0, 1 << (28 + irq_no) as u32);

        Some(BioI2C { pin_sda: sda, pin_scl: scl, irq_no, bio_ss })
    }

    /// This will block until the full transaction completes. On Xous, `do_yield` can be
    /// specified which causes the process to yield its quantum during the wait loop so that
    /// other processes can make progress.
    ///
    /// `dev_addr` is the 7-bit version of the target address (i.e. shifted without the r/w bit)
    ///
    /// Note that this can easily be made multi-bank/multi-I2C interface by simply making the SCL/SDA
    /// pin mappings an argument to this function. Every transaction can use a different GPIO mapping,
    /// so we could potentially have up to 16 free-from I2C units controlled with a single code block.
    pub fn txrx_blocking(
        &mut self,
        dev_addr: u8,
        tx: &[u8],
        rx: Option<&mut [u8]>,
        do_yield: bool,
    ) -> Result<(), Error> {
        // 1 is read, 0 is write
        if tx.len() > 0 {
            let wr_byte = (dev_addr << 1 | 0x0) as u8;
            let i2c_cmd = (self.pin_sda as u32) << 27
                | (self.pin_scl as u32) << 22
                | (tx.len() as u32 + 1).min(MAX_TX_LEN as u32) << 8
                | 0 << 15
                | wr_byte as u32;
            // start the Tx
            self.bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, i2c_cmd);
            // write the bytes to send
            for &data in tx.iter() {
                while self.bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1)
                    > FIFO_HIGH_WATER_MARK
                {
                    if do_yield {
                        #[cfg(feature = "std")]
                        xous::yield_slice();
                    }
                }
                self.bio_ss.bio.wfo(utra::bio_bdma::SFR_TXF1_FDIN, data as u32);
            }
            // wait for done
            while self.bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & (1 << self.irq_no + 28) == 0 {
                if do_yield {
                    #[cfg(feature = "std")]
                    xous::yield_slice();
                }
            }
            let result_code = self.bio_ss.bio.r(utra::bio_bdma::SFR_RXF2);
            if result_code != (tx.len() as u32 + 1) {
                #[cfg(not(feature = "tests"))]
                return Err(Error);
            }
            #[cfg(feature = "tests")]
            {
                if result_code == (tx.len() as u32 + 1) {
                    print!("  Tx pass, Result code: {:x}\r", result_code);
                } else {
                    print!("  Tx fail, Result code: {:x}\r", result_code);
                }
            }
        }
        if let Some(rx) = rx {
            if rx.len() > 0 {
                let wr_byte = (dev_addr << 1 | 0x1) as u8;
                let i2c_cmd = (self.pin_sda as u32) << 27
                    | (self.pin_scl as u32) << 22
                    | 1 << 8
                    | (rx.len() as u32) << 15
                    | wr_byte as u32;
                // start the Rx
                self.bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, i2c_cmd);
                // read the data
                for data in rx.iter_mut() {
                    while self.bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {
                        if do_yield {
                            #[cfg(feature = "std")]
                            xous::yield_slice();
                        }
                    }
                    *data = self.bio_ss.bio.rf(utra::bio_bdma::SFR_RXF1_FDOUT) as u8;
                }
                // wait for the status code
                while self.bio_ss.bio.r(utra::bio_bdma::SFR_EVENT_STATUS) & (1 << self.irq_no + 28) == 0 {
                    if do_yield {
                        #[cfg(feature = "std")]
                        xous::yield_slice();
                    }
                }
                let result_code = self.bio_ss.bio.r(utra::bio_bdma::SFR_RXF2);
                // should be 1 NACK on rx
                if result_code != ((1 + rx.len() as u32 - 1) | 0x1_0000) {
                    #[cfg(not(feature = "tests"))]
                    return Err(Error);
                }
                #[cfg(feature = "tests")]
                {
                    if result_code == ((1 + rx.len() as u32 - 1) | 0x1_0000) {
                        print!("  Rx pass, Result code: {:x}\r", result_code);
                    } else {
                        print!("  Rx fail, Result code: {:x}\r", result_code);
                    }
                    print!("  Rx data: {:x?}\r", rx);
                }
            }
        }
        Ok(())
    }
}

// An I2C test that gives us coverage of GPIO direction control
// I2C is initiated by writing a 32-bit word to x16 (FIFO 0) that has the following format:
//    bit[0..1]   - r/w. R=1, W=0
//    bit[1..8]   - device address
//    bit[8..15]  - bytes to write (0-127); amount includes device addressing write
//    bit[15..22] - bytes to read (0-127)
//    bit[22..27] - I/O pin for SCL
//    bit[27..32] - I/O pin for SDA
//
// Outgoing data is written into x17; incoming data is read from x17 (FIFO 1)
//
// Host determines if I2C is done by IRQ & value appearing on x18 (FIFO 2) which contains ack/nack status
// Format is:
//    bit[0..9]   - # of words successfully written or (read - 1)
//    bit[16..25] - # of words NACK'd - if a read, the last word is always NACK so this should be 1
//
// Quantum is assumed to be 4x of final I2C clock rate
#[rustfmt::skip]
bio_code!(
    i2c_driver,
    I2C_DRIVER_START,
    I2C_DRIVER_END,
  "90:", // machine 0 code
    "mv x1, x16",        // x1 <- initiation command
    "andi x2, x1, 1",    // x2 <- r/w bit. write if 0.
    "andi x3, x1, 0xff", // x3 <- word to send: initially, device address with r/w bit in place
    "mv x9, x1",         // x9 <- writes to transfer
    "srli x9, x9, 8",
    "andi x9, x9, 0x7f",
    "mv x10, x1",        // x10 <- reads to transfer
    "srli x10, x10, 15",
    "andi x10, x10, 0x7f",
    "mv x15, x1",        // x15 <- SCL pin temp
    "srli x15, x15, 22",
    "andi x15, x15, 0x1f",
    "mv x14, x1",        // x14 <- SDA pin temp
    "srli x14, x14, 27",
    "andi x14, x14, 0x1f",
    "li x5, 1",          // setup GPIO masks
    "sll x5, x5, x15",   // x5 <- SCL bitmask
    "li x4, 1",
    "sll x4, x4, x14",   // x4 <- SDA bitmask
    // setup gpios
    "mv x20, x0",        // sync to clock stream
    "or x26, x4, x5",    // set GPIO mask
    "or x23, x4, x5",    // clear SCL/SDA pins to 0 for "output low"
    "or x25, x4, x5",    // SCL/SDA to inputs
    // setup done
    "mv x20, x0",        // sync to clock stream
    "li x7, 0",          // x7 <- ACK counter
    "li x8, 0",          // x8 <- NACK counter

    // main loop
  "26:",
    // START bit
    "mv x20, x0", // SCL HS
    "mv x24, x4",        // SDA <- 0 while SCL is high (start)
    "mv x20, x0", // SCL HS+1 -- extra time for repeated start hold, some devices require this
    "li x6, 8",          // x6 <- set loop counter = 8
    "mv x11, x0",        // x11 is read shift register; initialize with 0

  "23:",
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "beq x9, x0, 30f",  // go to 30f if read cycle
    // write path
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
    "addi x6, x6, -1",   // decrement bit counter
    "bne x6, x0, 23b",   // loop back if we haven't shifted all the bits

    // incoming ACK
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "mv x20, x0", // SCL L1
    "mv x25, x4",        // SDA is input
    "mv x20, x0", // SCL H0
    "mv x25, x5",        // SCL <- 1
    "mv x13, x21",       // read GPIO pins
    "and x13, x4, x13",  // mask just SDA
    "beq x13, x0, 24f",  // if ack, go to 24f
    "addi x8, x8, 1",    //   NACK += 1
    "j 25f",
  "24:",
    "addi x7, x7, 1",    //   ACK += 1
    "j 25f",

    // read path
  "30:",
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
    "addi x6, x6, -1",   // decrement bit counter
    "bne x6, x0, 23b",   // loop back if we haven't shifted all the bits
    "mv x17, x11",       // x17 <- read data

    // outgoing ACK or NACK
    "beq x10, x0, 28f",  // if last iteration, do NACK
    "mv x24, x4",        //    otherwise ACK by SDA as output
    "addi x7, x7, 1",    //   ACK += 1
    "j 29f",
  "28:",
    "mv x25, x4",        // SDA is input (SDA goes high; NACK)
    "addi x8, x8, 1",    //   NACK += 1
  "29:",
    "mv x20, x0", // SCL L0
    "mv x24, x5",        // SCL <- 0
    "mv x20, x0", // SCL L1
    "mv x20, x0", // SCL H0
    "mv x25, x5",        // SCL <- 1
    "j 25f",

    // loop prologue
  "25:",
    "mv x20, x0", // SCL H1
    "beq x9, x0, 60f",   // don't decrement past 0; go to read processing instead
    "addi x9, x9, -1",   // if not, decrement total writes to transfer
    "beq x9, x0, 60f",   // check if writes are exhausted, if so, go to read processing
    "mv x3, x17",        // x3 <- x17 fetch outgoing from FIFO. Transfer will halt here until byte is available.
    "j 26b",             // loop back to "DATA bits"
  "60:",
    "beq x10, x0, 50f",  // if x10 is already 0, go to STOP
    "addi x10, x10, -1", //    decrement x10 (bytes to read)
    "j 26b",             // loop back to "DATA bits"

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
    "j 90b"             // wait for next command
 );
