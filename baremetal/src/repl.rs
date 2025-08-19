#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[allow(unused_imports)]
#[cfg(feature = "cramium-soc")]
use cramium_api::*;
#[allow(unused_imports)]
use utralib::*;
#[cfg(any(feature = "artybio", feature = "nto-bio"))]
use xous_bio_bdma::*;

pub struct Error {
    pub message: Option<&'static str>,
}
impl Error {
    pub fn none() -> Self { Self { message: None } }

    pub fn help(message: &'static str) -> Self { Self { message: Some(message) } }
}

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

    pub fn process(&mut self) -> Result<(), Error> {
        if !self.do_cmd {
            return Err(Error::none());
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
                    let addr = usize::from_str_radix(&args[0], 16)
                        .map_err(|_| Error::help("Peek address is in hex"))?;

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
                    return Err(Error::help("Help: peek <addr> [count], addr is in hex, count in decimal"));
                }
            }
            "poke" => {
                if args.len() == 2 || args.len() == 3 {
                    let addr = u32::from_str_radix(&args[0], 16)
                        .map_err(|_| Error::help("Poke address is in hex"))?;

                    let value =
                        u32::from_str_radix(&args[1], 16).map_err(|_| Error::help("Poke value is in hex"))?;
                    let count = if args.len() == 3 {
                        if let Ok(count) = u32::from_str_radix(&args[2], 10) { count } else { 1 }
                    } else {
                        1
                    };
                    // safety: it's not safe to do this, the user pokes at their own risk
                    let poke = unsafe { core::slice::from_raw_parts_mut(addr as *mut u32, count as usize) };
                    for d in poke.iter_mut() {
                        *d = value;
                    }
                    crate::println!("Poked {:x} into {:x}, {} times", value, addr, count);
                } else {
                    return Err(Error::help(
                        "Help: poke <addr> <value> [count], addr/value is in hex, count in decimal",
                    ));
                }
            }
            #[cfg(feature = "cramium-soc")]
            "rram" => {
                if args.len() == 2 || args.len() == 3 {
                    let addr = usize::from_str_radix(&args[0], 16)
                        .map_err(|_| Error::help("RRAM address is in hex"))?;
                    if addr < utralib::HW_RERAM_MEM_LEN {
                        let value = u32::from_str_radix(&args[1], 16)
                            .map_err(|_| Error::help("RRAM value is in hex"))?;
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
                        return Err(Error::help(
                            "RRAM addresses are relative to base of RRAM, max 4M, and in hex",
                        ));
                    }
                } else {
                    return Err(Error::help(
                        "Help: rram <addr> <value> [count], addr/value is in hex, count in decimal",
                    ));
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
                // this test must wait then reset - if the next set of tests run
                // too soon after this one, the system will be in a scrambled state.
                crate::platform::delay(20);
                bio_ss.init();
                // passing_tests += bio_tests::units::host_fifo_tests();

                passing_tests += bio_tests::units::fifo_level_tests();

                bio_tests::dma::dma_filter_off();
                passing_tests += bio_tests::dma::dmareq_test();
                // here
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
            #[cfg(feature = "nto-bio")]
            "bdma" => {
                let mut seed = 0;
                loop {
                    crate::platform::bio::bdma_coincident_test(&args, seed);
                    // let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                    // rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
                    // rcurst.wo(utra::sysctrl::SFR_RCURST1, 0x55AA);
                    seed += 1;
                    crate::println!("seed {}", seed);
                    if seed > 64 {
                        break;
                    }
                }
                /* let mut bio_ss = BioSharedState::new();
                bio_ss.init();
                */
            }
            #[cfg(feature = "cramium-soc")]
            "clocks" => {
                if args.len() == 1 {
                    let f = u32::from_str_radix(&args[0], 10)
                        .map_err(|_| Error::help("clock <freq>, where freq is a number from 100-1600"))
                        .and_then(|f| {
                            (f >= 100 && f <= 1600)
                                .then_some(f * 1_000_000)
                                .ok_or(Error::help("frequency should be a number from 100-1600"))
                        })?;
                    crate::println!("Setting clock to: {} MHz", f / 1_000_000);

                    crate::platform::clockset_wrapper(f);
                } else {
                    return Err(Error::help("clocks <CPU freq in MHz, 100-1600>"));
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
            #[cfg(feature = "nto-bio")]
            "pin" => {
                // We need at least a subcommand.
                if args.is_empty() {
                    return Err(Error::help("Usage: pin <set|pwm|read> ..."));
                }

                let subcommand = args[0].as_str();

                // Initialize BIO and IOX once, as they are common to all subcommands.
                let mut bio_ss = BioSharedState::new();
                let iox = cramium_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                iox.set_ports_from_pio_bitmask(0xFFFF_FFFF);

                match subcommand {
                    "set" => {
                        if args.len() < 3 || args.len() > 4 {
                            return Err(Error::help("Usage: pin set <pin#> <on|off> [core]"));
                        }

                        // Arg 1: Pin Number
                        let pin = u32::from_str_radix(&args[1], 10).map_err(|_| {
                            Error::help(
                                "Invalid pin number. Must be 0-31.\n\rUsage: pin set <pin#> <on|off> [core]",
                            )
                        })?;

                        // Arg 2: State
                        let state_str = args[2].as_str().to_lowercase();
                        if state_str != "on" && state_str != "off" {
                            return Err(Error::help(
                                "Invalid state. Use 'on' or 'off'.\n\rUsage: pin set <pin#> <on|off> [core]",
                            ));
                        }
                        let state_bool = state_str == "on";

                        // Arg 3 (Optional): Core ID
                        let target_core: Option<BioCore> = if args.len() == 4 {
                            Some(BioCore::from(
                                usize::from_str_radix(&args[3], 10).map_err(|_| {
                                    Error::help(
                                        "Invalid core ID. Must be 0-3.\n\rUsage: pin set <pin#> <on|off> [core]",
                                    )
                                })
                                .and_then(|c| {
                                    (c < 4)
                                    .then_some(c)
                                    .ok_or(Error::help("Core ID must be 0-3."))
                                })?
                            ))
                        } else {
                            None // `set_pin` function handles the default core.
                        };

                        let core_name = format!("{:?}", target_core.unwrap_or(BioCore::Core0));
                        crate::println!(
                            "Setting pin {} to {} using {}...",
                            pin,
                            state_str.to_uppercase(),
                            core_name
                        );
                        bio_ss.set_pin(pin, state_bool, target_core);
                        crate::println!("Command sent.");
                    }
                    "pwm" => {
                        if args.len() < 3 || args.len() > 4 {
                            return Err(Error::help("Usage: pin pwm <pin#> <on|off> [core]"));
                        }

                        // Arg 1: Pin Number
                        let pin = u32::from_str_radix(&args[1], 10)
                            .map_err( |_| Error::help(
                                "Invalid pin number. Must be 0-31.\n\rUsage: pin pwm <pin#> <on|off> [core]")
                            )
                            .and_then(|n| {
                                (n < 32)
                                    .then_some(n)
                                    .ok_or(Error::help("Pin number out of range. Must be 0-31."))
                            })?;

                        // Arg 2: State
                        let state = args[2].as_str().to_lowercase();
                        if state != "on" && state != "off" {
                            return Err(Error::help(
                                "Invalid state. Use 'on' or 'off'.\n\rUsage: pin pwm <pin#> <on|off> [core]",
                            ));
                        }

                        // Arg 3 (Optional): Core ID
                        let target_core: BioCore = if args.len() == 4 {
                            BioCore::from(
                                usize::from_str_radix(&args[3], 10).map_err(|_| {
                                    Error::help(
                                        "Invalid core ID. Must be 0-3.\n\rUsage: pin pwm <pin#> <on|off> [core]",
                                    )
                                })
                                .and_then(|c| {
                                    (c < 4)
                                    .then_some(c)
                                    .ok_or(Error::help("Core ID must be 0-3."))
                                })?
                            )
                        } else {
                            BioCore::Core0 // Default to Core0 if not specified
                        };

                        if state == "on" {
                            let clock_divisor = 0x5000000;
                            let delay_count = 2000;
                            bio_ss.start_wave_generator(pin, target_core, clock_divisor, delay_count);
                            crate::println!("Generating PWM on pin {} using {:?}.", pin, target_core);
                        } else {
                            bio_ss.stop_wave_generator(target_core);
                            crate::println!("Stopped PWM on {:?}.", target_core);
                        }
                    }
                    "read" => {
                        // Validate arguments: must have a pin number, core is optional.
                        if args.len() < 2 || args.len() > 3 {
                            return Err(Error::help("Usage: pin read <pin#> [core]"));
                        }

                        // Arg 1: Parse the pin number.
                        let pin = u32::from_str_radix(&args[1], 10)
                            .map_err(|_| {
                                Error::help(
                                    "Invalid pin number. Must be 0-31.\n\rUsage: pin read <pin#> [core]",
                                )
                            })
                            .and_then(|n| {
                                (n < 32)
                                    .then_some(n)
                                    .ok_or(Error::help("Pin number out of range. Must be 0-31."))
                            })?;

                        // Arg 2 (Optional): Parse the core ID.
                        let target_core: BioCore = if args.len() == 4 {
                            BioCore::from(
                                usize::from_str_radix(&args[3], 10).map_err(|_| {
                                    Error::help(
                                        "Invalid core ID. Must be 0-3.\n\rUsage: pin set <pin#> <on|off> [core]",
                                    )
                                })
                                .and_then(|c| {
                                    (c < 4)
                                    .then_some(c)
                                    .ok_or(Error::help("Core ID must be 0-3."))
                                })?
                            )
                        } else {
                            BioCore::Core0 // Default to Core0 if not specified
                        };

                        // Call the library function to read the pin state.
                        let is_high = bio_ss.read_pin(pin, target_core);
                        let state_str = if is_high { "high" } else { "low" };

                        crate::println!("Pin {} on {:?} is {}.", pin, target_core, state_str);
                    }
                    _ => {
                        crate::println!(
                            "Unknown pin command: '{}'. Use 'set', 'pwm', or 'read'.",
                            subcommand
                        );
                    }
                }
            }
            #[cfg(all(feature = "cramium-soc", not(feature = "nto-evb")))]
            "ldo" => {
                if args.len() != 1 {
                    return Err(Error::help("vdd85 [on|off]"));
                }
                use cramium_hal::iox::Iox;
                let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                if args[0] == "off" {
                    crate::println!("Check DCDC2");
                    let i2c_channel = cramium_hal::board::setup_i2c_pins(&iox);
                    use cramium_hal::udma::GlobalConfig;
                    let udma_global = GlobalConfig::new();
                    udma_global.clock(PeriphId::from(i2c_channel), true);
                    let i2c_ifram = unsafe {
                        cramium_hal::ifram::IframRange::from_raw_parts(
                            cramium_hal::board::I2C_IFRAM_ADDR,
                            cramium_hal::board::I2C_IFRAM_ADDR,
                            4096,
                        )
                    };
                    let perclk = 100_000_000; // assume this value
                    let mut i2c = unsafe {
                        cramium_hal::udma::I2c::new_with_ifram(
                            i2c_channel,
                            400_000,
                            perclk,
                            i2c_ifram,
                            &udma_global,
                        )
                    };

                    if let Ok(mut pmic) = cramium_hal::axp2101::Axp2101::new(&mut i2c) {
                        pmic.update(&mut i2c).ok();
                        if let Some((voltage, _dvm)) = pmic.get_dcdc(cramium_hal::axp2101::WhichDcDc::Dcdc2) {
                            crate::println!("DCDC2 is on and {:.2}v", voltage);
                        } else {
                            crate::println!("DCDC is off, turning it on!");
                            match pmic.set_dcdc(
                                &mut i2c,
                                Some((0.88, true)),
                                cramium_hal::axp2101::WhichDcDc::Dcdc2,
                            ) {
                                Ok(_) => crate::println!("turned on DCDC2"),
                                Err(_) => {
                                    return Err(Error::help("coludn't turn off DCDC2, aborted!"));
                                }
                            }
                        }
                    }

                    // shut down LDO
                    crate::println!("Engage DCDC2 FET");
                    iox.set_gpio_pin_value(IoxPort::PA, 5, IoxValue::Low);
                    // just "Lower the voltage"
                    let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
                    ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRMLP0, 0x08420002); // 0.7v
                    let mut cgu = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                    cgu.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
                } else {
                    crate::println!("Disengage DCDC2 FET");
                    iox.set_gpio_pin_value(IoxPort::PA, 5, IoxValue::High);
                }
            }
            #[cfg(all(feature = "cramium-soc", not(feature = "nto-evb")))]
            "wfi" => {
                let iox = cramium_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

                let ao_bkupreg = CSR::new(utralib::HW_AOBUREG_BASE as *mut u32);
                for i in 0..8 {
                    crate::println!("backup reg {}: {:08x}", i, unsafe {
                        ao_bkupreg.base().add(i).read_volatile()
                    });
                }
                let bkp = ao_bkupreg.r(utra::aobureg::SFR_BUREG_CR_BUREGS0);
                // this will cause the backup regs to increment every wakeup cycle
                for i in 0..8 {
                    unsafe {
                        ao_bkupreg.base().add(i).write_volatile(0xcafe_0001 + i as u32 + (bkp & 0xFFFF))
                    };
                }

                // pin map to PF is controlled here, for some reason...
                let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_IOX, 1);
                // ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x10_10);
                // ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x3FFFF);
                ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x3F);
                crate::println!("ao wkupmsk: {:x}", ao_sysctrl.r(utra::ao_sysctrl::CR_WKUPMASK));
                for pin in 2..=9 {
                    iox.setup_pin(
                        IoxPort::PF,
                        pin,
                        Some(IoxDir::Input),
                        Some(IoxFunction::AF1),
                        Some(IoxEnable::Enable),
                        Some(IoxEnable::Enable),
                        Some(IoxEnable::Enable),
                        Some(IoxDriveStrength::Drive2mA),
                    );
                }
                let mut dkpc = CSR::new(utralib::HW_DKPC_BASE as *mut u32);
                dkpc.wo(utra::dkpc::SFR_CFG0, 0x1d);
                dkpc.wo(utra::dkpc::SFR_CFG1, 0x04_02_02);
                dkpc.wo(utra::dkpc::SFR_CFG2, 0x0804_02_02);
                dkpc.wo(utra::dkpc::SFR_CFG4, 20);
                while dkpc.r(utra::dkpc::SFR_SR1) != 0 {
                    // this register didn't get mapped in register extraction because its type
                    // is `apb_buf2`: FIXME - adjust the register extraction script to capture this type.
                    // this register drains the pending interrupts from the wakeup/keyboard queue
                    let _ = unsafe { dkpc.base().add(8).read_volatile() };
                }
                let mut irqarray2 = CSR::new(utralib::HW_IRQARRAY2_BASE as *mut u32);
                irqarray2.wo(utra::irqarray2::EV_PENDING, 0xFFFF_FFFF);
                irqarray2.wo(utra::irqarray2::EV_ENABLE, 0xFFFF_FFFF);
                // crate::platform::irq::enable_irq(utra::irqarray2::IRQARRAY2_IRQ);
                let forever = if args.len() > 0 { args[0] == "loop" } else { false };

                // current status of WFI:
                //  - we can scan the KP inputs using the KP scan mechanism
                //  - stuff queues up in the fifo
                //  - we see the vld bit set
                //  - we can't get an interrupt to fire.
                let mut count = 0; // just so we know the machine hasn't crashed
                loop {
                    crate::print!("{}|", count);
                    for i in (0..6).chain(12..13).chain(8..9) {
                        crate::print!("{:x}: {:x} ", i * 4, unsafe { dkpc.base().add(i).read_volatile() });
                    }
                    let fr = ao_sysctrl.r(utra::ao_sysctrl::SFR_AOFR);
                    crate::print!(
                        "int: {:x}/{:x}/{:x}",
                        irqarray2.r(utra::irqarray2::EV_PENDING),
                        irqarray2.r(utra::irqarray2::EV_STATUS),
                        fr
                    );
                    ao_sysctrl.wo(utra::ao_sysctrl::SFR_AOFR, fr);
                    crate::println!("");
                    if !forever {
                        break;
                    } else {
                        crate::platform::delay(500);
                    }
                    count += 1;
                }

                // vexriscv::register::vexriscv::mim::write(0x0); // disable all interrupts

                crate::println!("entering wfi");
                // bring us down to 100MHz so we can turn off regulators
                let perclk = crate::platform::clockset_wrapper(100_000_000);
                crate::println!("clocks @ 100MHz");

                // configure PMIC to be off
                let i2c_channel = cramium_hal::board::setup_i2c_pins(&iox);
                use cramium_hal::udma::GlobalConfig;
                let udma_global = GlobalConfig::new();
                udma_global.clock(PeriphId::from(i2c_channel), true);
                let i2c_ifram = unsafe {
                    cramium_hal::ifram::IframRange::from_raw_parts(
                        cramium_hal::board::I2C_IFRAM_ADDR,
                        cramium_hal::board::I2C_IFRAM_ADDR,
                        4096,
                    )
                };
                let mut i2c = unsafe {
                    cramium_hal::udma::I2c::new_with_ifram(
                        i2c_channel,
                        400_000,
                        perclk,
                        i2c_ifram,
                        &udma_global,
                    )
                };

                if let Ok(mut pmic) = cramium_hal::axp2101::Axp2101::new(&mut i2c) {
                    match pmic.set_dcdc(&mut i2c, None, cramium_hal::axp2101::WhichDcDc::Dcdc2) {
                        Ok(_) => crate::println!("turned off DCDC2"),
                        Err(_) => crate::println!("couldn't turn off DCDC2"),
                    }
                }

                unsafe {
                    crate::platform::low_power();
                }
                crate::println!("pmu cr: {:x}", ao_sysctrl.r(utra::ao_sysctrl::SFR_PMUCSR));
                crate::println!("pmu status: {:x}", ao_sysctrl.r(utra::ao_sysctrl::SFR_PMUSR));
                crate::println!("pmu err: {:x}", ao_sysctrl.r(utra::ao_sysctrl::SFR_PMUFR));

                crate::println!("trying PD mode");
                ao_sysctrl.wo(utra::ao_sysctrl::CR_CR, 7);
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRPD, 0x4c);
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRMLP0, 0x08420002); // 0.7v
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUPDAR, 0x5a);
                crate::println!("entered PD mode");

                // turn regulator off - system of course does not work
                /*
                crate::println!("attempting to turn off VDD085");
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCSR, 0x4c);
                let mut cgu = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                cgu.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
                for _ in 0..1024 {
                    unsafe { core::arch::asm!("nop") };
                }
                crate::println!("PD pmu cr: {:x}", ao_sysctrl.r(utra::ao_sysctrl::SFR_PMUCSR));
                */

                unsafe { core::arch::asm!("wfi", "nop", "nop", "nop", "nop") };

                // turn regulator back on
                /*
                ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCSR, 0x7c);
                let mut cgu = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                cgu.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
                for _ in 0..1024 {
                    unsafe { core::arch::asm!("nop") };
                }
                crate::println!("PU pmu cr: {:x}", ao_sysctrl.r(utra::ao_sysctrl::SFR_PMUCSR));
                */
                crate::platform::clockset_wrapper(800_000_000);
                crate::println!("exiting wfi");
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
                crate::print!(", bio, bdma, pin");
                #[cfg(all(feature = "cramium-soc", not(feature = "nto-evb")))]
                crate::print!(", ldo, wfi");
                crate::println!("");
            }
        }

        // reset for next loop
        self.abort_cmd();
        Ok(())
    }

    pub fn abort_cmd(&mut self) {
        self.do_cmd = false;
        self.cmdline.clear();
    }
}
