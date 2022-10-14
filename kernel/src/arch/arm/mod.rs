// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

pub mod exception;
pub mod irq;
pub mod mem;
pub mod process;
pub mod rand;
pub mod syscall;

pub use process::Thread;

use xous_kernel::PID;

pub fn current_pid() -> PID {
    todo!()
}

pub fn init() {
    todo!();
}

pub fn idle() -> bool {
    todo!();
}