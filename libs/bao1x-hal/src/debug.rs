#[allow(unused_imports)]
use utralib::generated::*;
pub struct Uart {}

#[allow(dead_code)]
pub static mut SWAP_APP_UART_VADDR: usize = 0;
#[allow(dead_code)]
pub static mut SWAP_APP_UART_IFRAM_VADDR: usize = 0;

impl Uart {
    #[cfg(all(feature = "debug-print-usb", not(target_os = "xous")))]
    pub fn putc(&self, c: u8) {
        let base = utra::duart::HW_DUART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        while uart.r(utra::duart::SFR_SR) != 0 {}
        uart.wo(utra::duart::SFR_TXD, c as u32);
    }

    #[cfg(all(feature = "debug-print-usb", target_os = "xous"))]
    pub fn putc(&mut self, c: u8) {
        if unsafe { SWAP_APP_UART_VADDR } == 0 {
            match xous::syscall::map_memory(
                xous::MemoryAddress::new(utralib::utra::udma_uart_0::HW_UDMA_UART_0_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ) {
                Ok(uart) => {
                    unsafe { SWAP_APP_UART_VADDR = uart.as_mut_ptr() as usize };
                }
                _ => return,
            }
        }
        if unsafe { SWAP_APP_UART_IFRAM_VADDR } == 0 {
            match xous::syscall::map_memory(
                xous::MemoryAddress::new(crate::board::APP_UART_IFRAM_ADDR),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ) {
                Ok(mem) => {
                    unsafe { SWAP_APP_UART_IFRAM_VADDR = mem.as_mut_ptr() as usize };
                }
                _ => return,
            }
        }
        use crate::udma;
        // safety: safe to call as long as the raw parts are initialized and we exclusively
        // own it; and the UART has been initialized. For this peripheral, initialization
        // is handled by the loader and tossed to us, and exclusivity of access is something
        // we just have to not bungle.
        let mut uart = unsafe {
            udma::Uart::get_handle(
                SWAP_APP_UART_VADDR as usize,
                crate::board::APP_UART_IFRAM_ADDR as usize,
                SWAP_APP_UART_IFRAM_VADDR as usize,
            )
        };
        // enqueue our character to send via DMA
        uart.write(&[c]);
    }

    #[cfg(not(feature = "debug-print-usb"))]
    pub fn putc(&mut self, _c: u8) {}
}

use core::fmt::{Error, Write};
impl Write for Uart {
    fn write_str(&mut self, _s: &str) -> Result<(), Error> {
        #[cfg(not(all(feature = "std", not(feature = "debug-print-usb"))))]
        for c in _s.bytes() {
            self.putc(c);
        }
        // #[cfg(all(feature = "std", not(feature = "debug-print-usb")))]
        // log::info!("{}", _s);
        Ok(())
    }
}

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
