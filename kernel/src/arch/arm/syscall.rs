// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::services::Thread;

pub fn invoke(
    _thread: &mut Thread,
    _supervisor: bool,
    _pc: usize,
    _sp: usize,
    _ret_addr: usize,
    _args: &[usize],
) {
    todo!();
}

pub fn resume(_supervisor: bool, _thread: &Thread) -> ! {
    todo!();
}
