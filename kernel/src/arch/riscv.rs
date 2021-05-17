// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{satp, sie, sstatus};
use xous_kernel::PID;

pub mod exception;
pub mod irq;
pub mod mem;
pub mod process;
pub mod rand;
pub mod syscall;

pub use process::Thread;

pub fn current_pid() -> PID {
    PID::new(satp::read().asid() as _).unwrap()
}

pub fn init() {
    unsafe {
        sie::set_ssoft();
        sie::set_sext();
    }
    rand::init();
}

/// Put the core to sleep until an interrupt hits. Returns `true`
/// to indicate the kernel should not exit.
pub fn idle() -> bool {
    // Issue `wfi`. This will return as soon as an external interrupt
    // is available.
    unsafe { riscv::asm::wfi() };

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
