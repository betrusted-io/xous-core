use core::fmt::{Error, Write};

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
			use core::fmt::Write;
			let _ = write!(crate::debug::DEFAULT_UART, $($args)+);
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

pub struct Uart {
    pub base: *mut usize,
}

pub const DEFAULT_UART: Uart = Uart {
    base: 0xE000_1000 as *mut usize,
};

impl Uart {
    pub fn putc(&self, c: u8) {
        unsafe {
            // Wait until TXFULL is `0`
            while self.base.add(1).read_volatile() != 0 {
                ()
            }
            self.base.add(0).write_volatile(c as usize)
        };
    }

    pub fn enable_rx(self) {
        unsafe {
            self.base
                .add(5)
                .write_volatile(self.base.add(5).read_volatile() | 2)
        };
    }

    pub fn getc(&self) -> Option<u8> {
        unsafe {
            // If EV_PENDING_RX is 1, return the pending character.
            // Otherwise, return None.
            match self.base.add(4).read_volatile() & 2 {
                0 => None,
                ack => {
                    let c = Some(self.base.add(0).read_volatile() as u8);
                    self.base.add(4).write_volatile(ack);
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
