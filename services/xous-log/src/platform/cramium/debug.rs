use core::fmt::{Error, Write};

use utralib::generated::*;

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
            use core::fmt::Write;
			let _ = write!(crate::platform::debug::Uart {}, $($args)+);
	});
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

pub struct Uart {}

// this is a hack to bypass an explicit initialization/allocation step for the debug structure
pub static mut DEFAULT_UART_ADDR: *mut usize = 0x0000_0000 as *mut usize;

#[cfg(feature="cramium-fpga")]
impl Uart {
    pub fn putc(&self, c: u8) {
        assert!(unsafe { DEFAULT_UART_ADDR } as usize != 0);
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        while uart_csr.r(utra::duart::SFR_SR) != 0 {}
        uart_csr.wo(utra::duart::SFR_TXD, c as u32);
    }
}

#[cfg(feature="cramium-soc")]
impl Uart {
    pub fn putc(&self, c: u8) {
        assert!(unsafe { DEFAULT_UART_ADDR } as usize != 0);
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        while uart_csr.r(utra::duart::SFR_SR) != 0 {}
        uart_csr.wo(utra::duart::SFR_TXD, c as u32);
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}
