// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{satp, sie, sstatus};

pub mod exception;
pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;
pub mod panic;

pub use process::Thread;

#[cfg(any(feature="precursor", feature="renode"))]
use utralib::generated::*;
#[cfg(any(feature="precursor", feature="renode"))]
use xous_kernel::{MemoryFlags, MemoryType, PID};
#[cfg(any(feature="precursor", feature="renode"))]
use crate::mem::MemoryManager;
#[cfg(any(feature="cramium-soc", feature="cramium-fpga"))]
use xous_kernel::PID;

#[cfg(any(feature="precursor", feature="renode"))]
pub const WFI_KERNEL: Wfi = Wfi {
    // the manually chosen virtual address has to be in the top 4MiB as it is the only page shared among all processes
    base: 0xffcd_0000 as *mut usize, // see https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
};

#[cfg(any(feature="precursor", feature="renode"))]
pub struct Wfi {
    pub base: *mut usize,
}

pub fn current_pid() -> PID {
    PID::new(satp::read().asid() as _).unwrap()
}

pub fn init() {
    #[cfg(any(feature="precursor", feature="renode"))]
    MemoryManager::with_mut(|memory_manager| {
        memory_manager
            .map_range(
                utra::wfi::HW_WFI_BASE as *mut u8,
                ((WFI_KERNEL.base as u32) & !4095) as *mut u8,
                4096,
                PID::new(1).unwrap(),
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map WFI")
    });
    #[cfg(any(feature="precursor", feature="renode"))]
    let mut wfi_kernel_csr = CSR::new(WFI_KERNEL.base as *mut u32);
    #[cfg(any(feature="precursor", feature="renode"))]
    wfi_kernel_csr.wfo(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, 1);

    unsafe {
        sie::set_ssoft();
        sie::set_sext();
    }
}

/// Put the core to sleep until an interrupt hits. Returns `true`
/// to indicate the kernel should not exit.
pub fn idle() -> bool {
    #[cfg(any(feature="precursor", feature="renode"))]
    let mut wfi_kernel_csr = CSR::new(WFI_KERNEL.base as *mut u32);

    // Issue `wfi`. This will return as soon as an external interrupt
    // is available.
    #[cfg(any(feature="cramium-fpga", feature="cramium-soc"))]
    // "traditional" path for stopping a clock
    unsafe { riscv::asm::wfi() };

    #[cfg(any(feature="precursor", feature="renode"))]
    {
        // this invokes Precusor-SoC specific path to gate clocks:
        // 1. ignore_locked prevents the chip from going into reset if the PLL goes unlocked
        wfi_kernel_csr.wfo(utra::wfi::IGNORE_LOCKED_IGNORE_LOCKED, 1);
        // 2. wfi gates all the clocks (stops them) until a SoC-defined interrupt comes in
        wfi_kernel_csr.wfo(utra::wfi::WFI_WFI, 1);
    }

    // Enable interrupts temporarily in Supervisor mode, allowing them
    // to drain. Aside from this brief instance, interrupts are
    // disabled when running in Supervisor mode.
    //
    // These interrupts are handled by userspace, so code execution will
    // immediately jump to the interrupt handler and return here after
    // all interrupts have been handled.
    unsafe {
        sstatus::set_sie();
        sstatus::clear_sie();
    };
    true
}
