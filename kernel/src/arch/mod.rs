// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_arch = "arm")]
mod arm;
#[cfg(target_arch = "arm")]
pub use crate::arch::arm::*;

#[cfg(any(windows, unix))]
mod hosted;
#[cfg(any(windows, unix))]
pub use hosted::*;

#[cfg(target_arch = "riscv32")]
mod riscv;
#[cfg(target_arch = "riscv32")]
pub use crate::arch::riscv::*;

#[cfg(all(target_arch = "riscv64", not(baremetal)))]
mod riscv;
#[cfg(all(target_arch = "riscv64", not(baremetal)))]
pub use riscv::*;

#[cfg(all(target_arch = "x86_64", not(any(windows, unix))))]
mod x86_64;
#[cfg(all(target_arch = "x86_64", not(any(windows, unix))))]
pub use x86_64::*;
