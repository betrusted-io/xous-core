use core::fmt::{Error, Write};

#[cfg(feature = "cramium-soc")]
use cramium_hal::udma;

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

#[cfg(feature = "cramium-fpga")]
impl Uart {
    pub fn putc(&self, c: u8) {
        assert!(unsafe { DEFAULT_UART_ADDR } as usize != 0);
        let mut uart_csr = utralib::CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        while uart_csr.r(utralib::utra::duart::SFR_SR) != 0 {}
        uart_csr.wo(utralib::utra::duart::SFR_TXD, c as u32);
    }
}

#[cfg(feature = "cramium-soc")]
impl Uart {
    pub fn putc(&self, c: u8) {
        // check that we've been initialized before attempting to send any characters...
        if unsafe { DEFAULT_UART_ADDR } as usize == 0 {
            return;
        }
        if unsafe { crate::implementation::UART_DMA_TX_BUF_VIRT } as usize == 0 {
            return;
        }
        // safety: safe to call as long as the raw parts are initialized and we exclusively
        // own it; and the UART has been initialized. For this peripheral, initialization
        // is handled by the loader and tossed to us, and exclusivity of access is something
        // we just have to not bungle.
        let mut uart = unsafe {
            udma::Uart::get_handle(
                crate::platform::debug::DEFAULT_UART_ADDR as usize,
                cramium_hal::board::UART_DMA_TX_BUF_PHYS,
                crate::implementation::UART_DMA_TX_BUF_VIRT as usize,
            )
        };

        // enqueue our character to send via DMA
        uart.write(&[c]);
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
