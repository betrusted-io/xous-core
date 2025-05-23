// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

// FIXME(static_mut_refs): Do not allow `static_mut_refs` lint
#![allow(static_mut_refs)]

#[cfg(feature = "cramium-soc")]
use utralib::generated::*;
#[cfg(feature = "cramium-soc")]
use xous_kernel::{MemoryFlags, MemoryType};

#[cfg(feature = "cramium-fpga")]
use crate::io::{SerialRead, SerialWrite};
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
#[cfg(feature = "cramium-soc")]
pub const UART_ADDR: usize = 0xffcf_0000;
#[cfg(feature = "cramium-soc")]
pub const IRQ0_ADDR: usize = UART_ADDR + 0x1000;

/// UART instance.
///
/// Initialized by [`init`].
#[cfg(feature = "cramium-soc")]
pub static mut UART: Option<Uart> = None;

/// All dummy stubs for cramium-fpga because we want the console to have the DUART
#[cfg(feature = "cramium-fpga")]
pub fn init() {}

#[cfg(feature = "cramium-fpga")]
pub struct Uart {}

#[cfg(feature = "cramium-fpga")]
#[allow(dead_code)]
impl Uart {
    pub fn new(_addr: usize, _irq_addr: usize, _callback: fn(&mut Self)) -> Uart { Uart {} }

    pub fn init(&mut self) {}
}

#[cfg(feature = "cramium-fpga")]
impl SerialWrite for Uart {
    fn putc(&mut self, _c: u8) {}
}

#[cfg(feature = "cramium-fpga")]
impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> { None }
}

#[cfg(feature = "cramium-soc")]
pub fn init() {
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
