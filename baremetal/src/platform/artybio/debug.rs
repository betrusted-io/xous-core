use utralib::generated::*;

/// A trait for serial like drivers which allows reading from a source.
#[allow(dead_code)]
pub trait SerialRead {
    /// Read a single byte.
    fn getc(&mut self) -> Option<u8>;
}

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

    pub fn enable_rx(enable: bool) {
        let base = utra::uart::HW_UART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        if enable {
            uart.rmwf(utra::uart::EV_ENABLE_RX, 1);
        } else {
            uart.rmwf(utra::uart::EV_ENABLE_RX, 0);
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

impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> {
        let base = utra::uart::HW_UART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        match uart.rf(utra::uart::RXEMPTY_RXEMPTY) {
            1 => None,
            _ => {
                let ret = Some(uart.r(utra::uart::RXTX) as u8);
                uart.wfo(utra::uart::EV_PENDING_RX, 1);
                ret
            }
        }
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
