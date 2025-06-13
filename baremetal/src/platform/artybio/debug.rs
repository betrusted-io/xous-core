use utralib::generated::*;
pub struct Uart {
    // pub base: *mut u32,
}

impl Uart {
    pub fn putc(&self, c: u8) {
        let base = utra::uart::HW_UART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        // Wait until TXFULL is `0`
        while uart.r(utra::uart::TXFULL) != 0 {}
        uart.wo(utra::uart::RXTX, c as u32)
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
