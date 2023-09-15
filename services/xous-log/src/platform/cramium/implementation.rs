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
        let tx_buf_region = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_IFRAM0_MEM),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map UDMA buffer");
        unsafe { UART_DMA_BUF = tx_buf_region.as_mut_ptr() as *mut u8 };

        /*
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
            iox_csr.add(0).write_volatile(0b00_00_00_01_01_00_00_00);  // PAL AF1 on PA3/PA4
            iox_csr.add(0x1c / core::mem::size_of::<u32>()).write_volatile(0x1400); // PDH
            iox_csr.add(0x148 / core::mem::size_of::<u32>()).write_volatile(0x10); // PA4 output
            iox_csr.add(0x148 / core::mem::size_of::<u32>() + 3).write_volatile(0xffff); // PD
            iox_csr.add(0x160 / core::mem::size_of::<u32>()).write_volatile(0x8); // PA3 pullup
        }

        let mut udma_ctrl = CSR::new(udma_ctrl.as_mut_ptr() as _);
        // ungate the clock for the UART. TODO: send this to a power management common server...
        // probably should be in the same server that allocates UDMA buffers.
        udma_ctrl.wo(utra::udma_ctrl::REG_CG, 1);

        // setup the baud rate
        let mut uart_csr = CSR::new(uart.as_mut_ptr() as *mut u32);
        let baudrate: u32 = 115200;
        let freq: u32 = 100_000_000;
        let clk_counter: u32 = (freq + baudrate / 2) / baudrate;
        uart_csr.wo(utra::udma_uart_0::REG_UART_SETUP,
            0x0306 | (clk_counter << 16)
        ); */

        // rely on the bootloader to set up all the above params
        /* //  for debug: send a test string to confirm everything is configured correctly
        let tx_buf = tx_buf_region.as_mut_ptr() as *mut u8;
        for i in 0..16 {
            unsafe { tx_buf.add(i).write_volatile('M' as u32 as u8 + i as u8) };
        }
        let mut udma_uart = CSR::new(uart.as_mut_ptr() as *mut u32);
        udma_uart.wo(utra::udma_uart_0::REG_TX_SADDR, tx_buf as u32);
        udma_uart.wo(utra::udma_uart_0::REG_TX_SIZE, 16);
        // send it
        udma_uart.wo(utra::udma_uart_0::REG_TX_CFG, 0x10); // EN
        // wait for it all to be done
        while udma_uart.rf(utra::udma_uart_0::REG_TX_CFG_R_TX_EN) != 0 {   }
        while (udma_uart.r(utra::udma_uart_0::REG_STATUS) & 1) != 0 {  }
        */
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
        #[cfg(feature="cramium-soc")]
        {
            // write the whole buffer via DMA, and then idle with yield_slice() for better
            // concurrency (as opposed to character-by-character polling).

            let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });
            // enqueue our character to send via DMA
            unsafe {
                if UART_DMA_BUF as usize != 0 {
                    // convert the raw pointer to a 4k buffer region. This is "by fiat", we don't
                    // have a formal allocator for this region yet
                    let dest_buf = core::slice::from_raw_parts_mut(UART_DMA_BUF as *mut u8, 4096);
                    // copy the whole buf to the destination
                    for (&s, d) in buf.iter().zip(dest_buf.iter_mut()) {
                        *d = s;
                    }
                }
            }
            // configure the DMA
            uart_csr.wo(utra::udma_uart_0::REG_TX_SADDR, utralib::HW_IFRAM0_MEM as u32); // source is the physical address
            let writelen = buf.len().min(4096); // we will send the smaller of the buffer length or the maximum size of the DMA page
            uart_csr.wo(utra::udma_uart_0::REG_TX_SIZE, writelen as u32);
            // send it
            uart_csr.wo(utra::udma_uart_0::REG_TX_CFG, 0x10); // EN
            // wait for it all to be done
            while uart_csr.rf(utra::udma_uart_0::REG_TX_CFG_R_TX_EN) != 0 {
                // this should complete quickly because we're just ensuring nothing is already in progress
            }
            while (uart_csr.r(utra::udma_uart_0::REG_STATUS) & 1) != 0 {
                // this takes a bit longer; yield the time because we expect the average
                // time to send to be around 0.25ms or so, so this is worth it.
                xous::yield_slice();
            }

            writelen
        }
        #[cfg(feature="cramium-fpga")]
        {
            for c in buf {
                self.putc(*c)
            }
            buf.len()
        }
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
