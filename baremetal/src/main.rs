#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

// contains runtime setup
mod asm;

mod platform;
use platform::*;
use utralib::*;
use xous_bio_bdma::*;

/// Entrypoint
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry() -> ! {
    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    const SYSTEM_CLOCK_FREQUENCY: u32 = 40_000_000;
    const SYSTEM_TICK_INTERVAL_MS: u32 = 1;
    let ms = SYSTEM_TICK_INTERVAL_MS;
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    // load its values
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
    // enable the timer
    timer.wfo(utra::timer0::EN_EN, 0b1);

    // select a BIO test to run
    fifo_basic();
    // hello_world();

    let bio_ss = BioSharedState::new();
    // The green LEDs flash whenever the FPGA is configured with the Arty BIO design.
    // The RGB LEDs flash when the CPU is running this code.
    let mut count = 0;
    let mut rgb = CSR::new(utra::rgb::HW_RGB_BASE as *mut u32);
    // provide some feedback on the run state of the BIO by peeking at the program counter
    // value, and provide feedback on the CPU operation by flashing the RGB LEDs.
    loop {
        crate::println!(
            "pc: {:04x} {:04x} {:04x} {:04x}",
            bio_ss.bio.r(utra::bio_bdma::SFR_DBG0),
            bio_ss.bio.r(utra::bio_bdma::SFR_DBG1),
            bio_ss.bio.r(utra::bio_bdma::SFR_DBG2),
            bio_ss.bio.r(utra::bio_bdma::SFR_DBG3)
        );
        rgb.wfo(utra::rgb::OUT_OUT, count);
        delay(500);
        count += 1;
    }
}

// Install a panic handler when not running tests.
#[cfg(all(not(test)))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

/// Delay function that delays a given number of milliseconds.
pub fn delay(ms: usize) {
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}

// this test requires manual inspection of the outputs
// the GPIO pins should toggle with 0x11, 0x12, 0x13...
// at the specified quantum rate of the machine.
pub fn hello_world() {
    crate::println!("hello world test");
    let mut bio_ss = BioSharedState::new();
    crate::println!("cfginfo: {:x}", bio_ss.bio.r(utra::bio_bdma::SFR_CFGINFO));
    let simple_test_code = hello_world_code();
    // copy code to reset vector for 0th machine
    bio_ss.load_code(simple_test_code, 0, BioCore::Core0);
    // make sure the machine is stopped
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    for &d in simple_test_code {
        crate::print!("{:x}", d);
    }
    crate::println!("");
    for &d in bio_ss.imem_slice[0][..16].iter() {
        crate::println!("{:x}", d);
    }

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x20_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x111);
    crate::println!("===hello world end===");
}

#[rustfmt::skip]
bio_code!(hello_world_code, HELLO_START, HELLO_END,
    "li    t0, 0xFFFFFFFF",  // set all pins to outputs
    "mv    x24, t0",
    "li    a0, 1",
  "20:",
    "mv    x21, a0",
    "slli  a0, a0, 1",
    "bne   a0, zero, 21f",
    "li    a0, 1",           // if a0 is 0, reset its value to 1
  "21:",
    "mv   x20, zero",
    "j 20b",
    "nop"
);

// this test requires manual checking of gpio outputs
// GPIO pins should have the form 0x100n800m
// where n = 2*m. The output is not meant to be fully in sync,
// it will be "ragged" as the output snapping is not turned on.
// so 0x10008000, 0x10048002, 0x10088004, etc...
// but with a glitch before major transitions. The output could
// be sync'd locked, but we leave it off for this test so we have
// a demo of how things look when it's off.
pub fn fifo_basic() -> usize {
    crate::println!("FIFO basic");
    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    bio_ss.load_code(fifo_basic0_code(), 0, BioCore::Core0);
    bio_ss.load_code(fifo_basic1_code(), 0, BioCore::Core1);
    bio_ss.load_code(fifo_basic2_code(), 0, BioCore::Core2);
    bio_ss.load_code(fifo_basic3_code(), 0, BioCore::Core3);

    // The code readback is broken on the Arty BIO target due to a pipeline stage
    // in the code readback path that causes the previous read's data to show up
    // on the current read access. On the NTO-BIO (full chip version), the BIO runs
    // at a much higher speed than the bus framework and thus the data is returned
    // on time for the read. However in the FPGA for simplicity the BIO is geared
    // at 2:1 BIO speed to CPU core speed, and the bus fabric runs at a single speed
    // with no CDCs and also a fully OSS AXI to AHB bridge that I think could also
    // be contributing to this bug.

    /*
    // expect no error
    match bio_ss.verify_code(&fifo_basic0_code(), 0, BioCore::Core0) {
        Err(BioError::CodeCheck(at)) => {
            print!("Core 0 rbk fail at {}\r", at);
            return 0;
        }
        _ => (),
    }
    match bio_ss.verify_code(&fifo_basic1_code(), 0, BioCore::Core1) {
        Err(BioError::CodeCheck(at)) => {
            print!("Core 1 rbk fail at {}\r", at);
            return 0;
        }
        _ => (),
    }
    match bio_ss.verify_code(&fifo_basic2_code(), 0, BioCore::Core2) {
        Err(BioError::CodeCheck(at)) => {
            print!("Core 2 rbk fail at {}\r", at);
            return 0;
        }
        _ => (),
    }
    match bio_ss.verify_code(&fifo_basic3_code(), 0, BioCore::Core3) {
        Err(BioError::CodeCheck(at)) => {
            print!("Core 3 rbk fail at {}\r", at);
            return 0;
        }
        _ => (),
    }

    // expect error
    if bio_ss.verify_code(&fifo_basic1_code(), 0, BioCore::Core0).is_ok() {
        print!("FAIL: Core 0 passed check with false code\r");
        return 0;
    }
    */
    // configure & run the 0th machine
    // / 16. clock
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x23_BE00);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x23_BE00);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x33_1200);
    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x33_1200);
    // don't snap GPIO outputs
    bio_ss.bio.wo(utra::bio_bdma::SFR_CONFIG, 0);
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0xfff);
    crate::println!("===FIFO basic PASS===");
    1
}
#[rustfmt::skip]
bio_code!(fifo_basic0_code, FIFO_BASIC0_START, FIFO_BASIC0_END,
    "li    t0, 0xFFFFFFFF",  // set all pins to outputs
    "mv    x24, t0",
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
    "j 11b"
);
#[rustfmt::skip]
bio_code!(fifo_basic1_code, FIFO_BASIC1_START, FIFO_BASIC1_END,
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
    "j 21b"
);
#[rustfmt::skip]
bio_code!(fifo_basic2_code, FIFO_BASIC2_START, FIFO_BASIC2_END,
    // mach 2 code
    "92:",
    "addi x18, x18, 2", // increment the value in fifo by 2
    "mv x20, zero",
    "j 92b"
);
#[rustfmt::skip]
bio_code!(fifo_basic3_code, FIFO_BASIC3_START, FIFO_BASIC3_END,
    // mach 3 code
    "93:",
    "li x2, 0x40000",
    "23:",
    "add x19, x19, x2", // increment the value in fifo by 0x4_0000
    "mv x20, zero",
    "j 23b",
    "nop"
);
