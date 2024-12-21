use core::fmt::{Error, Write};

pub struct DebugUart {}
impl DebugUart {
    #[cfg(all(feature = "debug-print-swapper", any(feature = "precursor", feature = "renode")))]
    pub fn putc(&mut self, c: u8) {
        use utralib::generated::*;
        let mut csr = CSR::new(loader::swap::SWAP_APP_UART_VADDR as *mut u32);

        // Wait until TXFULL is `0`
        while csr.r(utra::app_uart::TXFULL) != 0 {}
        csr.wfo(utra::app_uart::RXTX_RXTX, c as u32);
    }

    #[cfg(all(feature = "debug-print-swapper", any(feature = "cramium-soc")))]
    pub fn putc(&mut self, c: u8) {
        use cramium_hal::udma;
        // safety: safe to call as long as the raw parts are initialized and we exclusively
        // own it; and the UART has been initialized. For this peripheral, initialization
        // is handled by the loader and tossed to us, and exclusivity of access is something
        // we just have to not bungle.
        let mut uart = unsafe {
            udma::Uart::get_handle(
                loader::swap::SWAP_APP_UART_VADDR as usize,
                cramium_hal::board::APP_UART_IFRAM_ADDR as usize,
                loader::swap::SWAP_APP_UART_IFRAM_VADDR as usize,
            )
        };
        // enqueue our character to send via DMA
        uart.write(&[c]);
    }

    #[cfg(not(feature = "debug-print-swapper"))]
    pub fn putc(&self, _c: u8) {}
}

impl Write for DebugUart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}
