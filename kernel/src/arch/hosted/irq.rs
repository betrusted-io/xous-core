// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use xous_kernel::{PID, TID};

pub fn enable_irq(_irq_no: usize) { unimplemented!() }

pub fn disable_irq(_irq_no: usize) { unimplemented!() }

pub unsafe fn set_isr_return_pair(_pid: PID, _ctx: TID) { unimplemented!() }
