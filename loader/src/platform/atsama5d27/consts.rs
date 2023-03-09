// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::PAGE_SIZE;

/// Virtual memory location of stack pages for kernel and processes.
pub const USER_STACK_TOP: usize = 0x8000_0000;

pub const LOADER_CODE_ADDRESS: usize = 0x20000000;

pub const PAGE_TABLE_OFFSET: usize = 0xfefe_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const CONTEXT_OFFSET: usize = 0xff80_4000;
pub const USER_AREA_END: usize = 0xfe00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const KERNEL_LOAD_OFFSET: usize = 0xffd0_0000;
pub const KERNEL_STACK_TOP: usize = 0xfff8_0000;
pub const KERNEL_ARGUMENT_OFFSET: usize = 0xffc0_0000;
pub const GUARD_MEMORY_BYTES: usize = 2 * PAGE_SIZE;

// Allocate more pages for the kernel stacks
pub const KERNEL_STACK_PAGE_COUNT: usize = 4;

pub const FLG_VALID: usize = 0x1;
pub const FLG_X: usize = 0x8;
pub const FLG_W: usize = 0x4;
pub const FLG_R: usize = 0x2;
pub const FLG_U: usize = 0x10;
