use core::fmt::{Error, Write};
use utralib::generated::*;

pub struct Output {}

#[cfg(feature="cramium-soc")]
pub static mut UART_DMA_BUF: *mut u8 = 0x0000_0000 as *mut u8;

pub fn init() -> Output {
    #[cfg(feature="cramium-fpga")]
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::duart::HW_DUART_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    // TODO: migrate this to a "proper" UART that is available on SoC hardware, but for now all we have access to is the DUART.
    #[cfg(feature="cramium-soc")]
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::udma_uart_0::HW_UDMA_UART_0_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    unsafe { crate::platform::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() as _ };
    println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
    println!("Process: map success!");

    #[cfg(feature="cramium-soc")]
    {
        // TODO: need to write an allocator for the UDMA memory region as well
        let tx_buf = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_IFRAM0_MEM),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map UDMA buffer");
        unsafe { UART_DMA_BUF = tx_buf.as_mut_ptr() as *mut u8 };

        // TODO: migrate this to an allocator that can handle all IOs
        let iox_buf = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::iox::HW_IOX_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map iox buffer");

        let udma_ctrl = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map UDMA control");

        let iox_csr = iox_buf.as_mut_ptr() as *mut u32;
        unsafe {
            iox_csr.add(0).write_volatile(0x0140);  // PAL
            iox_csr.add(0x1c / core::mem::size_of::<u32>()).write_volatile(0x1400); // PDH
            iox_csr.add(0x148 / core::mem::size_of::<u32>()).write_volatile(0xff); // PA
            iox_csr.add(0x148 / core::mem::size_of::<u32>() + 3).write_volatile(0xffff); // PD
            iox_csr.add(0x160 / core::mem::size_of::<u32>()).write_volatile(0xffff); // pullups for port A
        }

        let mut udma_ctrl = CSR::new(udma_ctrl.as_mut_ptr() as _);
        // ungate the clock for the UART. TODO: send this to a power management common server...
        // probably should be in the same server that allocates UDMA buffers.
        udma_ctrl.wo(utra::udma_ctrl::REG_CG, 1);

        // setup the baud rate
        let mut uart_csr = CSR::new(uart.as_mut_ptr() as *mut u32);
        //let baudrate: u32 = 115200;
        //let freq: u32 = 32_000_000;
        //let clk_counter: u32 = (freq + baudrate / 2) / baudrate;
        let clk_counter: u32 = 2174; // empirically determined for the FPGA target
        uart_csr.wo(utra::udma_uart_0::REG_UART_SETUP,
            0x0306 | (clk_counter << 16)
        );
    }

    #[cfg(feature="inject")]
    {
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
    pub fn get_writer(&self) -> OutputWriter {
        OutputWriter {}
    }

    pub fn run(&mut self) {
        loop {
            xous::wait_event();
        }
    }
}

#[cfg(feature="inject")]
fn handle_console_irq(_irq_no: usize, arg: *mut usize) {
    if cfg!(feature = "logging") {
        let mut inject_csr = CSR::new(arg as *mut u32);
        let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });
        // println!("rxe {}", uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY));
        while uart_csr.rf(utra::uart::RXEMPTY_RXEMPTY) == 0 {
            // I really rather think this is more readable, than the "Rusty" version below.
            inject_csr.wfo(
                utra::keyinject::UART_CHAR_CHAR,
                uart_csr.rf(utra::uart::RXTX_RXTX),
            );
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
        #[cfg(feature="cramium-fpga")]
        {
            let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });

            while uart_csr.r(utra::duart::SFR_SR) != 0 {}
            uart_csr.wo(utra::duart::SFR_TXD, c as u32);

            // there's a race condition in the handler, if a new character comes in while handling the interrupt,
            // the pending bit never clears. If the console seems to freeze, uncomment this line.
            // This kind of works around that, at the expense of maybe losing some Rx characters.
            // uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
        }
        #[cfg(feature="cramium-soc")]
        {
            let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });
            // enqueue our character to send via DMA
            unsafe {
                if UART_DMA_BUF as usize != 0 {
                    UART_DMA_BUF.write_volatile(c); // write to the virtual memory address
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

    /// Write a buffer to the output and return the number of
    /// bytes written. This is mostly compatible with `std::io::Write`,
    /// except it is infallible.
    pub fn write(&mut self, buf: &[u8]) -> usize {
        for c in buf {
            self.putc(*c);
        }
        buf.len()
    }

    pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> {
        Ok(self.write(buf))
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
