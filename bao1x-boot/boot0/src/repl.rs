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
            "check" => match bao1x_hal::sigcheck::validate_image(BOOT0_SELF_CHECK, None, None) {
                Ok(key_number, _key_number2, _id, _target) => {
                    crate::println!("sigcheck passed on key {}", key_number)
                }
                Err(e) => crate::println!("sigcheck failed: {}", e),
            },
            "reps" => {
                crate::println!("start test");
                // start the RTC
                unsafe { (0x4006100c as *mut u32).write_volatile(1) };
                let mut count = 0;
                let start_time = unsafe { (0x40061000 as *mut u32).read_volatile() };
                loop {
                    let new_time = unsafe { (0x40061000 as *mut u32).read_volatile() };
                    if new_time != start_time {
                        break;
                    }
                }
                let start_time = unsafe { (0x40061000 as *mut u32).read_volatile() };
                loop {
                    let new_time = unsafe { (0x40061000 as *mut u32).read_volatile() };
                    if new_time >= start_time + 5 {
                        break;
                    }
                    bao1x_hal::sigcheck::validate_image(BOOT0_SELF_CHECK, None, None).ok();
                    count += 1;
                }
                crate::println!("{} reps/sec", count / 5);
                crate::platform::setup_timer();
            }
            "boot1" => unsafe {
                crate::asm::jump_to(bao1x_api::BOOT1_START);
            },
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
