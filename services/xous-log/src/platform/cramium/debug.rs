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
        let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });
        // enqueue our character to send via DMA
        unsafe {
            if crate::implementation::UART_DMA_BUF as usize != 0 {
                crate::implementation::UART_DMA_BUF.write_volatile(c); // write to the virtual memory address
            }
        }
        // configure the DMA
        uart_csr.wo(utra::udma_uart_0::REG_TX_SADDR, utralib::HW_IFRAM0_MEM as u32); // source is the physical address
        uart_csr.wo(utra::udma_uart_0::REG_TX_SIZE, 1);
        // send it
        uart_csr.wo(utra::udma_uart_0::REG_TX_CFG, 0x10); // EN
        // wait for it all to be done
        while uart_csr.rf(utra::udma_uart_0::REG_TX_CFG_R_TX_EN) != 0 {   }
        while (uart_csr.r(utra::udma_uart_0::REG_STATUS) & 1) != 0 {  }
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
