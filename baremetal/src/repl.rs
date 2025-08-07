use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[allow(unused_imports)]
use utralib::*;
#[cfg(any(feature = "artybio", feature = "nto-bio"))]
use xous_bio_bdma::*;

pub struct Repl {
    cmdline: String,
    do_cmd: bool,
}

const COLUMNS: usize = 4;
impl Repl {
    pub fn new() -> Self { Self { cmdline: String::new(), do_cmd: false } }

    pub fn rx_char(&mut self, c: u8) {
        if c == b'\r' {
            crate::println!("");
            // carriage return
            self.do_cmd = true;
        } else if c == b'\x08' {
            // backspace
            crate::print!("\u{0008}");
            if self.cmdline.len() != 0 {
                self.cmdline.pop();
            }
        } else {
            // everything else
            match char::from_u32(c as u32) {
                Some(c) => {
                    crate::print!("{}", c);
                    self.cmdline.push(c);
                }
                None => {
                    crate::println!("Warning: bad char received, ignoring")
                }
            }
        }
    }

    pub fn process(&mut self) {
        if !self.do_cmd {
            return;
        }
        // crate::println!("got {}", self.cmdline);

        let mut parts = self.cmdline.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
        match cmd.as_str() {
            #[cfg(not(feature = "cramium-soc"))]
            "mon" => {
                #[cfg(feature = "artybio")]
                let bio_ss = BioSharedState::new();
                let mut rgb = CSR::new(utra::rgb::HW_RGB_BASE as *mut u32);
                let mut count = 0;
                let mut quit = false;
                const TICKS_PER_PRINT: usize = 5;
                const TICK_MS: usize = 100;
                while !quit {
                    // Hacky logic to create a 500ms interval on prints, but improve
                    // keyboard hit latency.
                    if count % TICKS_PER_PRINT == 0 {
                        #[cfg(feature = "artybio")]
                        crate::println!(
                            "pc: {:04x} {:04x} {:04x} {:04x}",
                            bio_ss.bio.r(utra::bio_bdma::SFR_DBG0),
                            bio_ss.bio.r(utra::bio_bdma::SFR_DBG1),
                            bio_ss.bio.r(utra::bio_bdma::SFR_DBG2),
                            bio_ss.bio.r(utra::bio_bdma::SFR_DBG3)
                        );
                        rgb.wfo(utra::rgb::OUT_OUT, (count / TICKS_PER_PRINT) as u32);
                    }
                    crate::platform::delay(TICK_MS);
                    count += 1;

                    // just check and see if the keyboard was hit
                    critical_section::with(|cs| {
                        let queue = crate::UART_RX.borrow(cs).borrow_mut();
                        if queue.len() > 0 {
                            quit = true;
                        }
                    });
                }
            }
            "peek" => {
                if args.len() == 1 || args.len() == 2 {
                    if let Ok(addr) = usize::from_str_radix(&args[0], 16) {
                        let count = if args.len() == 2 {
                            if let Ok(count) = u32::from_str_radix(&args[1], 10) { count } else { 1 }
                        } else {
                            1
                        };
                        // safety: it's not safe to do this, the user peeks at their own risk
                        let peek = unsafe { core::slice::from_raw_parts(addr as *const u32, count as usize) };
                        for (i, &d) in peek.iter().enumerate() {
                            if (i % COLUMNS) == 0 {
                                crate::print!("\n\r{:08x}: ", addr + i * size_of::<u32>());
                            }
                            crate::print!("{:08x} ", d);
                        }
                        crate::println!("");
                    } else {
                        crate::println!("Peek address is in hex");
                    }
                } else {
                    crate::println!("Help: peek <addr> [count], addr is in hex, count in decimal");
                }
            }
            "poke" => {
                if args.len() == 2 || args.len() == 3 {
                    if let Ok(addr) = u32::from_str_radix(&args[0], 16) {
                        if let Ok(value) = u32::from_str_radix(&args[1], 16) {
                            let count = if args.len() == 3 {
                                if let Ok(count) = u32::from_str_radix(&args[2], 10) { count } else { 1 }
                            } else {
                                1
                            };
                            // safety: it's not safe to do this, the user pokes at their own risk
                            let poke =
                                unsafe { core::slice::from_raw_parts_mut(addr as *mut u32, count as usize) };
                            for d in poke.iter_mut() {
                                *d = value;
                            }
                            crate::println!("Poked {:x} into {:x}, {} times", value, addr, count);
                        } else {
                            crate::println!("Poke value is in hex");
                        }
                    } else {
                        crate::println!("Poke address is in hex");
                    }
                } else {
                    crate::println!(
                        "Help: poke <addr> <value> [count], addr/value is in hex, count in decimal"
                    );
                }
            }
            #[cfg(feature = "cramium-soc")]
            "rram" => {
                if args.len() == 2 || args.len() == 3 {
                    if let Ok(addr) = usize::from_str_radix(&args[0], 16) {
                        if addr < utralib::HW_RERAM_MEM_LEN {
                            if let Ok(value) = u32::from_str_radix(&args[1], 16) {
                                let count = if args.len() == 3 {
                                    if let Ok(count) = u32::from_str_radix(&args[2], 10) { count } else { 1 }
                                } else {
                                    1
                                }
                                .min(utralib::HW_RRC_MEM_LEN as u32); // limit count to length of RRAM
                                let mut poke = Vec::new();
                                for _ in 0..count {
                                    poke.push(value);
                                }
                                // safety: this is safe because all elements are valid between the two
                                // representations, there are no alignment
                                // issues downcasting to a u8, and the underlying representation
                                // is in fact the data we're hoping to access.
                                let poke_inner = unsafe {
                                    core::slice::from_raw_parts(
                                        poke.as_ptr() as *const u8,
                                        poke.len() * core::mem::size_of::<u32>(),
                                    )
                                };
                                let mut rram = cramium_hal::rram::Reram::new();
                                rram.write_slice(addr, poke_inner);
                                crate::println!("RRAM written {:x} into {:x}, {} times", value, addr, count);
                            } else {
                                crate::println!("RRAM value is in hex");
                            }
                        } else {
                            crate::println!(
                                "RRAM addresses are relative to base of RRAM, max 4M, and in hex"
                            );
                        }
                    } else {
                        crate::println!("RRAM address is in hex");
                    }
                } else {
                    crate::println!(
                        "Help: rram <addr> <value> [count], addr/value is in hex, count in decimal"
                    );
                }
            }
            #[cfg(feature = "cramium-soc")]
            "bogomips" => {
                crate::println!("start test");
                // start the RTC
                unsafe { (0x4006100c as *mut u32).write_volatile(1) };
                let mut count: usize;
                unsafe {
                    #[rustfmt::skip]
                    core::arch::asm!(
                        // grab the RTC value
                        "li t0, 0x40061000",
                        "lw t1, 0x0(t0)",
                        "li t3, 0",
                        // wait until the next second
                    "10:",
                        "lw t2, 0x0(t0)",
                        "beq t1, t2, 10b",
                        // start of test
                    "20:",
                        // count outer loops
                        "addi t3, t3, 1",
                        // inner loop 10,000 times
                        "li t4, 10000",
                    "30:",
                        "addi t4, t4, -1",
                        "bne  x0, t4, 30b",
                        // after inner loop, check current time; do another outer loop if time is same
                        "lw t1, 0x0(t0)",
                        "beq t1, t2, 20b",
                        out("t0") _,
                        out("t1") _,
                        out("t2") _,
                        out("t3") count,
                        out("t4") _,
                    );
                }
                crate::println!("{}.{} bogomips", (count * 2 * 10_000) / 1_000_000, (count * 2) % 10_000);
                crate::platform::setup_timer();
            }

            #[cfg(feature = "nto-bio")]
            "bio" => {
                const BIO_TESTS: usize =
                    // get_id
                    1
                    // dma
                    + 4 * 5 + 1
                    // clocking modes
                    // + 4 + 4 + 4 + 4 + 2
                    // stack test
                    + 1
                    // hello word, hello multiverse, aclk_tests
                    + 3
                    // fifo_basic
                    + 1
                    // host_fifo_tests
                    + 1
                    // spi_test
                    // + 1
                    // i2c_test
                    // + 1
                    // complex_i2c_test
                    // + 1
                    // fifo_level_tests
                    + 1
                    // fifo_alias_tests
                    + 1
                    // event_aliases
                    + 1
                    // dmareq_test
                    + 1
                    // bio-mul
                    + 1
                    // filter test
                    + 1;

                crate::println!("--- Starting On-Demand BIO Test Suite ---");
                // Local counter to replace `self.passing_tests` from the TestRunner
                let mut passing_tests = 0;

                let id = get_id();
                crate::println!("BIO ID: {:x}", id);
                if (id >> 16) as usize == BIO_PRIVATE_MEM_LEN {
                    passing_tests += 1;
                } else {
                    crate::println!(
                        "Error: ID mem size does not match: {} != {}",
                        id >> 16,
                        BIO_PRIVATE_MEM_LEN
                    );
                }

                // map the BIO ports to GPIO pins
                let iox = cramium_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                iox.set_ports_from_pio_bitmask(0xFFFF_FFFF);
                iox.set_gpio_pullup(cramium_api::iox::IoxPort::PB, 2, cramium_api::iox::IoxEnable::Enable);
                iox.set_gpio_pullup(cramium_api::iox::IoxPort::PB, 3, cramium_api::iox::IoxEnable::Enable);

                passing_tests += bio_tests::units::hello_multiverse();

                passing_tests += bio_tests::units::hello_world();
                passing_tests += bio_tests::arith::stack_test();

                // safety: this is safe only if the target supports multiplication
                passing_tests += unsafe { bio_tests::arith::mac_test() }; // 1

                passing_tests += bio_tests::units::aclk_tests();

                passing_tests += bio_tests::units::event_aliases();
                passing_tests += bio_tests::units::fifo_alias_tests();

                passing_tests += bio_tests::units::fifo_basic();
                passing_tests += bio_tests::units::host_fifo_tests();

                passing_tests += bio_tests::units::fifo_level_tests();

                passing_tests += bio_tests::dma::filter_test();
                bio_tests::dma::dma_filter_off();
                passing_tests += bio_tests::dma::dmareq_test();

                bio_tests::dma::dma_filter_off();
                crate::println!("*** CLKMODE 3 ***");
                passing_tests += bio_tests::dma::dma_basic(false, 3); // 4
                passing_tests += bio_tests::dma::dma_basic(true, 3); // 4
                passing_tests += bio_tests::dma::dma_bytes(); // 4
                passing_tests += bio_tests::dma::dma_u16(); // 4
                passing_tests += bio_tests::dma::dma_coincident(3); // 4
                passing_tests += bio_tests::dma::dma_multicore(3); // 1

                // passing_tests += bio_tests::spi::spi_test();
                // passing_tests += bio_tests::i2c::i2c_test();
                // passing_tests += bio_tests::i2c::complex_i2c_test();

                // Final report
                crate::println!("\n--- BIO Tests Complete: {}/{} passed. ---\n", passing_tests, BIO_TESTS);
            }
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }
            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!("Commands include: echo, poke, peek, bogomips");
                #[cfg(feature = "cramium-soc")]
                crate::print!(", rram");
                #[cfg(not(feature = "cramium-soc"))]
                crate::print!(", mon");
                #[cfg(feature = "nto-bio")]
                crate::print!(", bio");
                crate::println!("");
            }
        }

        // reset for next loop
        self.do_cmd = false;
        self.cmdline.clear();
    }
}
