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

pub const DEFAULT: Uart = Uart {};

impl Uart {
    fn map_uart(&self) {
        /*
           Note: the memory address and interrupt specified here needs to map to a unique hardware
           UART resource. Modify in this function as necessary.
        */
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::console::HW_CONSOLE_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map debug uart");
        unsafe {
            DEFAULT_UART_ADDR = uart.as_mut_ptr() as _;
        }
        println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
    }

    pub fn putc(&self, c: u8) {
        if unsafe { DEFAULT_UART_ADDR } as usize == 0 {
            self.map_uart();
        }
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        // Wait until TXFULL is `0`
        while uart_csr.r(utra::uart::TXFULL) != 0 {}
        uart_csr.wo(utra::uart::RXTX, c as u32);
    }
    /// This is an abstraction violation, but this call is actually relied upon by the
    /// non-debug path to re-enable UART RX during a resume. This should be cleaned up.
    pub fn enable_rx(&self) {
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });
        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
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
