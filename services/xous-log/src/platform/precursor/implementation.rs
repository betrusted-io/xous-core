use core::fmt::{Error, Write};

use utralib::generated::*;

pub struct Output {}

pub fn init() -> Output {
    if cfg!(feature = "logging") {
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::console::HW_CONSOLE_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map serial port");
        unsafe { crate::platform::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() as _ };
        println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
        let mut uart_csr = CSR::new(uart.as_mut_ptr() as *mut u32);

        println!("Process: map success!");

        let inject_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::keyinject::HW_KEYINJECT_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map keyinjection CSR range");
        println!("Note: character injection via console UART is enabled.");

        println!("Allocating IRQ...");
        xous::syscall::claim_interrupt(
            utra::console::CONSOLE_IRQ,
            handle_console_irq,
            inject_mem.as_mut_ptr() as *mut usize,
        )
        .expect("couldn't claim interrupt");
        println!("Claimed IRQ {}", utra::console::CONSOLE_IRQ);
        uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
    }
    Output {}
}

impl Output {
    pub fn get_writer(&self) -> OutputWriter { OutputWriter {} }

    pub fn run(&mut self) {
        loop {
            xous::wait_event();
        }
    }
}

fn handle_console_irq(_irq_no: usize, arg: *mut usize) {
    if cfg!(feature = "logging") {
        let mut inject_csr = CSR::new(arg as *mut u32);
        let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });
        // println!("rxe {}", uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY));
        while uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY) == 0 {
            // I really rather think this is more readable, than the "Rusty" version below.
            inject_csr.wfo(utra::keyinject::UART_CHAR_CHAR, uart_csr.rf(utra::uart::RXTX_RXTX));
            uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);

            // I guess this is how you would do it if you were "really doing Rust"
            // (except this is checking pending not fifo status for loop termination)
            // (which was really hard to figure out just looking at this loop)
            /*
            let maybe_c = match uart_csr.rf(utra::uart::EV_PENDING_RX) {
                0 => None,
                ack => {
                    let c = Some(uart_csr.rf(utra::uart::RXTX_RXTX) as u8);
                    uart_csr.wfo(utra::uart::EV_PENDING_RX, ack);
                    c
                }
            };
            if let Some(c) = maybe_c {
                inject_csr.wfo(utra::keyinject::UART_CHAR_CHAR, (c & 0xff) as u32);
            } else {
                break;
            }*/
        }
        // println!("rxe {}", uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY));
        // println!("pnd {}", uart_csr.rf(utra::uart::EV_PENDING_RX));
    }
}

pub struct OutputWriter {}

impl OutputWriter {
    pub fn putc(&self, c: u8) {
        if cfg!(feature = "logging") {
            let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });

            // Wait until TXFULL is `0`
            while uart_csr.r(utra::uart::TXFULL) != 0 {}
            uart_csr.wo(utra::uart::RXTX, c as u32);

            // there's a race condition in the handler, if a new character comes in while handling the
            // interrupt, the pending bit never clears. If the console seems to freeze, uncomment
            // this line. This kind of works around that, at the expense of maybe losing some Rx
            // characters. uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
        }
    }
}

impl Write for OutputWriter {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
            if c == '\n' as u8 {
                self.putc('\r' as u8);
            }
        }
        Ok(())
    }
}
