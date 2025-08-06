use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

#[allow(unused_imports)]
use utralib::*;
#[cfg(feature = "artybio")]
use xous_bio_bdma::*;

use crate::arty_rgb;

// A server program that runs on a BIO core to handle GPIO control.
// It waits for a command from the main CPU via FIFO, executes it, and waits for the next.
#[cfg(feature = "artybio")]
#[rustfmt::skip]
bio_code!(pin_control_code, PIN_CONTROL_START, PIN_CONTROL_END,
    // Configure all GPIOs as outputs once at the start.
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
  "wait_for_cmd:",
    // Read the pin mask from FIFO0 (x16). The core will stall here until the CPU sends data.
    "mv    t1, x16",
    // Read the desired state (1 for high, 0 for low) from FIFO0. Stalls again.
    "mv    t2, x16",
    // Set the GPIO mask register (x26) to the pin mask we just received.
    "mv    x26, t1",
    // Set the GPIO output register (x21) to the state (high/low) we received.
    "mv    x21, t2",
    // Loop back to wait for the next command.
    "j     wait_for_cmd"
);

// A flag to ensure we only initialize the BIO core once.
static GPIO_CORE_INITIALIZED: AtomicBool = AtomicBool::new(false);

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
                let mut rgb = CSR::new(arty_rgb::HW_RGB_BASE as *mut u32);
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
                        rgb.wfo(arty_rgb::OUT_OUT, (count / TICKS_PER_PRINT) as u32);
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
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }

            "blinky" => {
                if args.len() != 2 {
                    crate::println!("Usage: blinky <LED_NAME> <RGB_HEX_VALUE>");
                    crate::println!("Example: blinky LD2 ff0000");
                    crate::println!("Available LEDs: LD0, LD1, LD2");
                    self.do_cmd = false;
                    self.cmdline.clear();
                    return;
                }

                let ld_name = &args[0];
                let hex_code = &args[1];

                let target_led_field = match ld_name.as_str() {
                    "LD0" => arty_rgb::LD0,
                    "LD1" => arty_rgb::LD1,
                    "LD2" => arty_rgb::LD2,
                    _ => {
                        crate::println!("Invalid LED name: {}. Use LD0, LD1, LD2.", ld_name);
                        self.cmdline.clear();
                        self.do_cmd = false;
                        return;
                    }
                };

                let color_val = if hex_code.starts_with("0x") {
                    u32::from_str_radix(&hex_code[2..], 16)
                } else {
                    u32::from_str_radix(hex_code, 16)
                };

                match color_val {
                    Ok(color) => {
                        // convert 24-bit RRGGBB into 3-bit BGR
                        let r_msb = color & 0x800000;
                        let g_msb = color & 0x008000;
                        let b_msb = color & 0x000080;
                        let bgr_val = (b_msb >> 6) | (g_msb >> 13) | (r_msb >> 23);

                        let mut rgb = CSR::new(arty_rgb::HW_RGB_BASE as *mut u32);

                        rgb.rmwf(target_led_field, bgr_val);

                        crate::println!(
                            "Set {} to BGR value 0b{:03b} (from hex {}).",
                            ld_name,
                            bgr_val,
                            hex_code
                        );
                    }
                    Err(_) => {
                        crate::println!("Invalid hex color value: {}", hex_code);
                    }
                }
            }
            "capsense" => {
                #[cfg(feature = "artybio")]
                {
                    if args.len() != 2 {
                        crate::println!("Usage: capsense <pin> <on|off>");
                        self.do_cmd = false;
                        self.cmdline.clear();
                        return;
                    }

                    let pin = match u32::from_str_radix(&args[0], 10) {
                        Ok(p) if p < 32 => p,
                        _ => {
                            crate::println!("Invalid pin number. Must be 0-31.");
                            self.do_cmd = false;
                            self.cmdline.clear();
                            return;
                        }
                    };

                    let state = args[1].as_str();
                    if state != "on" && state != "off" {
                        crate::println!("Invalid state. Use 'on' or 'off'.");
                        self.do_cmd = false;
                        self.cmdline.clear();
                        return;
                    }

                    let mut bio_ss = BioSharedState::new();

                    // On first run, load and start the GPIO control program on BIO Core 0.
                    if !GPIO_CORE_INITIALIZED.load(Ordering::Relaxed) {
                        crate::println!("Initializing GPIO control core (Core 0)...");
                        let mut ctrl = bio_ss.bio.r(utra::bio_bdma::SFR_CTRL);
                        ctrl &= !0b0001; // Stop Core 0
                        bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl);

                        let prog = pin_control_code();
                        bio_ss.load_code(prog, 0, BioCore::Core0);
                        ctrl |= 0b0001_0001_0001;
                        bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl);
                        GPIO_CORE_INITIALIZED.store(true, Ordering::Relaxed);
                        crate::println!("GPIO control core is running.");
                    }

                    // Prepare the command for the BIO core.
                    let pin_mask = 1 << pin;
                    let state_val = if state == "on" { 1 } else { 0 };

                    // Send the command words to the BIO core via its FIFO.
                    // The core is waiting to read these two values.
                    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, pin_mask);
                    bio_ss.bio.wo(utra::bio_bdma::SFR_TXF0, state_val);

                    crate::println!("Command sent: Pin {} -> {}.", pin, state.to_uppercase());
                }
                #[cfg(not(feature = "artybio"))]
                {
                    crate::println!("'capsense' command requires 'artybio' feature.");
                }
            }
            "help" => {
                crate::println!("Available commands:");
                crate::println!("  help                    - Shows this help message.");
                crate::println!("  echo [ARGS]             - Prints the arguments to the console.");
                crate::println!(
                    "  mon                     - Monitors the program counters of the BIO cores."
                );
                crate::println!("  blinky <LD> <RGB_HEX>   - Sets an LED to a color.");
                crate::println!("    LD: LD0, LD1, or LD2");
                crate::println!("    RGB_HEX: e.g., ff0000 (red), 00ff00 (green), 0000ff (blue)");
                crate::println!("  capsense <pin> <on|off> - Sets a GPIO pin high or low.");
            }

            "" => {}

            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!("Commands include: echo, poke, peek, bogomips");
                #[cfg(feature = "cramium-soc")]
                crate::print!(", rram");
                #[cfg(not(feature = "cramium-soc"))]
                crate::print!(", mon");
                crate::println!("");
            }
        }

        // reset for next loop
        self.do_cmd = false;
        self.cmdline.clear();
    }
}
