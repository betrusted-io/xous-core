// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::fmt::{Error, Write};

use atsama5d27::uart::{UART_BASE_ADDRESS, Uart as UartHw, Uart1};
#[cfg(feature = "lcd-console")]
use atsama5d27::{console::DisplayAndUartConsole, display::FramebufDisplay, lcdc::Lcdc};
#[cfg(feature = "lcd-console")]
use utralib::HW_LCDC_BASE;

const UART_NUMBER: usize = 1;
pub type UartType = UartHw<Uart1>; // Make sure this matches the UART_NUMBER above
pub const HW_UART_BASE: u32 = UART_BASE_ADDRESS[UART_NUMBER];

#[cfg(feature = "lcd-console")]
const WIDTH: usize = 800;
#[cfg(feature = "lcd-console")]
const HEIGHT: usize = 480;
#[cfg(feature = "lcd-console")]
const FB_SIZE_BYTES: usize = WIDTH * HEIGHT * 4; // RGBA888 is 4 bytes per pixel

pub static mut CONSOLE: Option<Console<UartType>> = None;

/// UART and display console.
pub struct Console<U: Write> {
    #[cfg(feature = "lcd-console")]
    inner: DisplayAndUartConsole<U>,

    #[cfg(not(feature = "lcd-console"))]
    inner: U,
}

impl Console<UartType> {
    pub fn new() -> Self {
        // Map the UART peripheral.
        let addr = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_UART_BASE as usize),
            None,
            4096 * 4, // In ATSAMA5D2 peripherals occupy 16K
            xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::DEV,
        )
        .expect("couldn't map debug UART");

        let addr = addr.as_mut_ptr() as _;
        let mut uart = UartType::with_alt_base_addr(addr);
        writeln!(uart, "[xous-log] Allocated UART peripheral at {:08x}", addr).ok();

        // Allocate a page for DMA descriptor
        #[cfg(feature = "lcd-console")]
        let inner = {
            let dma_desc_addr =
                xous::syscall::map_memory(None, None, 4096, xous::MemoryFlags::R | xous::MemoryFlags::W)
                    .expect("couldn't map LCDC DMA descriptor");

            // Access the address of the page to get it physically allocated
            unsafe {
                dma_desc_addr.as_mut_ptr().write_volatile(0);
            }
            let dma_desc_addr = dma_desc_addr.as_ptr() as _;
            let dma_desc_addr_phys = xous::syscall::virt_to_phys(dma_desc_addr).expect("can't convert v2p");

            // Allocate framebuffer pages
            let fb_addr = xous::syscall::map_memory(
                None,
                None,
                FB_SIZE_BYTES,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map lcd framebuffer");

            // Access the addresses of the framebuffer pages to give them
            // a continuous physical backing
            unsafe {
                fb_addr.as_mut_ptr().write_volatile(0x00);
            }
            let fb_addr = fb_addr.as_ptr() as _;
            for page in (0..FB_SIZE_BYTES).into_iter().step_by(4096) {
                unsafe {
                    let page_addr = fb_addr + page;
                    let page_ptr = page_addr as *mut u32;
                    page_ptr.write_volatile(0);
                }
            }

            let fb_addr_phys = xous::syscall::virt_to_phys(fb_addr).expect("can't convert v2p");

            // Map the LCDC peripheral.
            let addr = xous::syscall::map_memory(
                xous::MemoryAddress::new(HW_LCDC_BASE as usize),
                None,
                4096 * 4, // In ATSAMA5D2 peripherals occupy 16K
                xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::DEV,
            )
            .expect("couldn't map LCDC peripheral");
            let lcdc_addr = addr.as_mut_ptr() as _;
            let mut lcdc = Lcdc::new_vma(
                lcdc_addr,
                fb_addr_phys,
                WIDTH as u16,
                HEIGHT as u16,
                dma_desc_addr,
                dma_desc_addr_phys,
            );
            lcdc.init();

            writeln!(uart, "[xous-log] LCDC initialized").ok();

            let fb: &'static mut [u32; WIDTH * HEIGHT] = unsafe {
                let ptr: *const [u32; WIDTH * HEIGHT] = fb_addr as _;
                core::mem::transmute(ptr)
            };

            let display = FramebufDisplay::new(fb, WIDTH, HEIGHT);
            let mut inner = DisplayAndUartConsole::new(display, uart);
            writeln!(inner, "").ok();
            inner
        };
        #[cfg(not(feature = "lcd-console"))]
        let inner = uart;

        // Claim UART interrupt.
        // TODO:
        /*
        println!("Claiming IRQ {} via syscall...", utra::uart::UART_IRQ);
        xous_kernel::claim_interrupt(
            utra::uart::UART_IRQ,
            Uart::irq,
            (UART.as_mut().unwrap() as *mut Uart) as *mut usize,
        ).expect("Couldn't claim debug interrupt");
        */

        Self { inner }
    }

    fn write_str(s: &str) {
        unsafe {
            if CONSOLE.is_none() {
                CONSOLE.replace(Console::new());
            }
        }

        if let Some(console) = unsafe { &mut CONSOLE } {
            #[cfg(feature = "lcd-console")]
            console.inner.write_str(s).ok();

            #[cfg(not(feature = "lcd-console"))]
            console.inner.write_str(s);
        }
    }
}

pub struct ConsoleSingleton {}

impl Write for ConsoleSingleton {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        Console::write_str(s);

        Ok(())
    }
}
