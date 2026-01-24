/// Notional sketch code that can take a QSPI data stream and turn it into 8080 panel data
///
/// The goal is to run a 320x320 panel at 16bpp and 60fps
///
/// The architecture is as follows:
///
/// - BIO core0 is the core Rx loop from SPI. There is some placeholder code for the 1-bit SPI stuff
///   that happens before QSPI mode, and it's assumed we don't have to understand it or do anything
///   with it. Once we hit QSPI mode, the loop waits for a value to come in on FIFO1 (x17) and every
///   time a value arrives there it shifts the incoming 4-bit data into an 8-bit packet, which it shoves
///   into FIFO0 (x16). It also checks if x17 is non-zero; if it is not 0, it exits the loop so you can
///   go back to the 1-bit SPI mode at the start of next frame
/// - BIO core1 generates the synchronization for cores 0 and 3 from the clock stream. It's configured
///   to trigger off of the rising clock edge on the incoming SPI bus. Every clock edge, it puts a zero
///   into x17 (FIFO1), and every two edges, it puts a zero into x18 (FIFO2). These zero-values synchronize
///   the Tx and Rx sides.
/// - BIO core2 interprets the CS line. It fully asynchronously just watches the CS line. It waits for
///   the line to go high, then low, and then once it goes high again, it sticks a *non-zero* value into
///   x17/x18. This takes advantage of the fact that you can multiple writes to every FIFO. So during
///   data transmission, the clock pules causes 0's to be shoved into x17/x18, and on the rising edge
///   of CS, a non-zero value is put into x17/x18, allowing for loop termination.
/// - BIO core3 transmits the 8080 data. There is a placeholder where data is expected to arrive
///   on x18 (FIFO2 - maybe we could use a different fifo for this?) from the *host*. This initial
///   data is currently ignored but notionally this could be commands destined for the panel to set
///   up its registers. Once this phase is done, it enters a loop where first it automatically sends
///   the "write burst" command, and then goes into data mode. In data mode it grabs data from FIFO0 (x16)
///   and blasts it to the 8080, toggling the write pulse every time, and then checking for the termination
///   condition on x18. Once it terminates, it goes back to the top of the loop where depending on the
///   host considerations we could code it to automatically arm for another frame, or we could wait
///   until the host initiates another transfer.
///
///   This is estimated to meet timing (but just barely) at 700MHz BIO speeds. If we're missing timing by a hair,
///   we could set the clock to 800MHz on the BIO core, as the core should overclock pretty well.

#[rustfmt::skip]
bio_code!(qspi_rx, QSPI_RX_START, QSPI_RX_END,
   // core is configured to clock off of rising edge on SPI clk
   "li x1, 0xF",     // this loads the GPIO mask
"1:",
   "stuff",          // do stuff that handles the single-bit SPI stuff. Assuming we can ignore the contents
"10:",
   "mv x8, x17",     // x17 is a cleaned-up version of CS coming from a BIO that does that exclusively
   "bne x8, x0, 20f", // exit the loop if CS != 0
   "mv x2, x21",     // read the GPIO
   "and x3, x2, x1", // mask out unused bits and put in x3
   "slli x3, x3, 4", // shift into place

   "mv x8, x17",     // x17 is a cleaned-up version of CS coming from a BIO that does that exclusively
   "bne x8, x0, 20f", // exit the loop if CS != 0
   "mv x2, x21",     // read the GPIO
   "and x4, x2, x1", // mask out unused bits
   "or x3, x3, x4",  // combine bits
   "mv x16, x3",     // send byte to FIFO
   "j 10b"           // loop
   // 9 arith + 1 loop = 9 * 4 + 1 * 5 = 34 MHz rate (17 MHz per two edges)
   // max time between edges = 5 arith + 1 loop = 5 * 4 + 1 * 5 = 28 MHz max edge rate
"20:",
    "cleanup"       // cleanup code
);

/// This core generates the reads off of clock edge
#[rustfmt::skip]
bio_code!(qspi_clocker, QSPI_CLK_START, QSPI_CLK_END,
   // core is configured to clock off of rising edge on SPI clk
"10:",
   "mv x20, x0",     // wait for clock edge
   "mv x17, x0",     // send a 0 into the spi CS fifo to initiate read
   "mv x20, x0",     // wait for clock edge
   "mv x17, x0",     // send a 0 into the spi CS fifo to initiate read
   "mv x18, x0",     // send a 0 into 8080 CS fifo to continue writes
   "j 10b"
);

/// This core generates the chip select termination pulse
#[rustfmt::skip]
bio_code!(qspi_cs, QSPI_CS_START, QSPI_CS_END,
   "li x1, 0x10",    // notional chip select on bit 4
"10:",
   "mv x2, x21",     // read GPIO
   "and x3, x2, x1", // mask out the CS line
   "bne x3, x0, 20f", // wait for high
   "j 10b",
"20:",               // initial high found
   "mv x2, x21",     // read GPIO
   "and x3, x2, x1", // mask out the CS line
   "beq x3, x0, 30f", // wait for low
   "j 20b",
"30:",               // now the CS is low, wait for it to go high
   "mv x2, x21",     // read GPIO
   "and x3, x2, x1", // mask out the CS line
   "bne x3, x0, 30f", // wait for high
   "mv x17, x1",     // put a non-zero value into x17, which terminates the receive loop
   "mv x18, x1",     // put a non-zero value into x18, which terminates the transmit loop
   "j 20b"           // return to searching for CS low
);

#[rustfmt::skip]
bio_code!(i8080_tx, I8080_TX_START, I8080_RX_END,
   // core runs full speed whenever data comes into the FIFO
   "li x1, 8",        // data shift into place for GPIO
   "li x2, 0x0FF0",   // data pins mask, just made up for now
   "li x3, 0x1000",   // CSX set
   "li x4, 0xFFFFEFFF", // CSX clear
   "li x5, 0x2000",   // DCX set
   "li x6, 0xFFFFDFFF", // DCX clear
   "li x7, 0x4000",   // RD set
   "li x8, 0x8000",   // WR set
   "li x9, 0xFFFF7FFF", // WR clear
   "li x10 0xFFF0",   // all GPIO pins mask

   "mv x0, x18",      // block on FIFO2 data - host sends this to setup 8080 tx loop
   // later on x18 will have data automatically put in it by the CS interpreter

   // setup control signals
   "mv x22, x3,"      // set CSX
   "mv x23, x6,"      // clear DCX
   "mv x22, x7,"      // set RD
   "mv x23, x9,"      // clear WR
"10:",
   "mv x23, x4,"      // clear CSX
   "li x12, 0x2C",    // 8080 page write command
   "slli x12, x12, x1", // shift into place
   "mv x21, x12",     // set 8080 page write bits
   "mv x22, x8",      // ** WR rising edge **
   "mv x0, x0, x0",   // NOP for wr high time requirement
   "mv x23, x9,",     // WR edge drop
   "mv x22, x5",      // set DCX
"20:",   // main transmit loop
   "mv x15, x16",     // blocks until a byte arrives in the FIFO
   "sll x15, x15, x1", // shift data into place
   "mv x26, x2",      // set GPIO pin set mask
   "mv x21, x15",     // change just the data pins
   "mv x26, x10",     // allow control pins to change
   "mv x22, x8",      // ** WR rising edge **
   "mv x13, x18",     // check termination condition while allowing WR hold time
   "mv x23, x9,",     // WR edge drop
   "beq x13, x0, 20b", // go back if the termination condition is 0
   // 9 inst + 1 br = 9 * 4 + 1 *5 = 41 cycles = 58ns total time, 17MHz tx rate

   "loop cleanup",     // do other 8080 commands to cleanup the panel and setup for next initiation
);
