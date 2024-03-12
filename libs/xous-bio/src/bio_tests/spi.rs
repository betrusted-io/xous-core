use utralib::utra::bio::{SFR_EVENT_STATUS_SFR_EVENT_STATUS, SFR_FLEVEL_PCLK_REGFIFO_LEVEL1};

use super::report_api;
use crate::*;

pub fn spi_test() {
    report_api(0x1310_5000);
    report_api(0x51C0_0000);

    // clear prior test config state
    let mut test_cfg = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    test_cfg.wo(utra::main::WDATA, 0);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = spi_driver();
    report_api(code.len() as u32);
    bio_ss.load_code(code, 0);

    // configure fifo trigger levels
    bio_ss.bio.wfo(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL0, 4);
    bio_ss.bio.rmwf(utra::bio::SFR_ELEVEL_FIFO_EVENT_LEVEL2, 4);
    // configure the polarities
    bio_ss.bio.wfo(utra::bio::SFR_ETYPE_FIFO_EVENT_EQ_MASK, 0b00_00_01_01);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_LT_MASK, 0b00_00_00_01);
    bio_ss.bio.rmwf(utra::bio::SFR_ETYPE_FIFO_EVENT_GT_MASK, 0b00_00_01_00);
    // reset all the fifos
    bio_ss.bio.wo(utra::bio::SFR_FIFO_CLR, 0xF);

    // tx rate set
    bio_ss.bio.wo(utra::bio::SFR_QDIV2, 0x10_0000);
    // this actually shouldn't matter, but set it to be "fast" for testing
    bio_ss.bio.wo(utra::bio::SFR_QDIV3, 0x1_0000);
    // snap GPIO outputs
    bio_ss.bio.wo(
        utra::bio::SFR_CONFIG,
        bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, 1)
        | bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, 2)
    );

    // bypass sync on all but clock
    bio_ss.bio.wo(utra::bio::SFR_SYNC_BYPASS, 0x500);
    // use extclock on channel 3, tied to bit 9
    bio_ss.bio.wo(
        utra::bio::SFR_EXTCLOCK,
        bio_ss.bio.ms(utra::bio::SFR_EXTCLOCK_USE_EXTCLK, 0b1000)
        | bio_ss.bio.ms(utra::bio::SFR_EXTCLOCK_EXTCLK_GPIO_3, 9)
    );

    // start cores 2 & 3
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xCCC);

    let mut i = 0;
    let mut j = 0;
    const TEST_LEN: usize = 32;
    let mut retvals = [0u32; TEST_LEN];
    loop {
        // fill the FIFO
        while (((bio_ss.bio.rf(SFR_EVENT_STATUS_SFR_EVENT_STATUS) >> 24) & 0x1) != 0) && (i < TEST_LEN) {
            // report_api(i as u32 | 0xAA00);
            bio_ss.bio.wo(utra::bio::SFR_TXF0, i as u32 | 0xAA00);
            i += 1;
        }
        if i >= TEST_LEN {
            break;
        }
        // if FIFO is past the full water mark, drain one entry
        if ((bio_ss.bio.rf(SFR_EVENT_STATUS_SFR_EVENT_STATUS) >> 24) & 0x4) != 0 {
            retvals[j] = bio_ss.bio.r(utra::bio::SFR_RXF1);
            j += 1;
        }
    }
    while j < TEST_LEN {
        while bio_ss.bio.rf(SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {} // wait until there's something there
        retvals[j] = bio_ss.bio.r(utra::bio::SFR_RXF1);
        j += 1;
        report_api(j as u32);
    }
    for (index, &val) in retvals.iter().enumerate() {
        report_api(val);
        // the XOR to 0xAAAA is just a mask we threw in to test immediate loads
        assert!(val == ((0xAA00 | index as u32) ^ 0xAAAA));
    }

    report_api(0x51C0_600D);
}

// This is written to also test some additional features:
//   - loading word out of RAM (the random mask at the top)
//   - machine ID based dispatch
//   - how fast the SPI could run, hence the unrolled loop
//   - Receive clock triggering a code event (quanta register x20
//     is remapped to a GPIO input instead of the clock divider)
//   - Branch on quanta read (saves one clock cycle for a tight loop;
//     not strictly necessary but we use the idiom here to cap Tx loop)
// This runs at about a 25MHz SPI clock rate, assuming an 800MHz
// core clock for the complex. The Rx loop is potentially much faster
// than the Tx loop.
#[rustfmt::skip]
bio_code!(
    spi_driver,
    SPI_DRIVER_START,
    SPI_DRIVER_END,
    "j 90f",
    "nop",
    "j 90f",
    "nop",
    "j 90f",
    "nop",
    "j 90f",
    "nop",
    "retmask:",
    ".word 0x0000AAAA",    // just a random mask for testing immediate loads
    "90:",
    // dispatch based on machine ID
    "srli x1, x31, 30",
    "li  x2, 0",
    "beq x1, x2, 80f",
    "li  x2, 1",
    "beq x1, x2, 81f",
    "li  x2, 2",
    "beq x1, x2, 82f",
    "li  x2, 3",
    "beq x1, x2, 83f",
    "80:", // machine 0
    "j 80b",
    "81:", // machine 1
    "j 81b",
    "82:", // machine 2 - tx on bit 8, clock on bit 9, chip select on bit 10
    "li  x1, 0x700",       // setup output mask bits
    "mv  x26, x1",         // mask
    "mv  x24, x1",         // direction (1 = output)
    "li  x2, 0x100",       // bitmask for output data
    "not x3, x2",
    "li  x4, 0x200",       // bitmask for clock
    "not x5, x4",
    "li  x6, 0x400",       // bitmask for chip select
    "not x7, x6",
    "mv  x21, x6",         // setup GPIO with CS high, clock low, data low
    "mv  x20, x0",         // halt to quantum, make sure this takes effect

    "20:",  // main loop
    "mv  x15, x16",        // load data to transmit from fifo 0 - halts until data is available
    "mv  x23, x7",         // drop CS
    "slli x15, x15, 8",    // shift so the LSB is at bit 8
    "mv  x20, x0",         // wait quantum for CS hold time

    // bit 0
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x20, x0",         // wait quantum for data setup

    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 1
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 2
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 3
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 4
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 5
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 6
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 7
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 8
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 9
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 10
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 11
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 12
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 13
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 14
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    // bit 15
    "and x22, x15, x2",    // set data bit, if it's 1
    "or  x23, x15, x3",    // clear data bit, if it's 0
    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // wait quantum for data setup
    "mv  x22, x4",         // clock rise
    "srli x15, x15, 1",
    "mv  x20, x0",         // wait quantum for data hold

    "mv  x23, x5",         // clock fall
    "mv  x20, x0",         // CS setup time
    "mv  x22, x6",         // raise CS
    "mv  x20, x0",         // meet CS min-high time (if necessary)
    "beqz x20, 20b",       // wait quantum & loop back -- x20 reads back as 0

    "83:", // machine 3 - rx on bit 8, clock on bit 9, chip select on bit 10
    "li  x1, 0x700",       // setup mask bits
    "mv  x26, x1",         // mask
    "not x1, x1",
    // "mv  x25, x1",         // direction (0 = input) -- don't execute this, because we're looping back on the output pins
    "li  x2, 0x100",       // bitmask for input data
    "not x3, x2",
    "li  x4, 0x200",       // bitmask for clock
    "not x5, x4",
    "li  x6, 0x400",       // bitmask for chip select
    "not x7, x6",
    "lw  x13, retmask",    // test loading of words in program text

    "30:", // main loop
    "mv x14, zero",        // zero out the result register

    // wait until CS falls
    "31:",
    "and x8, x6, x21",
    "bnez x8, 31b",

    // bit 0
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 1
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 2
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 3
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 4
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 5
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 6
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 7
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 8
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 9
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 10
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 11
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 12
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 13
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 14
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    // bit 15
    "srli x14, x14, 1",    // shift bit into place
    // wait until clock rises
    "mv x20, zero",        // halts until rising edge on bit 9 (configured by host)
    "and x9, x2, x21",     // mask the bit
    "slli x9, x9, 7",      // move to MSB
    "or x14, x9, x14",     // OR into the result

    "xor x14, x13, x14",   // apply the test mask
    "mv x17, x14",         // move result into output FIFO
    "j 30b"                // loop to top
);
