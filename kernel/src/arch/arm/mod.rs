// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

pub mod exception;
pub mod irq;
pub mod mem;
pub mod panic;
pub mod process;
pub mod syscall;

use core::arch::asm;
use core::num::NonZeroU8;

pub use process::Thread;
use xous_kernel::PID;

pub fn current_pid() -> PID {
    let mut current_pid: usize;
    unsafe {
        asm!(
            "mrc p15, 0, {contextidr}, c13, c0, 1",
            contextidr = out(reg) current_pid,
        );

        assert_ne!(current_pid, 0, "Hardware PID is zero");

        NonZeroU8::new_unchecked((current_pid & 0xff) as u8)
    }
}

pub fn init() {
    unsafe {
        let pid = 1;
        let contextidr = (pid << 8) | pid;
        // Set initial (kernel) CONTEXTIDR
        asm!(
            "mcr p15, 0, {contextidr}, c13, c0, 1",
            contextidr = in(reg) contextidr,
        );
    }
}

pub fn idle() -> bool {
    // Ensure data and instruction reads are finished before WFI.
    // A NOP instruction after WFI is where an IRQ handler will jump back to
    unsafe { asm!("isb", "dsb", "wfi", "nop") }

    true
}
