use core::fmt::{Error, Write};

// manually sync'd with constant in loader/src/swap.rs, which is where the App UART
// is initially setup because a swapper can't set up its UART any other way (it needs
// to have its v-pages mapped by the loader as it is possible for the first v-page
// mapping to page fault into a swapped region)
pub const SWAP_APP_UART_VADDR: usize = 0xE180_0000;
pub const SWAP_APP_UART_IFRAM_VADDR: usize = 0xE180_1000;

pub struct DebugUart {}
impl DebugUart {
    #[cfg(feature = "debug-print")]
    pub fn putc(&mut self, c: u8) {
        use cramium_hal::udma;
        // safety: safe to call as long as the raw parts are initialized and we exclusively
        // own it; and the UART has been initialized. For this peripheral, initialization
        // is handled by the loader and tossed to us, and exclusivity of access is something
        // we just have to not bungle.
        let mut uart = unsafe {
            udma::Uart::get_handle(
                SWAP_APP_UART_VADDR as usize,
                cramium_hal::board::APP_UART_IFRAM_ADDR as usize,
                SWAP_APP_UART_IFRAM_VADDR as usize,
            )
        };
        // enqueue our character to send via DMA
        uart.write(&[c]);
    }

    // when debug-print is off, the print call code still exist, but the output step is skipped
    #[cfg(not(feature = "debug-print"))]
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

#[macro_use]
pub mod debug_print_hardware {
    #[macro_export]
    macro_rules! print
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!(crate::debug::DebugUart {}, $($args)+);
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
