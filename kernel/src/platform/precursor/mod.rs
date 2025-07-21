// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "gdb-stub")]
pub mod gdbuart;
#[cfg(all(feature = "print-panics"))]
pub mod lcdpanic;
pub mod rand;
#[cfg(any(feature = "debug-print", feature = "print-panics"))]
pub mod uart;

#[cfg(feature = "vexii-test")]
use crate::{MemoryFlags, MemoryType, PID, mem::MemoryManager};
#[cfg(feature = "vexii-test")]
pub const LEGACY_INT_VMEM: usize = 0xffcf_1000; // see https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md

#[allow(dead_code)]
#[cfg(feature = "vexii-test")]
pub mod legacy_int {
    // this enum is vestigal, and currently not used by anything
    #[derive(Debug, Copy, Clone)]
    pub enum LegacyIntOffset {
        MachMask = 0,
        MachPending = 1,
        SuperMask = 2,
        SuperPending = 3,
    }
    pub const LEGACY_INT_NUMREGS: usize = 4;

    pub const MACH_MASK: utralib::Register = utralib::Register::new(0, 0xffffffff);
    pub const MACH_MASK_MACH_MASK: utralib::Field = utralib::Field::new(32, 0, MACH_MASK);

    pub const MACH_PENDING: utralib::Register = utralib::Register::new(1, 0xffffffff);
    pub const MACH_PENDING_MACH_PENDING: utralib::Field = utralib::Field::new(32, 0, MACH_PENDING);

    pub const SUPER_MASK: utralib::Register = utralib::Register::new(2, 0xffffffff);
    pub const SUPER_MASK_SUPER_MASK: utralib::Field = utralib::Field::new(32, 0, SUPER_MASK);

    pub const SUPER_PENDING: utralib::Register = utralib::Register::new(3, 0xffffffff);
    pub const SUPER_PENDING_SUPER_PENDING: utralib::Field = utralib::Field::new(32, 0, SUPER_PENDING);

    pub const HW_LEGACY_INT_BASE: usize = 0xf0010000;
}

/// Precursor specific initialization.
pub fn init() {
    #[cfg(feature = "vexii-test")]
    // Map the interrupt manager shim
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                legacy_int::HW_LEGACY_INT_BASE as *mut u8,
                (LEGACY_INT_VMEM & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map legacy interrupt shim for vexii CPU")
    });

    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    self::uart::init();

    // handy routine for oscope debugging of up-ness of the CPU
    #[cfg(feature = "vexii-test")]
    let mut uart_csr = utralib::CSR::<u32>::new(0xffcf0000 as *mut u32);
    #[cfg(feature = "vexii-test")]
    for _ in 0..100 {
        uart_csr.wfo(utralib::utra::uart::RXTX_RXTX, 'b' as u32);
        while uart_csr.r(utralib::utra::uart::TXFULL) != 0 {}
    }

    self::rand::init();

    #[cfg(feature = "gdb-stub")]
    crate::debug::gdb::init();
}
