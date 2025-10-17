use utralib::*;
use xous_bio_bdma::*;

// pinmask 29 is not mapped out. This is tested by pushing the button to allow boot
const DABAO_PINMASK: u32 = 0b0001_1111_1000_1111_0111_1000_011_1110; // 0x1F8F783E;

pub fn dabao_selftest() {
    let mut bio_ss = BioSharedState::new();
    let iox = bao1x_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    iox.set_ports_from_pio_bitmask(DABAO_PINMASK);
    bio_ss.init();
    bio_ss.load_code(db_pin_test_code(), 0, BioCore::Core0);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0001);
    bio_ss.set_core_run_states([true, false, false, false]);

    let pin_ordering: [u32; 19] = [28, 27, 26, 25, 24, 23, 19, 18, 17, 16, 14, 13, 12, 11, 1, 2, 3, 4, 5];

    crate::println!(
        "Starting dabao mini self-test: sets each pin sequentially, in a counter-clockwise order."
    );
    crate::usb::flush();
    crate::delay(100);
    const TOTAL_ITERS: usize = 4;
    for i in 0..TOTAL_ITERS {
        crate::println!("Iter {}/{}", i + 1, TOTAL_ITERS);
        crate::usb::flush();
        for pin in pin_ordering {
            crate::print!("{} ", pin);
            crate::usb::flush();
            bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, 1u32 << pin);
            crate::delay(100);
        }
        crate::println!("");
    }
    crate::println!("Done; pin state reverted");
    crate::usb::flush();
    iox.set_ports_from_pio_bitmask(0x0);
}

#[rustfmt::skip]
bio_code!(db_pin_test_code, BM_PIN_TEST_START, BM_PIN_TEST_END,
    // set all pins as inputs
    "li a0, -1",
    "mv x26, a0", // mask
    "mv x25, a0", // inputs
    "li a1, 0x3F8F783E", // connected pin mask
    "mv x24, a1", // select pins as outputs
    // receive data from the FIFO, update it to the pins
"10:",
    "mv t0, x16", // wait for input
    "mv x21, t0", // pass to output pins
    "j 10b"
);
