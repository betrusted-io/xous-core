use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[allow(unused_imports)]
use utralib::*;
#[cfg(feature = "artybio")]
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
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }
            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!("Commands include: echo, poke, peek");
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
