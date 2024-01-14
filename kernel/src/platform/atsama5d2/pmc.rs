// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use atsama5d27::pmc::{PeripheralId, Pmc};
use utralib::HW_PMC_BASE;
use xous_kernel::{MemoryFlags, MemoryType, PID};

use crate::mem::MemoryManager;

/// The manually chosen virtual address has to be in the top 4MiB as it is the
/// only page shared among all processes.
///
/// See https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
pub const PMC_KERNEL_ADDR: usize = 0xffcc_0000;
pub static mut PMC_KERNEL: Option<PmcKernel> = None;

pub struct PmcKernel {
    base_addr: usize,
    pub inner: Option<Pmc>,
}

impl PmcKernel {
    pub fn new(addr: usize) -> PmcKernel { PmcKernel { base_addr: addr, inner: None } }

    pub fn init(&mut self) { self.inner = Some(Pmc::with_alt_base_addr(self.base_addr as u32)); }

    pub fn enable_peripheral_clock(&mut self, periph: PeripheralId) {
        if let Some(pmc) = &mut self.inner {
            return pmc.enable_peripheral_clock(periph);
        }

        unreachable!()
    }
}

/// Initialize PMC driver.
///
/// Needed so that the kernel can manage peripherals.
pub fn init() {
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                HW_PMC_BASE as *mut u8,
                (PMC_KERNEL_ADDR & !4095) as *mut u8,
                0x4000, // 16K
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W | MemoryFlags::DEV,
                MemoryType::Default,
            )
            .expect("unable to map PMC_KERNEL")
    });

    let mut pmc_kernel = PmcKernel::new(PMC_KERNEL_ADDR);
    pmc_kernel.init();

    unsafe {
        PMC_KERNEL = Some(pmc_kernel);
    }
}

/// Enables `TRNG` peripheral to make it usable.
pub fn enable_trng() {
    unsafe {
        PMC_KERNEL
            .as_mut()
            .expect("PMC_KERNEL driver not initialized")
            .enable_peripheral_clock(PeripheralId::Trng)
    }
}

/// Enables `AIC` peripheral to make it usable.
pub fn enable_aic() {
    unsafe {
        PMC_KERNEL
            .as_mut()
            .expect("PMC_KERNEL driver not initialized")
            .enable_peripheral_clock(PeripheralId::Aic)
    }
}

/// Enables `TC0` peripheral to make it usable.
pub fn enable_tc0() {
    unsafe {
        PMC_KERNEL
            .as_mut()
            .expect("PMC_KERNEL driver not initialized")
            .enable_peripheral_clock(PeripheralId::Tc0)
    }
}

/// Enables `LCDC` peripheral to make it usable.
pub fn enable_lcdc() {
    unsafe {
        PMC_KERNEL
            .as_mut()
            .expect("PMC_KERNEL driver not initialized")
            .enable_peripheral_clock(PeripheralId::Lcdc)
    }
}

/// Enables `PIOx` peripherals to make them usable.
pub fn enable_pio() {
    unsafe {
        let pmc = PMC_KERNEL.as_mut().expect("PMC_KERNEL driver not initialized");

        pmc.enable_peripheral_clock(PeripheralId::Pioa);
        pmc.enable_peripheral_clock(PeripheralId::Piob);
        pmc.enable_peripheral_clock(PeripheralId::Pioc);
        pmc.enable_peripheral_clock(PeripheralId::Piod);
    }
}
