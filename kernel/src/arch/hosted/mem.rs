pub const PAGE_SIZE: usize = 4096;
use crate::mem::MemoryManager;
use xous::{Error, MemoryFlags, PID};

pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;

#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct MemoryMapping {
    pid: usize,
}

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { pid: 0 };

impl MemoryMapping {
    pub unsafe fn from_raw(&mut self, satp: usize) {
        unimplemented!()
    }

    /// Get the currently active memory mapping.  Note that the actual root pages
    /// may be found at virtual address `PAGE_TABLE_ROOT_OFFSET`.
    pub fn current() -> MemoryMapping {
        unimplemented!()
    }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> PID {
        unimplemented!()
    }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(&self) {
        unimplemented!()
    }

    pub fn reserve_address(
        &mut self,
        mm: &mut MemoryManager,
        addr: usize,
        flags: MemoryFlags,
    ) -> Result<(), Error> {
        unimplemented!()
    }
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    unimplemented!()
}

pub fn map_page_inner(
    mm: &mut MemoryManager,
    pid: PID,
    phys: usize,
    virt: usize,
    req_flags: MemoryFlags,
    map_user: bool,
) -> Result<(), xous::Error> {
    unimplemented!()
}

pub fn move_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut usize,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut usize,
) -> Result<(), Error> {
    unimplemented!()
}

pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut usize,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut usize,
    mutable: bool,
) -> Result<usize, Error> {
    unimplemented!()
}

pub fn return_page_inner(
    _mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut usize,
    _dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut usize,
) -> Result<usize, Error> {
    unimplemented!()
}

pub fn unmap_page_inner(_mm: &mut MemoryManager, virt: usize) -> Result<usize, Error> {
    unimplemented!()
}

pub fn hand_page_to_user(virt: *mut usize) -> Result<(), Error> {
    unimplemented!()
}

pub fn virt_to_phys(virt: usize) -> Result<usize, Error> {
    unimplemented!()
}
