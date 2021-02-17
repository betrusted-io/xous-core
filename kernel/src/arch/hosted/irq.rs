// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use xous_kernel::{TID, PID};

pub fn disable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_irq(_irq_no: usize) {
    unimplemented!();
}

pub unsafe fn set_isr_return_pair(_pid: PID, _ctx: TID) {
    unimplemented!()
}
