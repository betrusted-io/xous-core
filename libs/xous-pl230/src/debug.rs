use utralib::generated::*;

pub struct Uart {
    // pub base: *mut u32,
}

#[allow(dead_code)]
impl Uart {
    fn put_digit(&mut self, d: u8) {
        let nyb = d & 0xF;
        let c = if nyb < 10 { nyb + 0x30 } else { nyb + 0x61 - 10 };
        assert!(c >= 0x30, "conversion failed!");
        self.putc(c);
    }

    pub fn put_hex(&mut self, c: u8) {
        self.put_digit(c >> 4);
        self.put_digit(c & 0xF);
    }

    pub fn newline(&mut self) {
        self.putc(0xa);
        self.putc(0xd);
    }

    pub fn print_hex_word(&mut self, word: u32) {
        for &byte in word.to_be_bytes().iter() {
            self.put_hex(byte);
        }
    }

    pub fn putc(&self, c: u8) {
        let base = utra::duart::HW_DUART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        while uart.r(utra::duart::SFR_SR) != 0 {}
        uart.wo(utra::duart::SFR_TXD, c as u32);
    }

    pub fn tiny_write_str(&mut self, s: &str) {
        for c in s.bytes() {
            self.putc(c);
        }
    }
}

use core::fmt::{Error, Write};
impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

#[macro_use]
#[cfg(all(not(test), feature = "debug-print"))]
pub mod debug_print_hardware {
    #[macro_export]
    macro_rules! print
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!(crate::debug::Uart {}, $($args)+);
        });
    }
}

#[macro_use]
#[cfg(all(not(test), not(feature = "debug-print")))]
mod debug_print_hardware {
    #[macro_export]
    #[allow(unused_variables)]
    macro_rules! print {
        ($($args:tt)+) => {{ () }};
    }
}

#[macro_use]
#[cfg(test)]
mod debug_print_hardware {
    #[macro_export]
    #[allow(unused_variables)]
    macro_rules! print {
        ($($args:tt)+) => ({
            std::print!($($args)+)
        });
    }
}

#[macro_export]
macro_rules! println
{
    () => ({
        $crate::print!("\r\n")
    });
    ($fmt:expr) => ({
        $crate::print!(concat!($fmt, "\r\n"))
    });
    ($fmt:expr, $($args:tt)+) => ({
        $crate::print!(concat!($fmt, "\r\n"), $($args)+)
    });
}
