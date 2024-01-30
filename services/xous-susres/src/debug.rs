use core::fmt::Error;
use core::fmt::Write;

use utralib::generated::*;
pub struct Uart {}

// this is a hack to bypass an explicit initialization/allocation step for the debug structure
pub static mut DEFAULT_UART_ADDR: *mut usize = 0x0000_0000 as *mut usize;

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
            use core::fmt::Write;
			let _ = write!(crate::debug::Uart {}, $($args)+);
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

fn handle_irq(irq_no: usize, arg: *mut usize) {
    print!("Susres IRQ {} ({}): ", irq_no, arg as usize);

    while let Some(c) = { crate::debug::Uart {} }.getc() {
        print!("{}", c as char);
    }
    println!();
}

impl Uart {
    fn map_uart(&mut self) {
        /*
           Note: the memory address and interrupt specified here needs to map to a unique hardware
           UART resource. Modify in this function as necessary.
        */
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::app_uart::HW_APP_UART_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map debug uart");
        unsafe {
            DEFAULT_UART_ADDR = uart.as_mut_ptr() as _;
        }
        println!("Mapped UART @ {:x?}", uart.as_ptr());

        println!("Allocating IRQ...");
        xous::claim_interrupt(utra::app_uart::APP_UART_IRQ, handle_irq, core::ptr::null_mut::<usize>())
            .expect("unable to allocate IRQ");
        self.enable_rx();
    }

    pub fn putc(&mut self, c: u8) {
        if unsafe { DEFAULT_UART_ADDR } as usize == 0 {
            self.map_uart();
        }
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        // Wait until TXFULL is `0`
        while uart_csr.r(utra::uart::TXFULL) != 0 {}
        uart_csr.wo(utra::uart::RXTX, c as u32);
    }

    pub fn enable_rx(&mut self) {
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
    }

    pub fn getc(&mut self) -> Option<u8> {
        if unsafe { DEFAULT_UART_ADDR } as usize == 0 {
            self.map_uart();
        }
        let mut uart_csr = CSR::new(unsafe { DEFAULT_UART_ADDR as *mut u32 });

        match uart_csr.rf(utra::uart::EV_PENDING_RX) {
            0 => None,
            ack => {
                let c = Some(uart_csr.rf(utra::uart::RXTX_RXTX) as u8);
                uart_csr.wfo(utra::uart::EV_PENDING_RX, ack);
                c
            }
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
