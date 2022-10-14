// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use xous_kernel::{PID, TID};

/// Disable external interrupts
pub fn disable_all_irqs() {
    todo!();
}

pub fn enable_irq(irq_no: usize) {
    todo!();
}

pub fn disable_irq(irq_no: usize) -> Result<(), xous_kernel::Error> {
    todo!();
}

pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) {
    todo!();
}

#[cfg(feature="gdb-stub")]
pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> {
    todo!();
}
