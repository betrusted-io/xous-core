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
                    // + 1 // can't work because sim loopback isn't there
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
                // let iox = cramium_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                // iox.set_ports_from_pio_bitmask(0xFFFF_FFFF);

                crate::println!("Resetting block");
                let mut bio_ss = BioSharedState::new();
                bio_ss.init();

                // run the simple tests to debug some basic core logics
                if false {
                    // let mut bio_ss = BioSharedState::new();
                    // stop all the machines, so that code can be loaded
                    bio_ss.bio.wo(utra::bio_bdma::SFR_EXTCLOCK, 0);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x0);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x0);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x0);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x0);
                    crate::println!(
                        "rxflevel0: {}",
                        bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0)
                    );
                    bio_ss.load_code(simple_test_code(), 0, BioCore::Core3);
                    bio_ss.set_core_run_states([false, false, false, true]);
                    for i in 0..12 {
                        debug_bio(&bio_ss);
                        bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, i);
                        crate::println!(
                            "f0:{} f1:{} f2:{} f3:{}",
                            bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0),
                            bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1),
                            bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2),
                            bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3)
                        );
                        crate::println!(
                            "{:x}, {:x}",
                            bio_ss.bio.rf(utra::bio_bdma::SFR_RXF1_FDOUT),
                            bio_ss.bio.rf(utra::bio_bdma::SFR_RXF2_FDOUT),
                        );
                    }
                    bio_ss.init();
                }

                passing_tests += bio_tests::arith::stack_test();

                passing_tests += bio_tests::units::hello_multiverse();

                passing_tests += bio_tests::units::hello_world();
                bio_ss.init();

                // safety: this is safe only if the target supports multiplication
                passing_tests += unsafe { bio_tests::arith::mac_test() }; // 1

                passing_tests += bio_tests::units::aclk_tests();

                passing_tests += bio_tests::units::event_aliases();
                passing_tests += bio_tests::units::fifo_alias_tests();

                passing_tests += bio_tests::units::fifo_basic();
                // passing_tests += bio_tests::units::host_fifo_tests();

                passing_tests += bio_tests::units::fifo_level_tests();

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

                bio_ss.init();
                passing_tests += bio_tests::dma::filter_test();

                // passing_tests += bio_tests::spi::spi_test();
                // passing_tests += bio_tests::i2c::i2c_test();
                // passing_tests += bio_tests::i2c::complex_i2c_test();

                // Final report
                crate::println!("\n--- BIO Tests Complete: {}/{} passed. ---\n", passing_tests, BIO_TESTS);
            }
            #[cfg(feature = "cramium-soc")]
            "clocks" => {
                use cramium_hal::udma;
                if args.len() == 1 {
                    let freq = match u32::from_str_radix(&args[0], 10) {
                        Ok(f) => {
                            if f >= 100 && f <= 1600 {
                                f * 1_000_000
                            } else {
                                crate::println!("{} should be a number from 100-1600", args[0]);
                                self.abort_cmd();
                                return;
                            }
                        }
                        _ => {
                            crate::println!("{} should be a number from 100-1600", args[0]);
                            self.abort_cmd();
                            return;
                        }
                    };
                    crate::println!("Setting clock to: {} MHz", freq / 1_000_000);

                    // reset the baud rate on the console UART
                    let perclk = unsafe { crate::platform::init_clock_asic(freq) };
                    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
                    #[cfg(feature = "nto-evb")]
                    let mut udma_uart = unsafe {
                        // safety: this is safe to call, because we set up clock and events prior to calling
                        // new.
                        udma::Uart::get_handle(
                            utra::udma_uart_1::HW_UDMA_UART_1_BASE,
                            uart_buf_addr,
                            uart_buf_addr,
                        )
                    };
                    #[cfg(not(feature = "nto-evb"))]
                    let mut udma_uart = unsafe {
                        // safety: this is safe to call, because we set up clock and events prior to calling
                        // new.
                        udma::Uart::get_handle(
                            utra::udma_uart_2::HW_UDMA_UART_2_BASE,
                            uart_buf_addr,
                            uart_buf_addr,
                        )
                    };
                    let baudrate: u32 = 115200;
                    let freq: u32 = perclk / 2;
                    udma_uart.set_baud(baudrate, freq);

                    crate::println!("clock set done, perclk is {} MHz", perclk / 1_000_000);
                    udma_uart.write("console up with clocks\r\n".as_bytes());
                } else {
                    crate::println!("clocks <CPU freq in MHz>")
                }
            }
            #[cfg(feature = "cramium-soc")]
            "usb" => {
                crate::println!("USB basic test...");
                let csr = cramium_hal::usb::compat::AtomicCsr::new(
                    cramium_hal::usb::utra::CORIGINE_USB_BASE as *mut u32,
                );
                let irq_csr = cramium_hal::usb::compat::AtomicCsr::new(
                    utralib::utra::irqarray1::HW_IRQARRAY1_BASE as *mut u32,
                );
                crate::println!("inspect USB region...");
                let usbregs = 0x50202400 as *const u32;
                for i in 0..32 {
                    crate::println!("{:x}, {:08x}", i, unsafe {
                        usbregs
                            .add(cramium_hal::usb::utra::CORIGINE_DEV_OFFSET / size_of::<u32>() + i)
                            .read_volatile()
                    });
                }
                // safety: this is safe because we are in machine mode, and vaddr/paddr always pairs up
                crate::println!("Getting pointer...");
                let mut usb = unsafe {
                    cramium_hal::usb::driver::CorigineUsb::new(
                        cramium_hal::board::CRG_UDC_MEMBASE,
                        csr.clone(),
                        irq_csr.clone(),
                    )
                };
                crate::println!("Reset");
                usb.reset();
                let mut idle_timer = 0;
                let mut vbus_on = false;
                let mut vbus_on_count = 0;
                let mut in_u0 = false;
                let mut last_sc = 0;
                loop {
                    let next_sc = csr.r(cramium_hal::usb::utra::PORTSC);
                    if last_sc != next_sc {
                        last_sc = next_sc;
                        crate::println!("**** SC update {:x?}", cramium_hal::usb::driver::PortSc(next_sc));
                        /*
                        if cramium_hal::usb::driver::PortSc(next_sc).pr() {
                            crate::println!("  >>reset<<");
                            usb.start();
                            in_u0 = false;
                            vbus_on_count = 0;
                        }
                        */
                    }
                    let event = usb.udc_handle_interrupt();
                    if event == cramium_hal::usb::driver::CrgEvent::None {
                        idle_timer += 1;
                    } else {
                        crate::println!("*Event {:?} at {}", event, idle_timer);
                        idle_timer = 0;
                    }

                    if !vbus_on && vbus_on_count == 4 {
                        crate::println!("*Vbus on");
                        usb.reset();
                        usb.init();
                        usb.start();
                        vbus_on = true;
                        in_u0 = false;

                        let irq1 = irq_csr.r(utralib::utra::irqarray1::EV_PENDING);
                        crate::println!(
                            "irq1: {:x}, status: {:x}",
                            irq1,
                            csr.r(cramium_hal::usb::utra::USBSTS)
                        );
                        irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, irq1);
                        // restore this to go on to boot
                        // break;
                    } else if usb.pp() && !vbus_on {
                        vbus_on_count += 1;
                        crate::println!("*Vbus_on_count: {}", vbus_on_count);
                        // mdelay(100);
                    } else if !usb.pp() && vbus_on {
                        crate::println!("*Vbus off");
                        usb.stop();
                        usb.reset();
                        vbus_on_count = 0;
                        vbus_on = false;
                        in_u0 = false;
                    } else if in_u0 && vbus_on {
                        // usb.udc_handle_interrupt();
                        // TODO
                    } else if usb.ccs() && vbus_on {
                        // usb.print_status(usb.csr.r(cramium_hal::usb::utra::PORTSC));
                        crate::println!("*Enter U0");
                        in_u0 = true;
                        let irq1 = irq_csr.r(utralib::utra::irqarray1::EV_PENDING);
                        // usb.print_status(csr.r(cramium_hal::usb::utra::PORTSC));
                        irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, irq1);
                    }
                }
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
                crate::print!(", rram, clocks, usb");
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

    #[allow(dead_code)]
    fn abort_cmd(&mut self) {
        self.do_cmd = false;
        self.cmdline.clear();
    }
}

#[rustfmt::skip]
bio_code!(simple_test_code, SIMPLE_TEST_START, SIMPLE_TEST_END,
    "li sp, 0x800",
  "10:",
    "mv t0, x31",
    "mv t1, x16",
    "sw t1, 0(sp)",
    "lw t2, 0(sp)",
    "addi t2, t2, 0x200",
    "mv x17, t2",
    "mv x18, t0",
    // "mv x20, x0",
    "j 10b"
);

#[rustfmt::skip]
bio_code!(ldst_code, LDST_START, LDST_END,
    "sw x0, 0x20(x0)",
    "li sp, 0x61200000",
    "addi sp, sp, -4",
    "sw x0, 0(sp)",
  "10:",
    "j 10b"
);

fn debug_bio(bio_ss: &BioSharedState) {
    crate::println!(
        "c0:{:04x} c1:{:04x} c2:{:04x} c3:{:04x}",
        bio_ss.bio.r(utra::bio_bdma::SFR_DBG0),
        bio_ss.bio.r(utra::bio_bdma::SFR_DBG1),
        bio_ss.bio.r(utra::bio_bdma::SFR_DBG2),
        bio_ss.bio.r(utra::bio_bdma::SFR_DBG3),
    );
}
