// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType};

use crate::{
    PID,
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
};

static UART_CALLBACK_POINTER: AtomicUsize = AtomicUsize::new(0);
static UART_COUNT: AtomicUsize = AtomicUsize::new(0);
static UART_ALLOCATED: AtomicBool = AtomicBool::new(false);

/// UART virtual address.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const GDB_UART_VADDR: usize = 0xffcc_0000;
pub const GDB_UART_IFRAM_VADDR: usize = 0xffcc_1000;
pub const GDB_UART_IRQ_VADDR: usize = 0xffcc_2000;

pub const GDB_BAUD: u32 = 115200;

/// UART peripheral driver.
pub struct GdbUart {
    uart_csr: CSR<u32>,
    uart_irq: CSR<u32>,
    constructed: bool,
}

fn gdbuart_isr(_irq_no: usize, _arg: *mut usize) {
    let target = UART_CALLBACK_POINTER.load(Ordering::Relaxed);
    // Return if uninitialized
    if target == 0 {
        return;
    }

    let cb = unsafe { core::mem::transmute::<_, fn(&mut GdbUart)>(target) };
    cb(&mut GdbUart {
        uart_csr: CSR::new(GDB_UART_VADDR as *mut u32),
        uart_irq: CSR::new(GDB_UART_IRQ_VADDR as *mut u32),
        constructed: false,
    });
}

impl GdbUart {
    pub fn new(callback: fn(&mut Self)) -> Option<GdbUart> {
        UART_CALLBACK_POINTER.store(callback as usize, Ordering::Relaxed);
        if UART_COUNT.fetch_add(1, Ordering::Relaxed) != 0 {
            panic!("UART has multiple consumers!");
        }

        Some(GdbUart {
            uart_csr: CSR::new(GDB_UART_VADDR as *mut u32),
            uart_irq: CSR::new(GDB_UART_IRQ_VADDR as *mut u32),
            constructed: true,
        })
    }

    pub fn enable(&mut self) {
        self.allocate();
        self.uart_csr.rmwf(utra::irqarray5::EV_ENABLE_UART2_RX, 1);
        let mut udma_uart = unsafe {
            // safety: this is safe to call, because we set up clock and events prior to calling new.
            bao1x_hal::udma::Uart::get_handle(
                self.uart_csr.base() as usize,
                bao1x_hal::board::UART_DMA_TX_BUF_PHYS,
                GDB_UART_IFRAM_VADDR,
            )
        };
        udma_uart.set_baud(GDB_BAUD, bao1x_hal::clocks::PERCLK_HZ);
        udma_uart.setup_async_read();
    }

    pub fn allocate(&mut self) {
        if UART_ALLOCATED.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed).is_err() {
            return;
        }

        // Map the UART CSR
        MemoryManager::with_mut(|memory_manager| {
            memory_manager
                .map_range(
                    utralib::utra::udma_uart_2::HW_UDMA_UART_2_BASE as *mut u8,
                    (GDB_UART_VADDR & !4095) as *mut u8,
                    4096,
                    PID::new(1).unwrap(),
                    MemoryFlags::R | MemoryFlags::W,
                    MemoryType::Default,
                )
                .expect("unable to map serial port");
            memory_manager
                .map_range(
                    bao1x_hal::board::UART_DMA_TX_BUF_PHYS as *mut u8,
                    (GDB_UART_IFRAM_VADDR & !4095) as *mut u8,
                    4096,
                    PID::new(1).unwrap(),
                    MemoryFlags::R | MemoryFlags::W,
                    MemoryType::Default,
                )
                .expect("unable to map serial port");
            memory_manager
                .map_range(
                    utralib::utra::irqarray5::HW_IRQARRAY5_BASE as *mut u8,
                    (GDB_UART_IRQ_VADDR & !4095) as *mut u8,
                    4096,
                    PID::new(1).unwrap(),
                    MemoryFlags::R | MemoryFlags::W,
                    MemoryType::Default,
                )
                .expect("unable to map serial port");
        });

        xous_kernel::claim_interrupt(utra::irqarray5::IRQARRAY5_IRQ, gdbuart_isr, core::ptr::null_mut())
            .expect("Couldn't claim debug interrupt");
    }

    #[allow(dead_code)]
    pub fn deallocate(&mut self) {
        self.uart_csr.rmwf(utra::irqarray5::EV_ENABLE_UART2_RX, 0);

        // Note: This can cause an ABA error if it's multi-threaded and `.allocate()`
        // is called at the same time as `.deallocate()`.
        if UART_ALLOCATED.compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed).is_err() {
            return;
        }

        xous_kernel::rsyscall(xous_kernel::SysCall::FreeInterrupt(utra::irqarray5::IRQARRAY5_IRQ)).unwrap();

        MemoryManager::with_mut(|memory_manager| {
            memory_manager.unmap_page((GDB_UART_VADDR & !4095) as *mut usize).unwrap();
            memory_manager.unmap_page((GDB_UART_IFRAM_VADDR & !4095) as *mut usize).unwrap();
            memory_manager.unmap_page((GDB_UART_IRQ_VADDR & !4095) as *mut usize).unwrap();
        });
    }
}

impl Drop for GdbUart {
    fn drop(&mut self) {
        // Sometimes (e.g. during IRQs) we synthesize a GdbUart from nothing.
        if !self.constructed {
            return;
        }

        if UART_COUNT.fetch_sub(1, Ordering::Relaxed) != 1 {
            panic!("UART had multiple consumers!");
        }

        // Disable the IRQ until we re-enable it again when the server is reconstituted
        self.uart_csr.rmwf(utra::irqarray5::EV_ENABLE_UART2_RX, 0);
    }
}

impl SerialWrite for GdbUart {
    fn putc(&mut self, c: u8) {
        let buf: [u8; 1] = [c];
        let mut udma_uart = unsafe {
            // safety: this is safe to call, because we set up clock and events prior to calling new.
            bao1x_hal::udma::Uart::get_handle(
                self.uart_csr.base() as usize,
                bao1x_hal::board::UART_DMA_TX_BUF_PHYS,
                GDB_UART_IFRAM_VADDR,
            )
        };
        udma_uart.write(&buf);
    }
}

impl SerialRead for GdbUart {
    fn getc(&mut self) -> Option<u8> {
        // might be a bit too big a hammer to clear everything pending, but this gets us moving
        self.uart_irq.wo(utra::irqarray5::EV_PENDING, 0xFFFF_FFFF);

        let mut uart = unsafe {
            bao1x_hal::udma::Uart::get_handle(
                self.uart_csr.base() as usize,
                bao1x_hal::board::UART_DMA_TX_BUF_PHYS,
                GDB_UART_IFRAM_VADDR,
            )
        };

        let mut c: u8 = 0;
        /*
        println!(
            "{:x} {:x} {:x} {:x} {:x}",
            uart.csr()
                .base()
                .add(bao1x_hal::udma::Bank::Custom.into())
                .add(bao1x_hal::udma::UartReg::Status.into())
                .read_volatile(),
            uart.csr()
                .base()
                .add(bao1x_hal::udma::Bank::Custom.into())
                .add(bao1x_hal::udma::UartReg::Setup.into())
                .read_volatile(),
            uart.csr()
                .base()
                .add(bao1x_hal::udma::Bank::Custom.into())
                .add(bao1x_hal::udma::UartReg::Error.into())
                .read_volatile(),
            uart.csr()
                .base()
                .add(bao1x_hal::udma::Bank::Custom.into())
                .add(bao1x_hal::udma::UartReg::Valid.into())
                .read_volatile(),
            uart.csr()
                .base()
                .add(bao1x_hal::udma::Bank::Custom.into())
                .add(bao1x_hal::udma::UartReg::Data.into())
                .read_volatile()
        ); */
        if uart.read_async(&mut c) != 0 {
            print!("{}", char::from_u32_unchecked(c as u32));
            Some(c)
        } else {
            return None;
        }
    }
}

impl gdbstub::conn::Connection for GdbUart {
    type Error = &'static str;

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.putc(byte);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        self.enable();
        Ok(())
    }
}
