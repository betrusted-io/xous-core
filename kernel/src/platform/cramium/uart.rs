// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

// FIXME(static_mut_refs): Do not allow `static_mut_refs` lint
#![allow(static_mut_refs)]

use utralib::generated::*;
use xous_kernel::{MemoryFlags, MemoryType};

#[cfg(feature = "cramium-fpga")]
use crate::{
    PID,
    debug::shell::process_characters,
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
};
#[cfg(feature = "cramium-soc")]
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
pub const IRQ0_ADDR: usize = UART_ADDR + 0x1000;

/// UART instance.
///
/// Initialized by [`init`].
pub static mut UART: Option<Uart> = None;

/// UART peripheral driver.
#[cfg(feature = "cramium-fpga")]
pub struct Uart {
    uart_csr: CSR<u32>,
    irq_csr: CSR<u32>,
    callback: fn(&mut Self),
}

#[cfg(feature = "cramium-fpga")]
impl Uart {
    pub fn new(addr: usize, irq_addr: usize, callback: fn(&mut Self)) -> Uart {
        Uart { uart_csr: CSR::new(addr as *mut u32), irq_csr: CSR::new(irq_addr as *mut u32), callback }
    }

    pub fn init(&mut self) {
        self.uart_csr.rmwf(utra::uart::EV_ENABLE_RX, 1);
        self.irq_csr.rmwf(utra::irqarray0::EV_ENABLE_SOURCE0, 1);
    }

    pub fn irq(_irq_number: usize, arg: *mut usize) {
        let uart = unsafe { &mut *(arg as *mut Uart) };
        (uart.callback)(uart);
        // uart.acknowledge_irq();
    }
}

#[cfg(feature = "cramium-fpga")]
impl SerialWrite for Uart {
    fn putc(&mut self, c: u8) {
        // Wait until TXFULL is `0`
        while self.uart_csr.r(utra::uart::TXFULL) != 0 {}
        self.uart_csr.wfo(utra::uart::RXTX_RXTX, c as u32);
    }
}

#[cfg(feature = "cramium-fpga")]
impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> {
        // If EV_PENDING_RX is 1, return the pending character.
        // Otherwise, return None.
        match self.uart_csr.rf(utra::uart::EV_PENDING_RX) {
            0 => None,
            _ => {
                let ret = Some(self.uart_csr.r(utra::uart::RXTX) as u8);
                self.uart_csr.wfo(utra::uart::EV_PENDING_RX, 1);
                self.irq_csr.wfo(utra::irqarray0::EV_PENDING_SOURCE0, 1);
                ret
            }
        }
    }
}

/// Initialize UART driver and debug shell.
#[cfg(feature = "cramium-fpga")]
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
    // Map the IRQ0 handler
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utra::irqarray0::HW_IRQARRAY0_BASE as *mut u8,
                (IRQ0_ADDR & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map serial port")
    });

    let mut uart = Uart::new(UART_ADDR, IRQ0_ADDR, process_characters);
    uart.init();

    unsafe {
        UART = Some(uart);
        crate::debug::shell::init((&mut *(&raw mut UART)).as_mut().unwrap());

        // Claim UART interrupt.
        println!("Claiming IRQ {} via syscall...", utra::uart::UART_IRQ);
        xous_kernel::claim_interrupt(
            utra::uart::UART_IRQ,
            Uart::irq,
            ((&mut *(&raw mut UART)).as_mut().unwrap() as *mut Uart) as *mut usize,
        )
        .expect("Couldn't claim debug interrupt");
    }
}

#[cfg(feature = "cramium-soc")]
pub fn init() {
    // there is no kernel UART yet...just a placeholder function
    // Map the UART peripheral.
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utra::duart::HW_DUART_BASE as *mut u8,
                (UART_ADDR & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map serial port")
    });
    let mut uart = Uart::new(UART_ADDR, IRQ0_ADDR, process_characters);
    uart.init();
    unsafe {
        UART = Some(uart);
        crate::debug::shell::init((&mut *(&raw mut UART)).as_mut().unwrap());
    }
}

#[cfg(feature = "cramium-soc")]
pub struct Uart {
    uart_csr: CSR<u32>,
}

#[cfg(feature = "cramium-soc")]
impl Uart {
    pub fn new(addr: usize, _irq_addr: usize, _callback: fn(&mut Self)) -> Uart {
        Uart { uart_csr: CSR::new(addr as *mut u32) }
    }

    pub fn init(&mut self) {
        // duart requires no special initializations
    }
}

#[cfg(feature = "cramium-soc")]
impl SerialWrite for Uart {
    fn putc(&mut self, c: u8) {
        while self.uart_csr.r(utra::duart::SFR_SR) != 0 {}
        self.uart_csr.wo(utra::duart::SFR_TXD, c as u32);
    }
}

#[cfg(feature = "cramium-soc")]
impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> { None }
}
