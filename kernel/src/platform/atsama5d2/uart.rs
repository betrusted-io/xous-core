// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use atsama5d27::uart::{Uart as UartHw, Uart1, UART_BASE_ADDRESS};
use xous_kernel::{arch::irq::IrqNumber, MemoryFlags, MemoryType};

use crate::{
    debug::shell::process_characters,
    io::{SerialRead, SerialWrite},
    mem::MemoryManager,
    PID,
};

const UART_NUMBER: usize = 1;
type UartType = UartHw<Uart1>; // Make sure this matches the UART_NUMBER above
const UART_IRQ_NUM: IrqNumber = IrqNumber::Uart1;

pub const HW_UART_BASE: u32 = UART_BASE_ADDRESS[UART_NUMBER];

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
    uart_csr: UartType,
    callback: fn(&mut Self),
}

impl Uart {
    pub fn new(addr: usize, callback: fn(&mut Self)) -> Uart {
        Uart { uart_csr: UartHw::with_alt_base_addr(addr as u32), callback }
    }

    pub fn init(&mut self) {
        // no-op, it should be already initialized by the 2nd stage bootloader
    }

    pub fn irq(_irq_number: usize, arg: *mut usize) {
        let uart = unsafe { &mut *(arg as *mut Uart) };
        (uart.callback)(uart);
    }

    pub fn enable_rx_irq(&mut self) {
        self.uart_csr.set_rx_interrupt(true);
        self.uart_csr.set_rx(true);
    }
}

impl SerialWrite for Uart {
    fn putc(&mut self, c: u8) { self.uart_csr.write_byte(c); }
}

impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> { self.uart_csr.getc_nonblocking() }
}

/// Initialize UART driver and debug shell.
pub fn init() {
    // Map the UART peripheral.
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_UART_BASE as *mut u8,
                (UART_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map serial port")
    });

    let mut uart = Uart::new(UART_ADDR, process_characters);
    uart.init();

    unsafe {
        UART = Some(uart);
        crate::debug::shell::init(UART.as_mut().unwrap());

        // Claim UART interrupt
        klog!("Claiming IRQ {:?} via syscall...", UART_IRQ_NUM);
        xous_kernel::claim_interrupt(
            UART_IRQ_NUM as usize,
            Uart::irq,
            (UART.as_mut().unwrap() as *mut Uart) as *mut usize,
        )
        .expect("Couldn't claim debug interrupts");
        (UART.as_mut().unwrap() as &mut Uart).enable_rx_irq();
    }
}
