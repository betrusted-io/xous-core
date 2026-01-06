#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[allow(unused_imports)]
#[cfg(feature = "bao1x")]
use bao1x_api::*;
#[allow(unused_imports)]
use bao1x_hal::board::{BOOKEND_END, BOOKEND_START};
#[allow(unused_imports)]
use utralib::*;
#[cfg(any(feature = "artybio", feature = "bao1x-bio"))]
use xous_bio_bdma::*;

#[cfg(feature = "bao1x-trng")]
static TRNG_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// courtesy of claude
#[cfg(feature = "bao1x")]
const K_DATA: &'static [u8; 853] = b"The quick brown fox jumps over the lazy dog while contemplating \
the meaning of existence in a digital world. Numbers like 123456789 and symbols @#$%^&*() add variety to this test \
message. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore \
magna aliqua. Testing patterns: ABCDEFGHIJKLMNOPQRSTUVWXYZ and abcdefghijklmnopqrstuvwxyz provide full alphabet coverage. \
Special characters !@#$%^&*()_+-=[]{}|;':.,.<>?/ enhance the diversity of this sample text. The year 2024 brings new \
challenges and opportunities for software development and testing methodologies. Random words like elephant, butterfly, \
quantum, nebula, crystalline, harmonic, and serendipity fill the remaining space. Pi equals 3.14159265358979323846 \
approximately. This text serves as a placeholder for various testing scenarios!!!";

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

    #[allow(dead_code)]
    pub fn init_cmd(&mut self, cmd: &str) {
        self.cmdline.push_str(cmd);
        self.cmdline.push('\n');
        self.do_cmd = true;
    }

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
            #[cfg(not(feature = "bao1x"))]
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
                        .map_err(|_| Error::help("Peek address is in hex, no leading 0x"))?;

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
                        .map_err(|_| Error::help("Poke address is in hex, no leading 0x"))?;

                    let value = u32::from_str_radix(&args[1], 16)
                        .map_err(|_| Error::help("Poke value is in hex, no leading 0x"))?;
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
            #[cfg(feature = "spim-tests")]
            "qe" => {
                use bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR;
                use bao1x_hal::iox::Iox;
                use bao1x_hal::udma::{GlobalConfig, Spim, SpimClkPha, SpimClkPol, SpimCs};

                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));

                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but run it as fast
                        // as we can run perclk
                        100_000_000 / 4,
                        100_000_000 / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        16, // just enough space to send commands
                        4096,
                        Some(6),
                        None,
                        bao1x_hal::ifram::IframRange::from_raw_parts(
                            SPIM_FLASH_IFRAM_ADDR,
                            SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                flash_spim.mem_qpi_mode(false);
                let status = flash_spim.flash_read_status_register();
                crate::println!("status register: {:x}", status);
                flash_spim.flash_set_qe();
                let status = flash_spim.flash_read_status_register();
                crate::println!("status register after qe set: {:x}", status);
            }
            #[cfg(feature = "spim-tests")]
            "check_qpi" => {
                use bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR;
                use bao1x_hal::iox::Iox;
                use bao1x_hal::udma::{GlobalConfig, Spim, SpimClkPha, SpimClkPol, SpimCs};
                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));
                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but run it as fast
                        // as we can run perclk
                        100_000_000 / 4,
                        100_000_000 / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        16, // just enough space to send commands
                        4096,
                        Some(6),
                        None,
                        bao1x_hal::ifram::IframRange::from_raw_parts(
                            SPIM_FLASH_IFRAM_ADDR,
                            SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                flash_spim.mem_read_id_flash();
                // turn off QPI mode, in case it was set from a reboot in a bad state
                flash_spim.mem_qpi_mode(false);

                // sanity check: read ID
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("flash ID (init): {:x}", flash_id);
                flash_spim.mem_qpi_mode(true);

                // re-check the ID to confirm we entered QPI mode correctly
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);
                flash_spim.mem_qpi_mode(false);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("SPI flash ID: {:x}", flash_id);
                flash_spim.mem_qpi_mode(true);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);
            }
            #[cfg(feature = "bao1x")]
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
                        let mut rram = bao1x_hal::rram::Reram::new();
                        rram.write_slice(addr, poke_inner).ok();
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
            #[cfg(feature = "bao1x")]
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

            #[cfg(feature = "bao1x-bio")]
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
                crate::println!("** Debug output is on DUART, not console **");
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
                // let iox = bao1x_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                // iox.set_ports_from_bio_bitmask(0xFFFF_FFFF);

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
            #[cfg(feature = "bao1x-bio")]
            "bdma" => {
                let mut seed = 0;
                let mut clkmode: u32 = 3;
                let mut passing: bool;
                loop {
                    crate::println!("seed {}, clkmode {}", seed, clkmode);
                    if crate::platform::bio::bdma_coincident_test(&args, seed, clkmode) != 4 {
                        clkmode = (clkmode + 1) % 4;
                        passing = false;
                        crate::println!("~~BDMAFAIL~~");
                    } else {
                        passing = true
                    };
                    crate::println!("  pasing: {:?}", passing);
                    seed += 1;
                    if passing || seed > 8 {
                        break;
                    }
                }
            }
            "reset" => {
                let mut csr = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                csr.wo(utra::sysctrl::SFR_RCURST0, 0x0000_55aa);
            }
            #[cfg(feature = "bao1x")]
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
            #[cfg(feature = "bao1x-buttons")]
            "buttons" => {
                use bao1x_hal::iox::Iox;
                let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                let (rows, cols) = bao1x_hal::board::baosec::setup_kb_pins(&iox);
                loop {
                    let kps = crate::scan_keyboard(&iox, &rows, &cols);
                    for kp in kps {
                        if kp != crate::KeyPress::None {
                            crate::println!("Got key: {:?}", kp);
                        }
                    }
                }
            }
            #[cfg(feature = "bao1x-bio")]
            "pin" => {
                // We need at least a subcommand.
                if args.is_empty() {
                    return Err(Error::help("Usage: pin <set|pwm|read> ..."));
                }

                let subcommand = args[0].as_str();

                // Initialize BIO and IOX once, as they are common to all subcommands.
                let mut bio_ss = BioSharedState::new();
                let iox = bao1x_hal::iox::Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                iox.set_ports_from_bio_bitmask(0xFFFF_FFFF, bio::IoConfigMode::Overwrite);

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
            #[cfg(all(feature = "bao1x", feature = "board-baosec"))]
            "ldo" => {
                if args.len() != 1 {
                    return Err(Error::help("vdd85 [on|off]"));
                }
                use bao1x_hal::iox::Iox;
                let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                if args[0] == "off" {
                    crate::println!("Check DCDC2");
                    let i2c_channel = bao1x_hal::board::setup_i2c_pins(&iox);
                    use bao1x_hal::udma::GlobalConfig;
                    let udma_global = GlobalConfig::new();
                    udma_global.clock(PeriphId::from(i2c_channel), true);
                    let i2c_ifram = unsafe {
                        bao1x_hal::ifram::IframRange::from_raw_parts(
                            bao1x_hal::board::I2C_IFRAM_ADDR,
                            bao1x_hal::board::I2C_IFRAM_ADDR,
                            4096,
                        )
                    };
                    let perclk = 100_000_000; // assume this value
                    let mut i2c = unsafe {
                        bao1x_hal::udma::I2cDriver::new_with_ifram(
                            i2c_channel,
                            400_000,
                            perclk,
                            i2c_ifram,
                            &udma_global,
                        )
                    };

                    if let Ok(mut pmic) = bao1x_hal::axp2101::Axp2101::new(&mut i2c) {
                        pmic.update(&mut i2c).ok();
                        if let Some((voltage, _dvm)) = pmic.get_dcdc(bao1x_hal::axp2101::WhichDcDc::Dcdc2) {
                            crate::println!(
                                "DCDC2 is on and {}.{}v",
                                voltage as i32,
                                (voltage * 100.0) as i32 % 100
                            );
                        } else {
                            crate::println!("DCDC is off, turning it on!");
                            match pmic.set_dcdc(
                                &mut i2c,
                                Some((0.88, true)),
                                bao1x_hal::axp2101::WhichDcDc::Dcdc2,
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
            #[cfg(feature = "ci-security")]
            // write protection only works after we leave the boot environment, thus it has to be tested in
            // baremetal
            "boot0-wp" => {
                crate::println!("Write protection test");
                let mut rram = bao1x_hal::rram::Reram::new();
                // boot0 should end at 0x2_0000; boot1 ends at 0x6_0000
                // expected result: the first four should have the update fail
                // the fifth value (in boot1) should update
                let regions =
                    [(false, 0x0), (false, 0xF000), (false, 0x1_1000), (false, 0x1_F000), (true, 0x5_F000)];
                let mut passing = true;
                for (i, &(writable, region)) in regions.iter().enumerate() {
                    let ref_data = unsafe {
                        core::slice::from_raw_parts((region + utralib::HW_RERAM_MEM) as *const u8, 32)
                    };
                    let base = ref_data[0]; // increment whatever is in the original area
                    let try_write = [base + 1 + i as u8; 32];
                    // crate::println!("orig ({:x}): {:x?}", region, test_data);
                    rram.write_slice(region, &try_write).ok(); // we expect write errors, don't panic on failure
                    if writable {
                        if ref_data != try_write {
                            crate::println!(
                                "Failure on writeable {:?} ({:x}): {:x?} vs {:x?}",
                                writable,
                                region,
                                ref_data,
                                try_write
                            );
                            passing = false;
                        }
                    } else {
                        if ref_data == try_write {
                            crate::println!(
                                "Failure on writeable {:?} ({:x}): {:x?} vs {:x?}",
                                writable,
                                region,
                                ref_data,
                                try_write
                            );
                            passing = false;
                        }
                    }
                }
                if passing {
                    crate::println!("{}SEC.BOOT0WP-PASS,{}", BOOKEND_START, BOOKEND_END);
                } else {
                    crate::println!("{}SEC.BOOT0WP-FAIL,{}", BOOKEND_START, BOOKEND_END);
                }
            }
            #[cfg(feature = "bao1x-trng")]
            "trngro" => {
                use base64::{Engine as _, engine::general_purpose};
                fn encode_base64(input: &[u8]) -> String { general_purpose::STANDARD.encode(input) }
                fn as_u8_slice(slice: &[u32]) -> &[u8] {
                    let len = slice.len() * size_of::<u32>();
                    unsafe { core::slice::from_raw_parts(slice.as_ptr() as *const u8, len) }
                }

                let mut trng = bao1x_hal::sce::trng::Trng::new(utralib::utra::trng::HW_TRNG_BASE);
                if TRNG_INIT.swap(true, core::sync::atomic::Ordering::SeqCst) == false {
                    crate::println!("setting up TRNG");
                    trng.setup_raw_generation(256);
                    trng.start();
                } else {
                    // safety: the handle is already initialized
                    unsafe {
                        trng.force_mode(bao1x_hal::sce::trng::Mode::Raw);
                    }
                }
                crate::println!("====ROSTART====");
                const BUFLEN: usize = 256;
                // test code for checking performance & alignment of decode data - eliminates
                // encoding & trng generation overhead
                // let b64_as_slice =
                // b"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+P0BBQkNERUZHSElKS0xNTk9QUVJTVFVWV1hZWltcXV5fYGFiY2RlZmdoaWprbG1ub3BxcnN0dXZ3eHl6e3x9fn+AgYKDhIWGh4iJiouMjY6PkJGSk5SVlpeYmZqbnJ2en6ChoqOkpaanqKmqq6ytrq+wsbKztLW2t7i5uru8vb6/
                // wMHCw8TFxsfIycrLzM3Oz9DR0tPU1dbX2Nna29zd3t/g4eLj5OXm5+jp6uvs7e7v8PHy8/T19vf4+fr7/P3+/w==";

                loop {
                    let mut buf = [0u32; BUFLEN / size_of::<u32>()];
                    #[cfg(not(feature = "trng-debug"))]
                    for d in buf.iter_mut() {
                        loop {
                            if let Some(word) = trng.get_u32() {
                                *d = word;
                                break;
                            }
                        }
                    }
                    #[cfg(feature = "trng-debug")]
                    crate::trng_ro(
                        0x0000001F,
                        0x0000FFFF,
                        0x00000002,
                        crate::TrngOpt::RngB,
                        0xFFFFFFFF,
                        0xFFFFFFFF,
                        0xFFFFFFFF,
                        0xFFFFFFFF,
                        &mut buf,
                        true,
                    );
                    let buf_u8 = as_u8_slice(&buf);
                    let b64 = encode_base64(buf_u8);
                    unsafe {
                        if let Some(ref mut usb_ref) = crate::platform::usb::USB {
                            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
                            let tx_buf = usb.cdc_acm_tx_slice();
                            let b64_as_slice = b64.as_bytes();
                            assert!(b64_as_slice.len() + 1 < tx_buf.len()); // this is ensured by adjusting BUFLEN
                            tx_buf[..b64_as_slice.len()].copy_from_slice(b64_as_slice);
                            tx_buf[b64_as_slice.len()] = '\n' as char as u32 as u8;
                            while !crate::platform::usb::TX_IDLE
                                .swap(false, core::sync::atomic::Ordering::SeqCst)
                            {
                                // wait for tx to go idle
                            }
                            usb.bulk_xfer(
                                3,
                                bao1x_hal::usb::driver::USB_SEND,
                                tx_buf.as_ptr() as usize,
                                b64_as_slice.len() + 1,
                                0,
                                0,
                            );
                        } else {
                            panic!("USB core not allocated, can't proceed!");
                        }
                    }
                }
            }
            #[cfg(all(feature = "bao1x-trng", feature = "board-baosec"))]
            "trngav" => {
                use base64::{Engine as _, engine::general_purpose};
                fn encode_base64(input: &[u8]) -> String { general_purpose::STANDARD.encode(input) }
                fn as_u8_slice(slice: &[u32]) -> &[u8] {
                    let len = slice.len() * size_of::<u32>();
                    unsafe { core::slice::from_raw_parts(slice.as_ptr() as *const u8, len) }
                }

                use bao1x_hal::iox::Iox;
                use xous_bio_bdma::*;

                let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
                let (power_port, power_pin) = bao1x_hal::board::setup_trng_power_pin(&iox);
                let bio_input = bao1x_hal::board::setup_trng_input_pin(&iox);
                // turn on the system
                iox.set_gpio_pin_value(power_port, power_pin, IoxValue::High);

                let mut bio_ss = BioSharedState::new();
                bio_ss.init();
                crate::println!("bio_input: {}", bio_input);
                crate::avtrng::setup(&mut bio_ss, bio_input);

                crate::println!("====AVSTART====");
                const BUFLEN: usize = 256;

                crate::usb::flush();

                const BITS_PER_SAMPLE: usize = 4;
                loop {
                    let mut buf = [0u32; BUFLEN / size_of::<u32>()];
                    for d in buf.iter_mut() {
                        let mut raw32 = 0;
                        for _ in 0..(size_of::<u32>() * 8) / BITS_PER_SAMPLE {
                            // wait for the next interval to arrive
                            while bio_ss.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) == 0 {}
                            let raw = bio_ss.bio.r(utra::bio_bdma::SFR_RXF0);
                            // crate::println!("raw: {:x}", raw);
                            raw32 <<= BITS_PER_SAMPLE;
                            // shift right by one because bit 0 always samples as 0, due to instruction timing
                            raw32 |= (raw >> 1) & ((1 << BITS_PER_SAMPLE) - 1)
                        }
                        *d = raw32;
                    }
                    let buf_u8 = as_u8_slice(&buf);
                    let b64 = encode_base64(buf_u8);
                    unsafe {
                        if let Some(ref mut usb_ref) = crate::platform::usb::USB {
                            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
                            let tx_buf = usb.cdc_acm_tx_slice();
                            let b64_as_slice = b64.as_bytes();
                            assert!(b64_as_slice.len() + 1 < tx_buf.len()); // this is ensured by adjusting BUFLEN
                            tx_buf[..b64_as_slice.len()].copy_from_slice(b64_as_slice);
                            tx_buf[b64_as_slice.len()] = '\n' as char as u32 as u8;
                            while !crate::platform::usb::TX_IDLE
                                .swap(false, core::sync::atomic::Ordering::SeqCst)
                            {
                                // wait for tx to go idle
                            }
                            usb.bulk_xfer(
                                3,
                                bao1x_hal::usb::driver::USB_SEND,
                                tx_buf.as_ptr() as usize,
                                b64_as_slice.len() + 1,
                                0,
                                0,
                            );
                        } else {
                            panic!("USB core not allocated, can't proceed!");
                        }
                    }
                }
            }
            #[cfg(feature = "bao1x")]
            "sha2" => {
                use digest::Digest;
                use hex_literal::hex;
                use sha2_bao1x::Sha256;

                const K_EXPECTED_DIGEST_256: [u8; 32] =
                    hex!("de1b3b58e16d6b12c906898025d4bc5a594075f4fd4252fa88128b2e0b7a266a");

                let mut pass: bool = true;
                let mut hasher = Sha256::new();

                hasher.update(K_DATA);
                // hasher.update(data);
                let digest = hasher.finalize();

                for (&expected, result) in K_EXPECTED_DIGEST_256.iter().zip(digest) {
                    if expected != result {
                        pass = false;
                    }
                }
                if pass {
                    crate::println!("Sha256 passed.");
                } else {
                    crate::println!("Sha256 failed: {:x?}", digest);
                }
            }
            #[cfg(feature = "bao1x")]
            "sha5" => {
                use digest::Digest;
                use hex_literal::hex;
                use sha2_bao1x::Sha512;

                // generated by claude.
                const K_EXPECTED_DIGEST_512: [u8; 64] = hex!(
                    "e827276a7d5f2653fe27abc0b0c86533e75acfd4d75253b1229bf86aee19e4a0722691e9ef60510892dc60f4edff795d7875d0d8293a39a7a327a7e1bf07000a"
                );

                let mut pass: bool = true;
                let mut hasher = Sha512::new();

                hasher.update(K_DATA);
                // hasher.update(data);
                let digest = hasher.finalize();

                for (&expected, result) in K_EXPECTED_DIGEST_512.iter().zip(digest) {
                    if expected != result {
                        pass = false;
                    }
                }
                if pass {
                    crate::println!("Sha512 passed.");
                } else {
                    crate::println!("Sha512 failed: {:x?}", digest);
                }
            }
            #[cfg(feature = "dabao-selftest")]
            "dbtest" => {
                crate::dabao_selftest::dabao_selftest();
            }
            "actest" => {
                let slot_man = bao1x_hal::acram::SlotManager::new();
                crate::println!(
                    "Slot 0(d): {:x?}",
                    slot_man.read(&bao1x_api::offsets::SlotIndex::Data(
                        0,
                        PartitionAccess::Unspecified,
                        RwPerms::Unspecified
                    ))
                );
                crate::println!(
                    "Slot 1(d): {:x?}",
                    slot_man.read(&bao1x_api::offsets::SlotIndex::Data(
                        1,
                        PartitionAccess::Unspecified,
                        RwPerms::Unspecified
                    ))
                );
            }
            #[cfg(feature = "board-baosec")]
            "pddberase" => {
                use bao1x_api::{baosec::PDDB_LEN, baosec::PDDB_ORIGIN};
                use bao1x_hal::{
                    board::SPINOR_BULK_ERASE_SIZE,
                    ifram::IframRange,
                    iox::Iox,
                    udma::{Spim, *},
                };
                let perclk = 100_000_000;
                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));
                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but
                        // run it as fast
                        // as we can run perclk
                        perclk / 4,
                        perclk / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        256 + 16, /* just enough space to send commands + programming
                                   * page */
                        4096,
                        Some(6),
                        Some(SpimMode::Standard), // guess Standard
                        IframRange::from_raw_parts(
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                flash_spim.identify_flash_reset_qpi();
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("flash ID (init): {:x}", flash_id);

                flash_spim.mem_qpi_mode(true);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);

                crate::println!("Erasing from {:x}-{:x}...", PDDB_ORIGIN, PDDB_ORIGIN + PDDB_LEN);
                for addr in (PDDB_ORIGIN..PDDB_ORIGIN + PDDB_LEN).step_by(SPINOR_BULK_ERASE_SIZE as usize) {
                    crate::println!("  {:x}...", addr);
                    flash_spim.flash_erase_block(addr, SPINOR_BULK_ERASE_SIZE as usize);
                }
                crate::println!("...done!");
            }
            #[cfg(feature = "board-baosec")]
            "erase_swap" => {
                use bao1x_hal::{
                    board::SPINOR_BULK_ERASE_SIZE,
                    ifram::IframRange,
                    iox::Iox,
                    udma::{Spim, *},
                };
                let perclk = 100_000_000;
                let udma_global = GlobalConfig::new();

                // setup the I/O pins
                let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                let channel = bao1x_hal::board::setup_memory_pins(&iox);
                udma_global.clock_on(PeriphId::from(channel));
                // safety: this is safe because clocks have been set up
                let mut flash_spim = unsafe {
                    Spim::new_with_ifram(
                        channel,
                        // has to be half the clock frequency reaching the block, but
                        // run it as fast
                        // as we can run perclk
                        perclk / 4,
                        perclk / 2,
                        SpimClkPol::LeadingEdgeRise,
                        SpimClkPha::CaptureOnLeading,
                        SpimCs::Cs0,
                        0,
                        0,
                        None,
                        256 + 16, /* just enough space to send commands + programming
                                   * page */
                        4096,
                        Some(6),
                        Some(SpimMode::Standard), // guess Standard
                        IframRange::from_raw_parts(
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                            4096 * 2,
                        ),
                    )
                };
                flash_spim.identify_flash_reset_qpi();
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("flash ID (init): {:x}", flash_id);

                flash_spim.mem_qpi_mode(true);
                let flash_id = flash_spim.mem_read_id_flash();
                crate::println!("QPI flash ID: {:x}", flash_id);

                crate::println!("Erasing from {:x}-{:x}...", 0, 2 * SPINOR_BULK_ERASE_SIZE);
                for addr in (0..2 * SPINOR_BULK_ERASE_SIZE).step_by(SPINOR_BULK_ERASE_SIZE as usize) {
                    crate::println!("  {:x}...", addr);
                    flash_spim.flash_erase_block(addr as usize, SPINOR_BULK_ERASE_SIZE as usize);
                }
                crate::println!("...done!");
            }
            "glue" => {
                let glue = CSR::new(utra::gluechain::HW_GLUECHAIN_BASE as *mut u32);
                let mut irq13 = CSR::new(utra::irqarray13::HW_IRQARRAY13_BASE as *mut u32);
                let mut irq15 = CSR::new(utra::irqarray15::HW_IRQARRAY15_BASE as *mut u32);
                irq15.wo(utra::irqarray15::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
                irq15.wo(utra::irqarray15::EV_POLARITY, 0xFFFF_FFFF);
                irq15.wo(utra::irqarray15::EV_PENDING, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_POLARITY, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_PENDING, 0xFFFF_FFFF);
                let mut last_irq15 = 1u32;
                let mut cur_irq15: u32;
                let mut last_irq13 = 1u32;
                let mut cur_irq13: u32;
                let mut last_state = [1u32; 8];
                let mut cur_state = [0u32; 8];
                let mut quit = false;
                let mut count = 0;
                unsafe {
                    glue.base().add(0).write_volatile(0x0);
                    glue.base().add(1).write_volatile(0x0);
                    glue.base().add(4).write_volatile(0xFFFF_FFFF);
                    glue.base().add(5).write_volatile(0xFFFF_FFFF);
                    glue.base().add(6).write_volatile(0x0);
                    glue.base().add(7).write_volatile(0x0);
                }
                loop {
                    for (i, dest) in cur_state.iter_mut().enumerate() {
                        *dest = unsafe { glue.base().add(i).read_volatile() };
                    }
                    if cur_state != last_state {
                        crate::println!("diff({:8}): {:08x?}", count, cur_state);
                        last_state = cur_state;
                    }
                    cur_irq13 = irq13.r(utra::irqarray13::EV_PENDING);
                    if cur_irq13 != last_irq13 {
                        crate::println!("irq13: {:x}", cur_irq13);
                        last_irq13 = cur_irq13;
                    }
                    cur_irq15 = irq15.r(utra::irqarray15::EV_PENDING);
                    if cur_irq15 != last_irq15 {
                        crate::println!("irq15: {:x}", cur_irq15);
                        last_irq15 = cur_irq15;
                    }
                    critical_section::with(|cs| {
                        if crate::USB_RX.borrow(cs).borrow().len() > 0
                            || crate::UART_RX.borrow(cs).borrow().len() > 0
                        {
                            quit = true;
                        }
                    });
                    if quit {
                        break;
                    }
                    count += 1;
                    crate::usb::flush();
                }
            }
            "sensor" => {
                let mut sensor = CSR::new(utra::sensorc::HW_SENSORC_BASE as *mut u32);
                let mut irq13 = CSR::new(utra::irqarray13::HW_IRQARRAY13_BASE as *mut u32);
                let mut irq15 = CSR::new(utra::irqarray15::HW_IRQARRAY15_BASE as *mut u32);
                irq15.wo(utra::irqarray15::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
                irq15.wo(utra::irqarray15::EV_POLARITY, 0xFFFF_FFFF);
                irq15.wo(utra::irqarray15::EV_PENDING, 0xFFFF_FFFF);
                irq15.wo(utra::irqarray15::EV_ENABLE, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_POLARITY, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_PENDING, 0xFFFF_FFFF);
                irq13.wo(utra::irqarray13::EV_ENABLE, 0xFFFF_FFFF);
                // glitch detect:
                // irq13 -> 8 -> secirq
                // irq15 -> 2 -> sensorc irq
                /*
                   vd_VD09_CFG => trim from 500, 550, 600, 650 mV VD09TL trigger
                    assign  { vd_VD09_CFG[1:0],VD09ENA,VD25ENA,VD33ENA } = sensor_vdena;
                    assign  { VD09TL,VD09TH,VD25TL,VD25TH,VD33TL,VD33TH } = sensor_vdtst ;
                    assign  sensor_vd = { VD09L,VD09H,VD25L,VD25H,VD33L,VD33H };
                */
                /* Results of laser glitch test:
                sensor
                diff: [0000, 003f, 0000, 0000, 0000, 0000, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                irq13: 0
                irq15: 0
                - glitch now -
                irq13: 8
                irq15: 2
                diff: [0000, 003f, 0000, 0000, 0000, 0002, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                diff: [0000, 003f, 0000, 0000, 0000, 0000, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                diff: [0000, 003f, 0000, 0000, 0000, 0002, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                                                        ^ this is the light sensor trigger bit
                diff: [0000, 003f, 000a, 000a, 0000, 0000, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                diff: [0000, 003f, 0002, 000a, 0000, 0000, 000c, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0000, 0007, 0000, 0000]
                                      ^ these are voltage glitches induced by light exposure
                */
                let mut last_irq15 = 1u32;
                let mut cur_irq15: u32;
                let mut last_irq13 = 1u32;
                let mut cur_irq13: u32;
                let mut last_state = [1u32; utra::sensorc::SENSORC_NUMREGS];
                let mut cur_state = [0u32; utra::sensorc::SENSORC_NUMREGS];
                let mut quit = false;
                sensor.wo(utra::sensorc::SFR_VDMASK0, 0);
                sensor.wo(utra::sensorc::SFR_VDMASK1, 0x3f); // when 0 it'll reset
                sensor.wo(utra::sensorc::SFR_LDIP_FD, 0x1ff); // setup filtering parameters
                sensor.wo(utra::sensorc::SFR_LDCFG, 0xc);
                sensor.wo(utra::sensorc::SFR_LDMASK, 0x0); // turn on both sensors
                loop {
                    for (i, dest) in cur_state.iter_mut().enumerate() {
                        *dest = unsafe { sensor.base().add(i).read_volatile() };
                    }
                    if cur_state != last_state {
                        crate::println!("diff: {:04x?}", cur_state);
                        last_state = cur_state;
                    }
                    cur_irq13 = irq13.r(utra::irqarray13::EV_PENDING);
                    if cur_irq13 != last_irq13 {
                        crate::println!("irq13: {:x}", cur_irq13);
                        last_irq13 = cur_irq13;
                    }
                    cur_irq15 = irq15.r(utra::irqarray15::EV_PENDING);
                    if cur_irq15 != last_irq15 {
                        crate::println!("irq15: {:x}", cur_irq15);
                        last_irq15 = cur_irq15;
                    }
                    critical_section::with(|cs| {
                        if crate::USB_RX.borrow(cs).borrow().len() > 0
                            || crate::UART_RX.borrow(cs).borrow().len() > 0
                        {
                            quit = true;
                        }
                    });
                    if quit {
                        break;
                    }
                    crate::usb::flush();
                }
            }
            "mesh" => {
                let mut mesh = CSR::new(utra::mesh::HW_MESH_BASE as *mut u32);
                let mut quit = false;
                mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE0, 0);
                mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE1, 0);
                mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV0, 0x5A5A_5A5A);
                mesh.wo(utra::mesh::SFR_MLDRV_CR_MLDRV1, 0xA5A5_A5A5);
                let mut last_state = [1u32; 8];
                let mut cur_state = [2u32; 8];
                for (i, state) in cur_state.iter_mut().enumerate() {
                    *state = unsafe { mesh.base().add(i + 8).read_volatile() }
                }
                let mut iters = 0;
                loop {
                    if cur_state != last_state {
                        crate::println!("diff: {:08x?}", cur_state);
                        last_state = cur_state;
                    }
                    for (i, state) in cur_state.iter_mut().enumerate() {
                        *state = unsafe { mesh.base().add(i + 8).read_volatile() }
                    }
                    critical_section::with(|cs| {
                        if crate::USB_RX.borrow(cs).borrow().len() > 0
                            || crate::UART_RX.borrow(cs).borrow().len() > 0
                        {
                            quit = true;
                        }
                    });
                    crate::platform::delay(1);
                    iters += 1;
                    if iters == 100 {
                        crate::println!("measure!");
                        mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE0, 0xffff_ffff);
                        mesh.wo(utra::mesh::SFR_MLIE_CR_MLIE1, 0xffff_ffff);
                    }
                    if quit {
                        break;
                    }
                    crate::usb::flush();
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
                #[cfg(feature = "bao1x")]
                crate::print!(", rram, clocks");
                #[cfg(not(feature = "bao1x"))]
                crate::print!(", mon");
                #[cfg(feature = "bao1x-bio")]
                crate::print!(", bio, bdma, pin");
                #[cfg(all(feature = "bao1x", feature = "board-baosec"))]
                crate::print!(", ldo, wfi, pdbberase, erase_swap");
                #[cfg(feature = "bao1x-usb")]
                crate::print!(", usb");
                #[cfg(feature = "bao1x-trng")]
                crate::print!(", trngro, trngav");
                #[cfg(feature = "dabao-selftest")]
                crate::print!(", dbtest");
                #[cfg(feature = "spim-tests")]
                crate::print!(", qe, check_qpi");
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
