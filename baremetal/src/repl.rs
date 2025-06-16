use alloc::string::{String, ToString};
use alloc::vec::Vec;

use utralib::*;
use xous_bio_bdma::*;

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

                // Determine shift from LED name based on the hardware layout.
                let shift = match ld_name.as_str() {
                    "LD0" => 0,
                    "LD1" => 3,
                    "LD2" => 6,
                    _ => {
                        crate::println!("Invalid LD name: {}. Use LD0, LD1, or LD2", ld_name);
                        self.do_cmd = false;
                        self.cmdline.clear();
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

                        let mut rgb = CSR::new(utra::rgb::HW_RGB_BASE as *mut u32);
                        let new_state = bgr_val << shift;

                        rgb.rmwf(utra::rgb::OUT_OUT, rgb.r(utra::rgb::OUT) | new_state);

                        crate::println!(
                            "Set {} to BGR value 0b{:03b} (from hex {}). Other LEDs are off.",
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
            "help" => {
                crate::println!("Available commands:");
                crate::println!("  help                    - Shows this help message.");
                crate::println!("  echo [ARGS]             - Prints the arguments to the console.");
                crate::println!(
                    "  mon                     - Monitors the program counters of the BIO cores."
                );
                crate::println!("  blinky <LD> <RGB_HEX>   - Sets an LED to a color (turns others off).");
                crate::println!("    LD: LD1, LD2, or LD3");
                crate::println!("    RGB_HEX: e.g., ff0000 (red), 00ff00 (green), 0000ff (blue)");
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
