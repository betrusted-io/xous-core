use core::fmt::{Error, Write};

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
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        while uart_csr.r(utra::duart::SFR_SR) != 0 {}
        uart_csr.wo(utra::duart::SFR_TXD, c as u32);
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
        // safety: safe to call as long as DEFAULT_UART_ADDR is initialized and we exclusively
        // own it; and the UART has been initialized. For this peripheral, initialization
        // is handled by the loader and tossed to us.
        let mut uart = unsafe { udma::Uart::get_handle(DEFAULT_UART_ADDR as usize) };

        // enqueue our character to send via DMA
        let tx_buf_virt = unsafe {
            // safety: it's safe only because we are manually tracking the allocations in IFRAM0.
            core::slice::from_raw_parts_mut(crate::implementation::UART_DMA_TX_BUF_VIRT as *mut u8, 1)
        };
        let tx_buf_phys = unsafe {
            // safety: it's safe only because we are manually tracking the allocations in IFRAM0.
            core::slice::from_raw_parts_mut(crate::implementation::UART_DMA_TX_BUF_PHYS as *mut u8, 1)
        };
        tx_buf_virt[0] = c;
        // note that write takes *physical* addresses
        unsafe {
            uart.write_phys(tx_buf_phys);
        }
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
