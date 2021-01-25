// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use super::process::Thread;
#[allow(dead_code)]
pub fn invoke(
    _context: &mut Thread,
    _supervisor: bool,
    _pc: usize,
    _sp: usize,
    _ret_addr: usize,
    _args: &[usize],
) {
    unimplemented!()
}
