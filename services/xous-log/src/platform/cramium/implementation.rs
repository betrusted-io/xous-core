use core::fmt::{Error, Write};
use std::pin::Pin;

use cramium_hal::board::UART_DMA_TX_BUF_PHYS;
use cramium_hal::udma;
use utralib::generated::*;

pub struct Output {}

#[cfg(feature = "cramium-soc")]
pub static mut UART_DMA_TX_BUF_VIRT: *mut u8 = 0x0000_0000 as *mut u8;

#[cfg(feature = "cramium-soc")]
pub static mut UART_IRQ: Option<Pin<Box<cramium_hal::udma::UartIrq>>> = None;

#[cfg(feature = "cramium-soc")]
pub static mut KBD_CONN: u32 = 0;

pub fn init() -> Output {
    #[cfg(feature = "cramium-fpga")]
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::duart::HW_DUART_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    // TODO: migrate this to a "proper" UART that is available on SoC hardware, but for now all we have access
    // to is the DUART.
    #[cfg(feature = "cramium-soc")]
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::udma_uart_1::HW_UDMA_UART_1_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    unsafe { crate::platform::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() as _ };
    #[cfg(feature = "cramium-soc")]
    {
        // Note: for the TX buf, we allocate a pre-reserved portion of IFRAM as our
        // DMA buffer. We do *not* use the IFRAM allocator in `cram-hal-service` because
        // we want to avoid a circular dependency between the logging crate and
        // the HAL service. Instead, we simply note that this region of memory is
        // always reserved by the logging crate in its allocator.
        let tx_buf_region = xous::syscall::map_memory(
            // we take the last page of IFRAM0 for the Tx buffer.
            xous::MemoryAddress::new(UART_DMA_TX_BUF_PHYS),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map UDMA buffer");
        unsafe { UART_DMA_TX_BUF_VIRT = tx_buf_region.as_mut_ptr() as *mut u8 };

        let mut uart_irq = Box::pin(cramium_hal::udma::UartIrq::new());
        // safety: This is safe because uart_irq is committed into a `static mut` variable that
        // ensures that its lifetime is `static`
        unsafe {
            Pin::as_mut(&mut uart_irq).register_handler(udma::UartChannel::Uart1, uart_handler);
        }
        uart_irq.rx_irq_ena(udma::UartChannel::Uart1, true);
        unsafe { UART_IRQ = Some(uart_irq) };
        let mut udma_uart = unsafe {
            udma::Uart::get_handle(
                crate::platform::debug::DEFAULT_UART_ADDR as usize,
                UART_DMA_TX_BUF_PHYS,
                UART_DMA_TX_BUF_VIRT as usize,
            )
        };
        udma_uart.setup_async_read();
    }
    println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
    println!("Process: map success!");

    Output {}
}

#[cfg(feature = "cramium-soc")]
fn uart_handler(_irq_no: usize, _arg: *mut usize) {
    let mut uart = unsafe {
        udma::Uart::get_handle(
            crate::platform::debug::DEFAULT_UART_ADDR as usize,
            UART_DMA_TX_BUF_PHYS,
            UART_DMA_TX_BUF_VIRT as usize,
        )
    };
    let mut c: u8 = 0;
    if uart.read_async(&mut c) != 0 {
        if unsafe { KBD_CONN } == 0 {
            match xous::try_connect(xous::SID::from_bytes(b"keyboard_bouncer").unwrap()) {
                Ok(cid) => unsafe { KBD_CONN = cid },
                // ignore the character and wait until there's a server for us to send it to
                _ => return,
            }
        }
        if unsafe { KBD_CONN != 0 } {
            let c = char::from_u32(c as u32).unwrap_or('.');
            print!("{}", c); // local echo
            // naked carriage return
            if c == '\r' {
                println!(" "); // line feed with carriage return
            }
            xous::try_send_message(unsafe { KBD_CONN }, xous::Message::new_scalar(0, c as usize, 0, 0, 0))
                .ok();
        }
    }
}

impl Output {
    pub fn get_writer(&self) -> OutputWriter { OutputWriter {} }

    pub fn run(&mut self) {
        loop {
            xous::wait_event();
        }
    }
}

pub struct OutputWriter {}

#[allow(dead_code)]
impl OutputWriter {
    pub fn putc(&self, c: u8) {
        #[cfg(feature = "cramium-fpga")]
        {
            let mut uart_csr = CSR::new(unsafe { crate::platform::debug::DEFAULT_UART_ADDR as *mut u32 });

            while uart_csr.r(utra::duart::SFR_SR) != 0 {}
            uart_csr.wo(utra::duart::SFR_TXD, c as u32);

            // there's a race condition in the handler, if a new character comes in while handling the
            // interrupt, the pending bit never clears. If the console seems to freeze, uncomment
            // this line. This kind of works around that, at the expense of maybe losing some Rx
            // characters. uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
        }
        #[cfg(feature = "cramium-soc")]
        {
            // safety: safe to call as long as the raw parts are initialized and we exclusively
            // own it; and the UART has been initialized. For this peripheral, initialization
            // is handled by the loader and tossed to us, and exclusivity of access is something
            // we just have to not bungle.
            let mut uart = unsafe {
                udma::Uart::get_handle(
                    crate::platform::debug::DEFAULT_UART_ADDR as usize,
                    UART_DMA_TX_BUF_PHYS,
                    UART_DMA_TX_BUF_VIRT as usize,
                )
            };

            // enqueue our character to send via DMA
            uart.write(&[c]);
        }
    }

    /// Write a buffer to the output and return the number of
    /// bytes written. This is mostly compatible with `std::io::Write`,
    /// except it is infallible.
    pub fn write(&mut self, buf: &[u8]) -> usize {
        #[cfg(feature = "cramium-soc")]
        {
            // safety: safe to call as long as the raw parts are initialized and we exclusively
            // own it; and the UART has been initialized. For this peripheral, initialization
            // is handled by the loader and tossed to us, and exclusivity of access is something
            // we just have to not bungle.
            let mut uart = unsafe {
                udma::Uart::get_handle(
                    crate::platform::debug::DEFAULT_UART_ADDR as usize,
                    UART_DMA_TX_BUF_PHYS,
                    UART_DMA_TX_BUF_VIRT as usize,
                )
            };
            uart.write(buf)
        }
        #[cfg(feature = "cramium-fpga")]
        {
            for c in buf {
                self.putc(*c)
            }
            buf.len()
        }
    }

    pub fn write_all(&mut self, buf: &[u8]) -> core::result::Result<usize, ()> { Ok(self.write(buf)) }
}

impl Write for OutputWriter {
    /// Optimizes performance for the typical case of strings < 210 chars long.
    /// We have to insert `\r` after every `\n`, leading to a potential 2x growth in
    /// string size for a reference string that is all `\n`. The downside of
    /// over-initializing stack space is has to have 0's written to it, which costs
    /// CPU time and blows out the cache. We're going to go out on a limb and guess
    /// that there are few cases where we'd pack a huge number of newlines with
    /// short strings. In the case that we do, we'll end up truncating the output
    /// because we ran out of space to insert the newlines.
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        const OPTIMIZATION_BREAKPOINT: usize = 210;
        const STACKBUF_LEN: usize = 256;
        if s.len() < OPTIMIZATION_BREAKPOINT {
            let mut crlf_buf = [0u8; STACKBUF_LEN];
            let mut ptr = 0;
            for c in s.bytes() {
                crlf_buf[ptr] = c;
                ptr += 1;
                if ptr >= STACKBUF_LEN {
                    break;
                }
                if c == '\n' as u8 {
                    crlf_buf[ptr] = '\r' as u8;
                    ptr += 1;
                    if ptr >= STACKBUF_LEN {
                        break;
                    }
                }
            }
            // now emit the buffer in one efficient, DMA-powered write operation
            self.write(&crlf_buf[..ptr]);
        } else {
            // in case of long strings, the print will be less efficient because
            // we go character-by-character to convert CRLF; the trade-off is the storage
            // could be unbounded. Could also fall back to using a `Vec`, but alloc's
            // are actually extremely expensive and we'd like to avoid them in the logging
            // crate.
            for c in s.bytes() {
                self.putc(c);
                if c == '\n' as u8 {
                    self.putc('\r' as u8);
                }
            }
        }
        Ok(())
    }
}
