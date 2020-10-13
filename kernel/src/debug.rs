use core::fmt::{Error, Write};
use utralib::generated::*;

#[macro_use]
#[cfg(all(not(test), any(feature = "debug-print", feature = "print-panics")))]
pub mod debug_print_hardware {
    use crate::debug::*;
    pub const SUPERVISOR_UART: Uart = Uart {
        base: 0xffcf_0000 as *mut usize,
    };

    #[macro_export]
    macro_rules! print
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!(crate::debug::debug_print_hardware::SUPERVISOR_UART, $($args)+);
        });
    }
}
#[cfg(all(not(test), any(feature = "debug-print", feature = "print-panics")))]
pub use crate::debug::debug_print_hardware::SUPERVISOR_UART;

#[cfg(all(not(test), not(any(feature = "debug-print", feature = "print-panics"))))]
#[macro_export]
macro_rules! print {
    ($($args:tt)+) => {{
        ()
    }};
}

#[macro_export]
macro_rules! println
{
	() => ({
		print!("\r\n")
	});
	($fmt:expr) => ({
		print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!($fmt, "\r\n"), $($args)+)
	});
}

pub struct Uart {
    pub base: *mut usize,
}

impl Uart {
    #[allow(dead_code)]
    pub fn enable_rx(self) {
        let mut uart_csr = CSR::new(self.base as *mut u32);
        uart_csr.wfo(utra::uart::EV_ENABLE_ENABLE, uart_csr.rf(utra::uart::EV_ENABLE_ENABLE) | 2);
    }

    pub fn putc(&self, c: u8) {
        let mut uart_csr = CSR::new(self.base as *mut u32);
        // Wait until TXFULL is `0`
        while uart_csr.r(utra::uart::TXFULL) != 0 {
            ()
        }
        uart_csr.wfo(utra::uart::RXTX_RXTX, c as u32);
    }

    #[allow(dead_code)]
    pub fn getc(&self) -> Option<u8> {
        let mut uart_csr = CSR::new(self.base as *mut u32);
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        match uart_csr.r(utra::uart::EV_PENDING) & 2 {
            0 => None,
            ack => {
                let c = Some(uart_csr.r(utra::uart::RXTX) as u8);
                uart_csr.wo(utra::uart::EV_PENDING, ack);
                c
            }
        }
    }
}

#[cfg(all(not(test), any(feature = "debug-print", feature = "print-panics")))]
pub fn irq(_irq_number: usize, _arg: *mut usize) {
    println!(
        "Interrupt {}: Key pressed: {}",
        _irq_number,
        SUPERVISOR_UART
            .getc()
            .expect("no character queued despite interrupt") as char
    );
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}
