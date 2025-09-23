#[allow(unused_imports)]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[allow(unused_imports)]
use bao1x_api::*;
#[allow(unused_imports)]
use utralib::*;

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

// courtesy of claude
const K_DATA: &'static [u8; 853] = b"The quick brown fox jumps over the lazy dog while contemplating \
the meaning of existence in a digital world. Numbers like 123456789 and symbols @#$%^&*() add variety to this test \
message. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore \
magna aliqua. Testing patterns: ABCDEFGHIJKLMNOPQRSTUVWXYZ and abcdefghijklmnopqrstuvwxyz provide full alphabet coverage. \
Special characters !@#$%^&*()_+-=[]{}|;':.,.<>?/ enhance the diversity of this sample text. The year 2024 brings new \
challenges and opportunities for software development and testing methodologies. Random words like elephant, butterfly, \
quantum, nebula, crystalline, harmonic, and serendipity fill the remaining space. Pi equals 3.14159265358979323846 \
approximately. This text serves as a placeholder for various testing scenarios!!!";
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
            "reset" => {
                let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
                // rcurst.wo(utra::sysctrl::SFR_RCURST1, 0x55AA);
            }
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
            "echo" => {
                for word in args {
                    crate::print!("{} ", word);
                }
                crate::println!("");
            }
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
            _ => {
                crate::println!("Command not recognized: {}", cmd);
                crate::print!("Commands include: echo, bogomips, peek, poke, sha256check");
                crate::print!(", clocks");
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
