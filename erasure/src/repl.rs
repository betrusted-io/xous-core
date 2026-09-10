use crate::SerialInteract;
use crate::erase::Erasure;
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

pub struct Error {
    pub message: Option<&'static str>,
}
impl Error {
    pub fn none() -> Self {
        Self { message: None }
    }

    pub fn help(message: &'static str) -> Self {
        Self { message: Some(message) }
    }
}

pub struct Repl {
    cmdline: String,
    do_cmd: bool,
    erasure: Erasure,
    rx_bin: usize,
    bin_data: Vec<u8>,
    bin_write_count: usize,
}

/// Number of bytes to write at a time when accepting data in binary mode.
const BIN_DATA_WRITE_INTERVAL: usize = 32;

/// Default offset from BAREMETAL_START to start erasing RRAM from.
const DEFAULT_BAREMETAL_RRAM_OFFSET: u32 = 102400;

const COLUMNS: usize = 4;
impl Repl {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            cmdline: String::new(),
            do_cmd: false,
            erasure: Erasure::new(DEFAULT_BAREMETAL_RRAM_OFFSET).unwrap(),
            rx_bin: 0,
            bin_data: Vec::new(),
            bin_write_count: 0,
        }
    }

    #[allow(dead_code)]
    pub fn init_cmd(&mut self, cmd: &str) {
        self.cmdline.push_str(cmd);
        self.cmdline.push('\n');
        self.do_cmd = true;
    }

    fn try_process(&mut self) -> Result<(), Error> {
        if !self.do_cmd {
            return Err(Error::none());
        }
        // crate::println!("got {}", self.cmdline);

        let mut parts = self.cmdline.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
        match cmd.as_str() {
            "peek" => {
                // Without this, we sometimes see unsuccessful writes as successful.
                bao1x_hal::cache_flush();

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
            "erase" => {
                if args.len() >= 1 {
                    match args[0].as_str() {
                        "len" => {
                            crate::println!("{:?} bytes remaining", self.erasure.len());
                        }
                        "restart" => {
                            if args.len() == 1 {
                                self.erasure = Erasure::new(DEFAULT_BAREMETAL_RRAM_OFFSET)
                                    .map_err(|_| Error::help("Problem initializing erasure"))?;
                            } else if args.len() == 2 {
                                let offset = u32::from_str_radix(&args[1], 16)
                                    .map_err(|_| Error::help("Offset must be in hex and fit in u32"))?;
                                self.erasure = Erasure::new(offset)
                                    .map_err(|_| Error::help("Problem initializing erasure"))?;
                            } else {
                                return Err(Error::help(
                                    "Help: erase restart [<rram_offset>], offset optional and in hex",
                                ));
                            }
                        }
                        "write-bin" => {
                            if args.len() != 2 {
                                return Err(Error::help(
                                    "Help: erase write-bin <count>, count is in decimal",
                                ));
                            }
                            let count = usize::from_str_radix(&args[1], 10)
                                .map_err(|_| Error::help("Count must be a decimal integer"))?;
                            self.rx_bin = count;
                            self.bin_data = Vec::with_capacity(BIN_DATA_WRITE_INTERVAL);
                        }
                        "write" => {
                            let hex_str = &args[1];
                            let addr = self.erasure.peek();
                            if args.len() != 2 {
                                return Err(Error::help("Help: erase write <value>, value is in hex"));
                            }
                            let mut data: Vec<u8> = Vec::new();
                            for i in 0..(hex_str.len() / 2) {
                                let value = u8::from_str_radix(&args[1][2 * i..(i + 1) * 2], 16)
                                    .map_err(|_| Error::help("Value is in hex, no leading 0x"))?;
                                data.push(value);
                            }
                            self.erasure.write_slice(data.as_slice());
                            crate::println!(
                                "wrote {:?} bytes starting at {:x} and ending at {:x}",
                                data.len(),
                                addr,
                                self.erasure.peek()
                            );
                        }
                        "key" => {
                            if args.len() != 3 {
                                return Err(Error::help(
                                    "Help: erase key <seed> <keyblock>, seed and keyblock in hex",
                                ));
                            }
                            let mut seed: Vec<u8> = Vec::new();
                            for i in 0..(args[1].len() / 2) {
                                let b = u8::from_str_radix(&args[1][i * 2..(i + 1) * 2], 16)
                                    .map_err(|_| Error::help("Value is in hex, no leading 0x"))?;
                                seed.push(b);
                            }
                            let mut key_block: Vec<u8> = Vec::new();
                            for i in 0..(args[2].len() / 2) {
                                let b = u8::from_str_radix(&args[2][i * 2..(i + 1) * 2], 16)
                                    .map_err(|_| Error::help("Value is in hex, no leading 0x"))?;
                                key_block.push(b);
                            }
                            if seed.len() != 16 || key_block.len() != 16 {
                                return Err(Error::help(
                                    "Help: erase key <seed> <keyblock>, seed and keyblock must be exactly 16 bytes each",
                                ));
                            }
                            let key: [u8; 16] = self
                                .erasure
                                .recover_key(seed.as_slice(), key_block.as_slice())
                                .map_err(|_| Error::help("Problem recovering key!"))?;
                            crate::println!("key = {:02x?}", key);
                        }
                        _ => {
                            return Err(Error::help("Help: erase {len, restart, write-bin, write, key}"));
                        }
                    }
                } else {
                    return Err(Error::help("Help: erase {len, restart, write-bin, write, key}"));
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
                crate::println!("Commands include: echo, peek, erase");
            }
        }

        // reset for next loop
        self.abort_cmd();
        Ok(())
    }

    fn abort_cmd(&mut self) {
        self.do_cmd = false;
        self.cmdline.clear();
    }
}

impl SerialInteract for Repl {
    fn rx_char(&mut self, c: u8) {
        // If we are receiving data in binary mode, copy it to the buffer instead of cmdline.
        if self.rx_bin > 0 {
            self.bin_data.push(c);
            self.rx_bin -= 1;
            if self.bin_data.len() >= BIN_DATA_WRITE_INTERVAL {
                self.erasure.write_slice(&self.bin_data);
                self.bin_write_count += self.bin_data.len();
                self.bin_data.clear();
            }
            if self.rx_bin == 0 {
                self.erasure.write_slice(self.bin_data.as_slice());
                self.bin_write_count += self.bin_data.len();
                crate::println!("wrote {:?} bytes ending at {:x}", self.bin_write_count, self.erasure.peek());
                self.bin_data.clear();
                self.bin_write_count = 0;
            }
            return;
        }
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

    fn process(&mut self) {
        match self.try_process() {
            Err(e) => {
                if let Some(m) = e.message {
                    crate::println!("{}", m);
                    self.abort_cmd();
                }
            }
            _ => (),
        }
    }
}
