// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

// FIXME(static_mut_refs): Do not allow `static_mut_refs` lint
#![allow(static_mut_refs)]

#[cfg(feature = "bao1x")]
use utralib::generated::*;
#[cfg(feature = "bao1x")]
use xous_kernel::{MemoryFlags, MemoryType};

#[cfg(feature = "bao1x")]
use crate::{PID, io::SerialWrite, mem::MemoryManager};

/// UART virtual address.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
#[cfg(feature = "bao1x")]
pub const UART_ADDR: usize = 0xffcf_0000;

/// UART instance.
///
/// Initialized by [`init`].
#[cfg(feature = "bao1x")]
pub static mut UART: Option<Uart> = None;

#[cfg(feature = "bao1x")]
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
    let mut uart = Uart::new(UART_ADDR);
    uart.init();
    unsafe {
        UART = Some(uart);
        crate::debug::shell::init((&mut *(&raw mut UART)).as_mut().unwrap());
    }
}

#[cfg(feature = "bao1x")]
pub struct Uart {
    uart_csr: CSR<u32>,
}

#[cfg(feature = "bao1x")]
impl Uart {
    pub fn new(addr: usize) -> Uart { Uart { uart_csr: CSR::new(addr as *mut u32) } }

    pub fn init(&mut self) {
        // duart requires no special initializations
    }
}

#[cfg(feature = "bao1x")]
impl SerialWrite for Uart {
    fn putc(&mut self, c: u8) {
        while self.uart_csr.r(utra::duart::SFR_SR) != 0 {}
        self.uart_csr.wo(utra::duart::SFR_TXD, c as u32);
    }
}
