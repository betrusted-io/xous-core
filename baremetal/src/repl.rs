use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

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
                crate::println!("Command not recognized: '{}'", cmd);
                crate::println!("Type 'help' for a list of commands.");
            }
        }

        // reset for next loop
        self.do_cmd = false;
        self.cmdline.clear();
    }
}
