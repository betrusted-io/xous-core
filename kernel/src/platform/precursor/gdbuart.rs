// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::{
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
    PID,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType};

static UART_USAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// UART virtual address.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const APP_UART_ADDR: usize = 0xffcc_0000;

/// UART peripheral driver.
pub struct GdbUart {
    uart_csr: CSR<u32>,
    callback: fn(&mut Self),
}

impl GdbUart {
    pub fn new(callback: fn(&mut Self)) -> Option<GdbUart> {
        if UART_USAGE_COUNT
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return None;
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
        let mut gdb_uart = GdbUart {
            uart_csr: CSR::new(APP_UART_ADDR as *mut u32),
            callback,
        };

        xous_kernel::claim_interrupt(
            utra::app_uart::APP_UART_IRQ,
            GdbUart::irq,
            (&mut gdb_uart as *mut GdbUart) as *mut usize,
        )
        .expect("Couldn't claim debug interrupt");

        Some(gdb_uart)
    }

    pub fn enable(&mut self) {
        self.uart_csr.rmwf(utra::app_uart::EV_ENABLE_RX, 1);
    }

    pub fn irq(_irq_number: usize, arg: *mut usize) {
        let uart = unsafe { &mut *(arg as *mut GdbUart) };
        (uart.callback)(uart);
    }
}

impl Drop for GdbUart {
    fn drop(&mut self) {
        if UART_USAGE_COUNT.fetch_sub(1, Ordering::Relaxed) != 1 {
            panic!("UART ad a usage count of more than 1");
        }

        println!("Freeing interrupt since the count has gone to zero");
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
        match self.uart_csr.rf(utra::app_uart::EV_PENDING_RX) {
            0 => None,
            _ => {
                let ret = Some(self.uart_csr.r(utra::app_uart::RXTX) as u8);
                self.uart_csr.wfo(utra::app_uart::EV_PENDING_RX, 1);
                ret
            }
        }
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
