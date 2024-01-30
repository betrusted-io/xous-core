use core::fmt::{Error, Write};

use crate::platform::console::ConsoleSingleton;

pub struct Output {}

pub fn init() -> Output {
    if cfg!(feature = "logging") {
        // println!("[xous-log::implementation] Installing custom panic hook...");
        // std::panic::set_hook(Box::new(|info| {
        //     println!("[!] [xous-log panic] {}", info);
        //     unsafe { core::arch::asm!("bkpt"); }
        // }));
        // println!("[xous-log::implementation] Panic hook installed");
    }

    Output {}
}

impl Output {
    pub fn get_writer(&self) -> OutputWriter { OutputWriter {} }

    pub fn run(&mut self) {
        loop {
            xous::wait_event();
        }
    }
}

#[allow(dead_code)]
fn handle_console_irq(_irq_no: usize, _arg: *mut usize) {
    if cfg!(feature = "logging") {
        // let mut inject_csr = CSR::new(arg as *mut u32);
        //let mut uart_csr = crate::platform::debug::UartType::with_alt_base_addr(unsafe {
        // crate::platform::debug::DEFAULT_UART_ADDR as u32 }); TODO:
    }
}

pub struct OutputWriter {}

impl OutputWriter {
    pub fn putc(&self, c: u8) {
        if cfg!(feature = "logging") {
            write!(ConsoleSingleton {}, "{}", c as char).ok();
        }
    }

    /// Write a buffer to the output and return the number of
    /// bytes written. This is mostly compatible with `std::io::Write`,
    /// except it is infallible.
    pub fn write(&mut self, buf: &[u8]) -> usize {
        for c in buf {
            self.putc(*c);
        }
        buf.len()
    }

    pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> { Ok(self.write(buf)) }
}

impl Write for OutputWriter {
    fn write_str(&mut self, s: &str) -> Result<(), Error> { ConsoleSingleton {}.write_str(s) }
}
