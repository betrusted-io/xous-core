// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType};

use crate::{
    PID,
    debug::shell::process_characters,
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
};

/// UART virtual address.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const UART_ADDR: usize = 0xffcf_0000;

/// UART instance.
///
/// Initialized by [`init`].
pub static mut UART: Option<Uart> = None;

/// UART peripheral driver.
pub struct Uart {
    uart_csr: CSR<u32>,
    callback: fn(&mut Self),
}

impl Uart {
    pub fn new(addr: usize, callback: fn(&mut Self)) -> Uart {
        Uart { uart_csr: CSR::new(addr as *mut u32), callback }
    }

    pub fn init(&mut self) { self.uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1); }

    pub fn irq(_irq_number: usize, arg: *mut usize) {
        let uart = unsafe { &mut *(arg as *mut Uart) };
        (uart.callback)(uart);
        // uart.acknowledge_irq();
    }
}

impl SerialWrite for Uart {
    fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.uart_csr.r(utra::uart::TXFULL) != 0 {}
        self.uart_csr.wfo(utra::uart::RXTX_RXTX, c as u32);
    }
}

impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> {
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        match self.uart_csr.rf(utra::uart::EV_PENDING_RX) {
            0 => None,
            _ => {
                let ret = Some(self.uart_csr.r(utra::uart::RXTX) as u8);
                self.uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
                ret
            }
        }
    }
}

/// Initialize UART driver and debug shell.
pub fn init() {
    // Map the UART peripheral.
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utra::uart::HW_UART_BASE as *mut u8,
                (UART_ADDR & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map serial port")
    });

    let mut uart = Uart::new(UART_ADDR, process_characters);
    uart.init();

    unsafe {
        UART = Some(uart);
        crate::debug::shell::init(UART.as_mut().unwrap());

        // Claim UART interrupt.
        println!("Claiming IRQ {} via syscall...", utra::uart::UART_IRQ);
        xous_kernel::claim_interrupt(
            utra::uart::UART_IRQ,
            Uart::irq,
            (UART.as_mut().unwrap() as *mut Uart) as *mut usize,
        )
        .expect("Couldn't claim debug interrupt");
    }
}
