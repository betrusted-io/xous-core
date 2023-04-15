// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::{
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
    PID,
};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType};

static UART_CALLBACK_POINTER: AtomicUsize = AtomicUsize::new(0);
static UART_COUNT: AtomicUsize = AtomicUsize::new(0);
static UART_ALLOCATED: AtomicBool = AtomicBool::new(false);

/// UART virtual address.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const APP_UART_ADDR: usize = 0xffcc_0000;

/// UART peripheral driver.
pub struct GdbUart {
    uart_csr: CSR<u32>,
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
        uart_csr: CSR::new(APP_UART_ADDR as *mut u32),
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
            uart_csr: CSR::new(APP_UART_ADDR as *mut u32),
            constructed: true,
        })
    }

    pub fn enable(&mut self) {
        self.allocate();
        self.uart_csr.rmwf(utra::app_uart::EV_ENABLE_RX, 1);
    }

    pub fn allocate(&mut self) {
        if UART_ALLOCATED
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        // Map the UART peripheral.
        MemoryManager::with_mut(|memory_manager| {
            memory_manager
                .map_range(
                    utra::app_uart::HW_APP_UART_BASE as *mut u8,
                    (APP_UART_ADDR & !4095) as *mut u8,
                    4096,
                    PID::new(1).unwrap(),
                    MemoryFlags::R | MemoryFlags::W,
                    MemoryType::Default,
                )
                .expect("unable to map serial port")
        });

        xous_kernel::claim_interrupt(
            utra::app_uart::APP_UART_IRQ,
            gdbuart_isr,
            core::ptr::null_mut(),
        )
        .expect("Couldn't claim debug interrupt");
    }

    #[allow(dead_code)]
    pub fn deallocate(&mut self) {
        self.uart_csr.rmwf(utra::app_uart::EV_ENABLE_RX, 0);

        // Note: This can cause an ABA error if it's multi-threaded and `.allocate()`
        // is called at the same time as `.deallocate()`.
        if UART_ALLOCATED
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        xous_kernel::rsyscall(xous_kernel::SysCall::FreeInterrupt(
            utra::app_uart::APP_UART_IRQ,
        ))
        .unwrap();

        MemoryManager::with_mut(|memory_manager| {
            memory_manager
                .unmap_page((APP_UART_ADDR & !4095) as *mut usize)
                .unwrap()
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
        self.uart_csr.rmwf(utra::app_uart::EV_ENABLE_RX, 0);
    }
}

impl SerialWrite for GdbUart {
    fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.uart_csr.r(utra::app_uart::TXFULL) != 0 {}
        self.uart_csr.wfo(utra::app_uart::RXTX_RXTX, c as u32);
    }
}

impl SerialRead for GdbUart {
    fn getc(&mut self) -> Option<u8> {
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        if self.uart_csr.r(utra::app_uart::RXEMPTY) != 0 {
            return None;
        }

        let ret = self.uart_csr.r(utra::app_uart::RXTX) as u8;
        self.uart_csr.wfo(utra::app_uart::EV_PENDING_RX, 1);
        Some(ret)
    }
}

impl gdbstub::conn::Connection for GdbUart {
    type Error = &'static str;

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.putc(byte);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        self.enable();
        Ok(())
    }
}
