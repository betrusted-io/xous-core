use core::fmt::{Error, Write};

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
			use core::fmt::Write;
			let _ = write!(crate::debug::DEFAULT, $($args)+);
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
    print!("Handling IRQ {} (arg: {:08x}): ", irq_no, arg as usize);

    while let Some(c) = crate::debug::DEFAULT.getc() {
        print!("0x{:02x}", c);
    }
    println!();
}

pub struct Uart {}

pub static mut DEFAULT_UART_ADDR: *mut usize = 0x0000_0000 as *mut usize;

pub const DEFAULT: Uart = Uart {};

impl Uart {
    pub fn putc(&self, c: u8) {
        unsafe {
            if DEFAULT_UART_ADDR as usize == 0 {
                let uart = xous::syscall::map_memory(
                    xous::MemoryAddress::new(0xf000_1000),
                    None,
                    4096,
                    xous::MemoryFlags::R | xous::MemoryFlags::W,
                )
                .expect("couldn't map uart");
                DEFAULT_UART_ADDR = uart.as_mut_ptr() as _;
                println!("Mapped UART @ {:08x}", uart.addr.get());
                // core::mem::forget(uart);

                println!("Allocating IRQ...");
                xous::claim_interrupt(2, handle_irq, core::ptr::null_mut::<usize>()).expect("unable to allocate IRQ");
                self.enable_rx();
            }
            let base = DEFAULT_UART_ADDR;

            // Wait until TXFULL is `0`
            while base.add(1).read_volatile() != 0 {}
            base.add(0).write_volatile(c as usize)
        };
    }

    pub fn enable_rx(&self) {
        unsafe {
            let base = DEFAULT_UART_ADDR;
            base.add(5).write_volatile(base.add(5).read_volatile() | 2)
        };
    }

    pub fn getc(&self) -> Option<u8> {
        unsafe {
            let base = DEFAULT_UART_ADDR;
            // If EV_PENDING_RX is 1, return the pending character.
            // Otherwise, return None.
            match base.add(4).read_volatile() & 2 {
                0 => None,
                ack => {
                    let c = Some(base.add(0).read_volatile() as u8);
                    base.add(4).write_volatile(ack);
                    c
                }
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
