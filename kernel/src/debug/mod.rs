// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
mod macros;
#[cfg(baremetal)]
pub mod shell;

#[cfg(all(baremetal, feature = "gdb-stub"))]
pub mod gdb;
