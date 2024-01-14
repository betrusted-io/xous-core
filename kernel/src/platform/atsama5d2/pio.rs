// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use atsama5d27::pio::{Direction, Func, PioA, PioB, PioC, PioD, PioPort};
use utralib::HW_PIO_BASE;
use xous_kernel::{MemoryFlags, MemoryType, PID};

use crate::mem::MemoryManager;

/// The manually chosen virtual address has to be in the top 4MiB as it is the
/// only page shared among all processes.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const PIO_KERNEL_ADDR: usize = 0xffcd_0000;

/// Initialize PIO driver.
///
/// Needed so that the kernel can manage peripherals.
pub fn init() {
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_PIO_BASE as *mut u8,
                (PIO_KERNEL_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map PIO to kernel")
    });
}

/// *NOTE*: this assumes that PIO memory mapping was initialized beforehand.
pub fn init_lcd_pins() {
    let addr = PIO_KERNEL_ADDR as u32;

    PioA::configure_pins_by_mask(Some(addr), 1 << 10, Func::Gpio, Direction::Output);
    PioA::clear_all(Some(addr));

    PioB::configure_pins_by_mask(Some(addr), 0xe7e7e000, Func::A, None);
    PioB::configure_pins_by_mask(Some(addr), 0x2, Func::A, None);
    PioB::clear_all(Some(addr));
    PioC::configure_pins_by_mask(Some(addr), 0x1ff, Func::A, None);
    PioD::configure_pins_by_mask(Some(addr), 0x30, Func::A, None);
}
