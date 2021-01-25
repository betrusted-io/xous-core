// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use core::slice;
use core::str;

pub use crate::arch::mem::{MemoryMapping, PAGE_SIZE};
use crate::arch::process::Process;

use xous_kernel::{MemoryFlags, MemoryRange, PID};

#[derive(Debug)]
enum ClaimOrRelease {
    Claim,
    Release,
}

#[repr(C)]
pub struct MemoryRangeExtra {
    mem_start: u32,
    mem_size: u32,
    mem_tag: u32,
    _padding: u32,
}

impl fmt::Display for MemoryRangeExtra {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag_name_bytes = self.mem_tag.to_le_bytes();
        let s = unsafe {
            // First, we build a &[u8]...
            let slice = slice::from_raw_parts(tag_name_bytes.as_ptr(), 4);
            // ... and then convert that slice into a string slice
            str::from_utf8_unchecked(slice)
        };

        write!(
            f,
            "{} ({:08x}) {:08x} - {:08x} {} bytes",
            s,
            self.mem_tag,
            self.mem_start,
            self.mem_start + self.mem_size,
            self.mem_size
        )
    }
}

pub struct MemoryManager {
    ram_start: usize,
    ram_size: usize,
    #[allow(dead_code)]
    ram_name: u32,
    #[allow(dead_code)]
    last_ram_page: usize,
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::default_hack()
    }
}

#[cfg(not(baremetal))]
std::thread_local!(static MEMORY_MANAGER: core::cell::RefCell<MemoryManager> = core::cell::RefCell::new(MemoryManager::default()));

#[cfg(baremetal)]
static mut MEMORY_MANAGER: MemoryManager = MemoryManager::default_hack();
#[cfg(baremetal)]
static mut MEMORY_ALLOCATIONS: &mut [Option<PID>] = &mut [];
#[cfg(baremetal)]
static mut EXTRA_REGIONS: &[MemoryRangeExtra] = &[];

/// Initialize the memory map.
/// This will go through memory and map anything that the kernel is
/// using to process 1, then allocate a pagetable for this process
/// and place it at the usual offset.  The MMU will not be enabled yet,
/// as the process entry has not yet been created.
impl MemoryManager {
    const fn default_hack() -> Self {
        MemoryManager {
            ram_start: 0,
            ram_size: 0,
            ram_name: 0,
            last_ram_page: 0,
        }
    }

    // /// Calls the provided function with the current inner process state.
    // pub fn with<F, R>(f: F) -> R
    // where
    //     F: FnOnce(&MemoryManager) -> R,
    // {
    //     #[cfg(baremetal)]
    //     unsafe {
    //         f(&MEMORY_MANAGER)
    //     }

    //     #[cfg(not(baremetal))]
    //     MEMORY_MANAGER.with(|ss| f(&ss.borrow()))
    // }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut MemoryManager) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut MEMORY_MANAGER)
        }

        #[cfg(not(baremetal))]
        MEMORY_MANAGER.with(|ss| f(&mut ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn init_from_memory(
        &mut self,
        base: *mut u32,
        args: &crate::args::KernelArguments,
    ) -> Result<(), xous_kernel::Error> {
        let mut args_iter = args.iter();
        let xarg_def = args_iter.next().expect("mm: no kernel arguments found");
        unsafe {
            assert!(
                EXTRA_REGIONS.is_empty(),
                "mm: self.extra.len() was {}, not 0",
                EXTRA_REGIONS.len()
            );
        }
        assert!(
            xarg_def.name == make_type!("XArg"),
            "mm: first tag wasn't XArg"
        );
        assert!(xarg_def.data[1] == 1, "mm: XArg had unexpected version");
        self.ram_start = xarg_def.data[2] as usize;
        self.ram_size = xarg_def.data[3] as usize;
        self.ram_name = xarg_def.data[4];

        let mut mem_size = self.ram_size / PAGE_SIZE;
        for tag in args_iter {
            if tag.name == make_type!("MREx") {
                unsafe {
                    assert!(
                        EXTRA_REGIONS.is_empty(),
                        "mm: MREx tag appears twice!  self.extra.len() is {}, not 0",
                        EXTRA_REGIONS.len()
                    );
                    let ptr = tag.data.as_ptr() as *mut MemoryRangeExtra;
                    EXTRA_REGIONS = slice::from_raw_parts_mut(
                        ptr,
                        tag.data.len() * 4 / core::mem::size_of::<MemoryRangeExtra>(),
                    )
                };
            }
        }

        unsafe {
            for range in EXTRA_REGIONS.iter() {
                mem_size += range.mem_size as usize / PAGE_SIZE;
            }
        }

        unsafe {
            MEMORY_ALLOCATIONS = slice::from_raw_parts_mut(base as *mut Option<PID>, mem_size)
        };
        Ok(())
    }

    #[cfg(all(baremetal, feature = "print-debug"))]
    pub fn print_ownership(&self) {
        println!("Ownership ({} bytes in all):", unsafe {
            MEMORY_ALLOCATIONS.len()
        });

        let mut offset = 0;
        unsafe {
            // First, we build a &[u8]...
            let name_bytes = self.ram_name.to_le_bytes();
            // ... and then convert that slice into a string slice
            let _ram_name = str::from_utf8_unchecked(&name_bytes);
            println!(
                "    Region {} ({:08x}) {:08x} - {:08x} {} bytes:",
                _ram_name,
                self.ram_name,
                self.ram_start,
                self.ram_start + self.ram_size,
                self.ram_size
            );
        };
        for o in 0..self.ram_size / PAGE_SIZE {
            unsafe {
                if let Some(_allocation) = MEMORY_ALLOCATIONS[offset + o] {
                    println!(
                        "        {:08x} => {}",
                        self.ram_size + o * PAGE_SIZE,
                        _allocation.get()
                    );
                }
            }
        }

        offset += self.ram_size / PAGE_SIZE;

        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        unsafe {
            for region in EXTRA_REGIONS {
                println!("    Region {}:", region);
                for o in 0..(region.mem_size as usize) / PAGE_SIZE {
                    if let Some(_allocation) = MEMORY_ALLOCATIONS[offset + o] {
                        println!(
                            "        {:08x} => {}",
                            (region.mem_start as usize) + o * PAGE_SIZE,
                            _allocation.get()
                        )
                    }
                }
                offset += region.mem_size as usize / PAGE_SIZE;
            }
        }
    }

    /// Allocate a single page to the given process. DOES NOT ZERO THE PAGE!!!
    /// This function CANNOT zero the page, as it hasn't been mapped yet.
    #[cfg(baremetal)]
    pub fn alloc_page(&mut self, pid: PID) -> Result<usize, xous_kernel::Error> {
        // Go through all RAM pages looking for a free page.
        // Optimization: start from the previous address.
        // println!("Allocating page for PID {}", pid);
        unsafe {
            for index in self.last_ram_page..((self.ram_size as usize) / PAGE_SIZE) {
                // println!("    Checking {:08x}...", index * PAGE_SIZE + self.ram_start as usize);
                if MEMORY_ALLOCATIONS[index].is_none() {
                    MEMORY_ALLOCATIONS[index] = Some(pid);
                    self.last_ram_page = index + 1;
                    let page = index * PAGE_SIZE + self.ram_start;
                    return Ok(page);
                }
            }
            for index in 0..self.last_ram_page {
                // println!("    Checking {:08x}...", index * PAGE_SIZE + self.ram_start as usize);
                if MEMORY_ALLOCATIONS[index].is_none() {
                    MEMORY_ALLOCATIONS[index] = Some(pid);
                    self.last_ram_page = index + 1;
                    let page = index * PAGE_SIZE + self.ram_start;
                    return Ok(page);
                }
            }
        }
        Err(xous_kernel::Error::OutOfMemory)
    }

    /// Find a virtual address in the current process that is big enough
    /// to fit `size` bytes.
    pub fn find_virtual_address(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        kind: xous_kernel::MemoryType,
    ) -> Result<*mut u8, xous_kernel::Error> {
        // If we were supplied a perfectly good address, return that.
        if virt_ptr as usize != 0 {
            return Ok(virt_ptr);
        }

        // let process = Process::current();
        Process::with_inner_mut(|process_inner| {
            let (start, end, initial) = match kind {
                xous_kernel::MemoryType::Stack => return Err(xous_kernel::Error::BadAddress),
                xous_kernel::MemoryType::Heap => {
                    let new_virt =
                        process_inner.mem_heap_base + process_inner.mem_heap_size + PAGE_SIZE;
                    if new_virt + size > process_inner.mem_heap_base + process_inner.mem_heap_max {
                        return Err(xous_kernel::Error::OutOfMemory);
                    }
                    return Ok(new_virt as *mut u8);
                }
                xous_kernel::MemoryType::Default => (
                    process_inner.mem_default_base,
                    process_inner.mem_default_base + 0x1000_0000,
                    process_inner.mem_default_last,
                ),
                xous_kernel::MemoryType::Messages => (
                    process_inner.mem_message_base,
                    process_inner.mem_message_base + 0x1000_0000,
                    process_inner.mem_message_last,
                ),
            };

            // Look for a sequence of `size` pages that are free.
            for potential_start in (initial..end - size).step_by(PAGE_SIZE) {
                // println!("    Checking {:08x}...", potential_start);
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => {
                            process_inner.mem_default_last = potential_start
                        }
                        xous_kernel::MemoryType::Messages => {
                            process_inner.mem_message_last = potential_start
                        }
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }

            for potential_start in (start..initial).step_by(PAGE_SIZE) {
                // println!("    Checking {:08x}...", potential_start);
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => {
                            process_inner.mem_default_last = potential_start
                        }
                        xous_kernel::MemoryType::Messages => {
                            process_inner.mem_message_last = potential_start
                        }
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }
            // println!("Couldn't find virtual address");
            Err(xous_kernel::Error::BadAddress)
        })
    }

    /// Reserve the given range without actually allocating memory.
    /// That way we can overpromise on stack size and heap size without
    /// needing to actually have pages to back it.
    pub fn reserve_range(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        flags: MemoryFlags,
    ) -> Result<xous_kernel::MemoryRange, xous_kernel::Error> {
        // If no address was specified, pick the next address that fits
        // in the "default" range
        let virt =
            self.find_virtual_address(virt_ptr, size, xous_kernel::MemoryType::Default)? as usize;

        if virt & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        if size & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let mut mm = MemoryMapping::current();
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            // FIXME: Un-reserve addresses if we encounter an error here
            mm.reserve_address(self, virt, flags)?;
        }
        Ok(xous_kernel::MemoryRange::new(virt_ptr as usize, size)?)
    }

    /// Attempt to allocate a single page from the default section.
    /// Note that this will be backed by a real page.
    #[cfg(baremetal)]
    pub fn map_zeroed_page(
        &mut self,
        pid: PID,
        is_user: bool,
    ) -> Result<*mut usize, xous_kernel::Error> {
        let virt = self.find_virtual_address(
            core::ptr::null_mut(),
            PAGE_SIZE,
            xous_kernel::MemoryType::Default,
        )? as usize;

        // Grab the next available page.  This claims it for this process.
        let phys = self.alloc_page(pid)?;

        // Actually perform the map.  At this stage, every physical page should be owned by us.
        if let Err(e) = crate::arch::mem::map_page_inner(
            self,
            pid,
            phys as usize,
            virt as usize,
            xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
            false,
        ) {
            self.release_page(phys as *mut usize, pid).ok();
            return Err(e);
        }

        let virt = virt as *mut usize;

        // Zero-out the page
        unsafe { virt.write_bytes(0, PAGE_SIZE / core::mem::size_of::<usize>()) };
        if is_user {
            crate::arch::mem::hand_page_to_user(virt as _)?;
        }
        println!(
            "Mapped {:08x} -> {:08x} (user? {})",
            phys as usize, virt as usize, is_user
        );
        Ok(virt)
    }

    pub fn is_main_memory(&self, phys: *mut u8) -> bool {
        (phys as usize) >= self.ram_start && (phys as usize) < self.ram_start + self.ram_size
    }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn map_range(
        &mut self,
        phys_ptr: *mut u8,
        virt_ptr: *mut u8,
        size: usize,
        pid: PID,
        flags: MemoryFlags,
        kind: xous_kernel::MemoryType,
    ) -> Result<xous_kernel::MemoryRange, xous_kernel::Error> {
        let phys = phys_ptr as usize;
        let virt = self.find_virtual_address(virt_ptr, size, kind)?;

        // If no physical address is specified, give the user the next available pages
        if phys == 0 {
            return self.reserve_range(virt, size, flags);
        }

        // 1. Attempt to claim all physical pages in the range
        for claim_phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
            if let Err(err) = self.claim_page(claim_phys as *mut usize, pid) {
                // If we were unable to claim one or more pages, release everything and return
                for rel_phys in (phys..claim_phys).step_by(PAGE_SIZE) {
                    self.release_page(rel_phys as *mut usize, pid).ok();
                }
                return Err(err);
            }
        }

        // Actually perform the map.  At this stage, every physical page should be owned by us.
        for offset in (0..size).step_by(PAGE_SIZE) {
            if let Err(e) = crate::arch::mem::map_page_inner(
                self,
                pid,
                offset + phys as usize,
                offset + virt as usize,
                flags,
                false,
            ) {
                for unmap_offset in (0..offset).step_by(PAGE_SIZE) {
                    crate::arch::mem::unmap_page_inner(self, unmap_offset + virt as usize).ok();
                    self.release_page((unmap_offset + phys) as *mut usize, pid)
                        .ok();
                }
                return Err(e);
            }
        }

        Ok(MemoryRange::new(virt as usize, size)?)
    }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn unmap_page(&mut self, virt: *mut usize) -> Result<usize, xous_kernel::Error> {
        let pid = crate::arch::process::current_pid();
        let phys = crate::arch::mem::virt_to_phys(virt as usize)?;
        self.release_page(phys as *mut usize, pid)?;
        crate::arch::mem::unmap_page_inner(self, virt as usize)
    }

    /// Move a page from one process into another, keeping its permissions.
    #[allow(dead_code)]
    pub fn move_page(
        &mut self,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
    ) -> Result<(), xous_kernel::Error> {
        crate::arch::mem::move_page_inner(
            self,
            &src_mapping,
            src_addr,
            dest_pid,
            &dest_mapping,
            dest_addr,
        )
    }

    /// Mark the page in the current process as being lent.  If the borrow is
    /// read-only, then additionally remove the "write" bit on it.  If the page
    /// is writable, then remove it from the current process until the borrow is
    /// returned.
    #[allow(dead_code)]
    pub fn lend_page(
        &mut self,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
        mutable: bool,
    ) -> Result<usize, xous_kernel::Error> {
        // If this page is to be writable, detach it from this process.
        // Otherwise, mark it as read-only to prevent a process from modifying
        // the page while it's borrowed.
        crate::arch::mem::lend_page_inner(
            self,
            &src_mapping,
            src_addr as _,
            dest_pid,
            &dest_mapping,
            dest_addr as _,
            mutable,
        )
    }

    /// Return the range from `src_mapping` back to `dest_mapping`
    #[allow(dead_code)]
    pub fn unlend_page(
        &mut self,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
    ) -> Result<usize, xous_kernel::Error> {
        // If this page is to be writable, detach it from this process.
        // Otherwise, mark it as read-only to prevent a process from modifying
        // the page while it's borrowed.
        crate::arch::mem::return_page_inner(
            self,
            &src_mapping,
            src_addr,
            dest_pid,
            &dest_mapping,
            dest_addr,
        )
    }

    /// Claim the given memory for the given process, or release the memory
    /// back to the free pool.
    #[cfg(not(baremetal))]
    fn claim_or_release(
        &mut self,
        _addr: *mut usize,
        _pid: PID,
        _action: ClaimOrRelease,
    ) -> Result<(), xous_kernel::Error> {
        Ok(())
    }

    #[cfg(baremetal)]
    fn claim_or_release(
        &mut self,
        addr: *mut usize,
        pid: PID,
        action: ClaimOrRelease,
    ) -> Result<(), xous_kernel::Error> {
        /// Modify the memory tracking table to note which process owns
        /// the specified address.
        fn action_inner(
            addr: &mut Option<PID>,
            pid: PID,
            action: ClaimOrRelease,
        ) -> Result<(), xous_kernel::Error> {
            if let Some(current_pid) = *addr {
                if current_pid != pid {
                    return Err(xous_kernel::Error::MemoryInUse);
                }
            }
            match action {
                ClaimOrRelease::Claim => {
                    *addr = Some(pid);
                }
                ClaimOrRelease::Release => {
                    *addr = None;
                }
            }
            Ok(())
        }
        let addr = addr as usize;

        // Ensure the address lies on a page boundary
        if addr & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let mut offset = 0;
        // Happy path: The address is in main RAM
        if addr >= self.ram_start && addr < self.ram_start + self.ram_size {
            offset += (addr - self.ram_start) / PAGE_SIZE;
            return unsafe { action_inner(&mut MEMORY_ALLOCATIONS[offset], pid, action) };
        }

        offset += self.ram_size / PAGE_SIZE;
        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        unsafe {
            for region in EXTRA_REGIONS {
                if addr >= (region.mem_start as usize)
                    && addr < (region.mem_start + region.mem_size) as usize
                {
                    offset += (addr - (region.mem_start as usize)) / PAGE_SIZE;
                    return action_inner(&mut MEMORY_ALLOCATIONS[offset], pid, action);
                }
                offset += region.mem_size as usize / PAGE_SIZE;
            }
        }
        // println!(
        //     "mem: unable to claim or release physical address {:08x}",
        //     addr
        // );
        Err(xous_kernel::Error::BadAddress)
    }

    /// Mark a given address as being owned by the specified process ID
    fn claim_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_or_release(addr, pid, ClaimOrRelease::Claim)
    }

    /// Mark a given address as no longer being owned by the specified process ID
    fn release_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_or_release(addr, pid, ClaimOrRelease::Release)
    }
}
