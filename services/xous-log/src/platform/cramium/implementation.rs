use core::fmt::{Error, Write};

use cramium_hal::udma;
use utralib::generated::*;

pub struct Output {}

#[cfg(feature = "cramium-soc")]
pub static mut UART_DMA_TX_BUF_VIRT: *mut u8 = 0x0000_0000 as *mut u8;

#[cfg(feature = "cramium-soc")]
pub const UART_DMA_TX_BUF_PHYS: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;

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
        xous::MemoryAddress::new(utra::udma_uart_0::HW_UDMA_UART_0_BASE),
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
    }
    println!("Mapped UART @ {:08x}", uart.as_ptr() as usize);
    println!("Process: map success!");

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
            // safety: safe to call as long as DEFAULT_UART_ADDR is initialized and we exclusively
            // own it; and the UART has been initialized. For this peripheral, initialization
            // is handled by the loader and tossed to us.
            let mut uart =
                unsafe { udma::Uart::get_handle(crate::platform::debug::DEFAULT_UART_ADDR as usize) };

            // enqueue our character to send via DMA
            let tx_buf_virt = unsafe {
                // safety: it's safe only because we are manually tracking the allocations in IFRAM0.
                core::slice::from_raw_parts_mut(UART_DMA_TX_BUF_VIRT as *mut u8, 1)
            };
            let tx_buf_phys = unsafe {
                // safety: it's safe only because we know that the UART TX buffer is expressly reserved at
                // this location
                core::slice::from_raw_parts_mut(UART_DMA_TX_BUF_PHYS as *mut u8, 1)
            };
            tx_buf_virt[0] = c;
            // note that write_phys takes the *physical* address. We use `write_phys` because benchmarks
            // show significant performance improvements skipping the V2P step inherent `write` (an extra
            // 2 milliseconds for 15 print statements that are 90 chars in length).
            unsafe {
                uart.write_phys(tx_buf_phys);
            }
        }
    }

    /// Write a buffer to the output and return the number of
    /// bytes written. This is mostly compatible with `std::io::Write`,
    /// except it is infallible.
    pub fn write(&mut self, buf: &[u8]) -> usize {
        #[cfg(feature = "cramium-soc")]
        {
            // safety: safe to call as long as DEFAULT_UART_ADDR is initialized and we exclusively
            // own it; and the UART has been initialized. For this peripheral, initialization
            // is handled by the loader and tossed to us.
            let mut uart =
                unsafe { udma::Uart::get_handle(crate::platform::debug::DEFAULT_UART_ADDR as usize) };

            // enqueue our character to send via DMA
            let tx_buf_virt = unsafe {
                // safety: it's safe only because we are manually tracking the allocations in IFRAM0.
                core::slice::from_raw_parts_mut(UART_DMA_TX_BUF_VIRT as *mut u8, 4096)
            };
            // create a "fictional" physical slice at the physical address where our buffer is mapped.
            let tx_buf_phys = unsafe {
                // safety: it's safe only because we know that the UART TX buffer is expressly reserved at
                // this location
                core::slice::from_raw_parts_mut(UART_DMA_TX_BUF_PHYS as *mut u8, 4096)
            };
            let mut writelen = 0;
            for page in buf.chunks(4096) {
                tx_buf_virt[..page.len()].copy_from_slice(page);
                // note that the write_phys uses the *physical address* slice
                unsafe {
                    uart.write_phys(&tx_buf_phys[..page.len()]);
                }
                writelen += page.len();
            }

            writelen
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
