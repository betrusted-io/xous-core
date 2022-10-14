// SPDX-FileCopyrightText: 2022 Foundation Devices <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::mem::MemoryManager;
use xous_kernel::{MemoryFlags, PID};

pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const THREAD_CONTEXT_AREA: usize = 0xff80_1000;

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { };

pub const FLG_VALID: usize = 0x1;
pub const FLG_R: usize = 0x2;
pub const FLG_W: usize = 0x4;
// pub const FLG_X: usize = 0x8;
pub const FLG_U: usize = 0x10;
pub const FLG_A: usize = 0x40;
pub const FLG_D: usize = 0x80;

bitflags! {
    pub struct MMUFlags: usize {
        const NONE      = 0b00_0000_0000;
        const VALID     = 0b00_0000_0001;
        const R         = 0b00_0000_0010;
        const W         = 0b00_0000_0100;
        const X         = 0b00_0000_1000;
        const USER      = 0b00_0001_0000;
        const GLOBAL    = 0b00_0010_0000;
        const A         = 0b00_0100_0000;
        const D         = 0b00_1000_0000;
        const S         = 0b01_0000_0000; // Shared page
        const P         = 0b10_0000_0000; // Previously writable
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct MemoryMapping {}

impl MemoryMapping {
    pub unsafe fn from_raw(&mut self, _: usize) {
        todo!();
    }

    pub unsafe fn allocate(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    pub fn current() -> MemoryMapping {
        todo!();
    }

    pub fn get_pid(&self) -> PID {
        todo!();
    }

    pub fn is_kernel(&self) -> bool {
        todo!();
    }

    pub fn activate(self) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    pub fn print_map(&self) {
        todo!();
    }

    pub fn reserve_address(&mut self, mm: &mut MemoryManager, addr: usize, flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
        todo!();
    }
}

pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    todo!();
}

#[cfg(feature="gdb-stub")]
pub fn peek_memory<T>(addr: *mut T) -> Result<T, xous_kernel::Error> {
    todo!();
}

#[cfg(feature="gdb-stub")]
pub fn poke_memory<T>(addr: *mut T, val: T) -> Result<(), xous_kernel::Error> {
    todo!();
}

/// Map the given page to the specified process table.  If necessary,
/// allocate a new page.
///
/// # Errors
///
/// * OutOfMemory - Tried to allocate a new pagetable, but ran out of memory.
pub fn map_page_inner(
    mm: &mut MemoryManager,
    pid: PID,
    phys: usize,
    virt: usize,
    req_flags: MemoryFlags,
    map_user: bool,
) -> Result<(), xous_kernel::Error> {
    todo!();
}

/// Get the pagetable entry for a given address, or `Err()` if the address is invalid
pub fn pagetable_entry(addr: usize) -> Result<*mut usize, xous_kernel::Error> {
    todo!();
}

/// Ummap the given page from the specified process table.  Never allocate a new
/// page.
///
/// # Returns
///
/// The physical address for the page that was just unmapped
///
/// # Errors
///
/// * BadAddress - Address was not already mapped.
pub fn unmap_page_inner(_mm: &mut MemoryManager, virt: usize) -> Result<usize, xous_kernel::Error> {
    todo!();
}

/// Move a page from one address space to another.
pub fn move_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<(), xous_kernel::Error> {
    todo!();
}

/// Determine if a page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    todo!();
}

pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
    mutable: bool,
) -> Result<usize, xous_kernel::Error> {
    todo!();
}

/// Return a page from `src_space` back to `dest_space`.
pub fn return_page_inner(
    _mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    _dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<usize, xous_kernel::Error> {
    todo!();
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous_kernel::Error> {
    todo!();
}

pub fn ensure_page_exists_inner(address: usize) -> Result<usize, xous_kernel::Error> {
    todo!();
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    todo!();
}

/// Get the `MemoryFlags` for the requested virtual address. The address must
/// be valid and page-aligned, and must not be Shared.
///
/// # Returns
///
/// * **None**: The page is not valid or is shared
/// * **Some(MemoryFlags)**: The translated sharing permissions of the given flags
pub fn page_flags(virt: usize) -> Option<MemoryFlags> {
    todo!();
}

pub fn update_page_flags(virt: usize, flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
    todo!();
}
