use core::fmt::{Error, Write};

use utralib::*;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Take the page lent to us, encrypt it and write it to swap
    WriteToSwap = 0,
    /// Find the requested page, decrypt it, and return it
    ReadFromSwap = 1,
    /// Kernel message advising us that a page of RAM was allocated
    AllocateAdvisory = 2,
    /// Kernel message requesting N pages to be swapped out.
    Trim = 3,
    /// Kernel message informing us that we have pages to free.
    Free = 4,
}

pub struct DebugUart {
    #[cfg(feature = "debug-print")]
    csr: CSR<u32>,
}
impl DebugUart {
    #[cfg(feature = "debug-print")]
    pub fn new() -> Self {
        let debug_uart_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::app_uart::HW_APP_UART_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't claim the debug UART");
        let debug_uart = CSR::new(debug_uart_mem.as_mut_ptr() as *mut u32);

        Self { csr: debug_uart }
    }

    #[cfg(feature = "debug-print")]
    pub fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.csr.r(utra::app_uart::TXFULL) != 0 {}
        self.csr.wfo(utra::app_uart::RXTX_RXTX, c as u32);
    }

    #[cfg(not(feature = "debug-print"))]
    pub fn new() -> Self { Self {} }

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

fn main() {
    // init the log, but this is mostly unused.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // swapper is not allowed to use `log` for debugging under most circumstances, because
    // the swapper can't send messages when handling a swap call. Instead, we use a local
    // debug UART to handle this. This needs to be enabled with the "debug-print" feature
    // and is mutually exclusive with the "gdb-stub" feature in the kernel since it uses
    // the same physical hardware.
    let mut duart = DebugUart::new();
    write!(duart, "Swapper started.\n\r");

    let sid = xous::create_server().unwrap();
    loop {
        let msg = xous::receive_message(shch_sid).unwrap();
        let op: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        write!(duart, "Swapper got {:?}", msg);
        match op {
            Some(Opcode::WriteToSwap) => {
                unimplemented!();
            }
            // ... todo, other opcodes.
            _ => {
                write!(duart, "Unknown opcode {:?}", op);
            }
        }
    }
}
