// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

use xous_kernel::{MemoryFlags, MemoryRange, PID};

pub use crate::arch::mem::{MemoryMapping, PAGE_SIZE};
use crate::arch::process::Process;
#[cfg(feature = "swap")]
use crate::swap::SwapAlloc;

#[derive(Debug)]
// below suppresses warning from unused Move argument in hosted mode
#[allow(dead_code)]
enum ClaimReleaseMove {
    Claim,
    Release,
    Move(PID /* from */),
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
        write!(
            f,
            "{}{}{}{} - ({:08x}) {:08x} - {:08x} {} bytes",
            ((self.mem_tag) & 0xff) as u8 as char,
            ((self.mem_tag >> 8) & 0xff) as u8 as char,
            ((self.mem_tag >> 16) & 0xff) as u8 as char,
            ((self.mem_tag >> 24) & 0xff) as u8 as char,
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
    fn default() -> Self { Self::default_hack() }
}

#[cfg(not(baremetal))]
std::thread_local!(static MEMORY_MANAGER: core::cell::RefCell<MemoryManager> = core::cell::RefCell::new(MemoryManager::default()));

#[cfg(baremetal)]
static mut MEMORY_MANAGER: MemoryManager = MemoryManager::default_hack();
#[cfg(baremetal)]
#[cfg(feature = "swap")]
static mut MEMORY_ALLOCATIONS: &mut [SwapAlloc] = &mut [];
#[cfg(baremetal)]
#[cfg(not(feature = "swap"))]
static mut MEMORY_ALLOCATIONS: &mut [Option<PID>] = &mut [];
#[cfg(baremetal)]
static mut EXTRA_ALLOCATIONS: &mut [Option<PID>] = &mut [];
#[cfg(baremetal)]
static mut EXTRA_REGIONS: &[MemoryRangeExtra] = &[];

#[cfg(feature = "swap")]
/// Epoch rollover handling -- two options:
///   - Fast epoch rollover, but long-lasting performance impact: just reset all counters to 0, and let the
///     system re-discover LRU order based on usage patterns again
///   - Slow epoch rollover, with no performance impact: go through all pages and "compact" the count down to
///     the lowest level, resetting the epoch counter to the next available epoch. LRU patterns are
///     maintained, but the search could take a long time.
///
/// In this implementation, we go with the slow epoch rollover, under the theory that (a) the
/// actual size of the list to normalize is "small" (few hundred to thousands of
/// entries) compared to the cost of re-discovering the LRU order by swapping out (e.g.
/// encrypting and decrypting) 4096-byte pages. The implementation itself could be more
/// efficient if we could, for example, make a full copy of the table, but we don't
/// want to blow out the kernel stack or allocate a static structure for temporary
/// data, so the entire algorithm is implemented with about a dozen entries of scratch
/// space, iterating over the list until every item is renormalized.
///
/// The code is in the `mem` crate instead of `swap`, because it has to directly access the locally scoped
/// MEM_ALLOCATIONS variable. The loop is likewise marked unsafe, because it accesses this static mut.
pub unsafe fn renormalize_allocs() -> u32 {
    #[cfg(feature = "debug-swap-verbose")]
    for (i, alloc) in MEMORY_ALLOCATIONS.iter().enumerate() {
        if alloc.get_raw_vpn() != 0 || alloc.get_timestamp() != 0 {
            println!("  {:x}: {:08x}[{:x}]", i, alloc.get_raw_vpn(), alloc.get_timestamp());
        }
    }
    // min_search_limit tracks the window of values we can renormalize in a single pass.
    // It is guaranteed to increase by exactly RENORM_PASS_SIZE every pass, except for the final pass.
    let mut min_search_limit = 0;
    loop {
        let mut ms = crate::utils::MinSet::new();
        for d in MEMORY_ALLOCATIONS.iter() {
            if d.get_timestamp() >= min_search_limit {
                ms.insert(d.get_timestamp());
            }
        }
        // remap elements in d that match the minset
        for d in MEMORY_ALLOCATIONS.iter_mut() {
            if let Some(i) = ms.index_of(d.get_timestamp()) {
                d.set_timestamp(i as u32 + min_search_limit);
            }
        }
        min_search_limit += crate::utils::RENORM_PASS_SIZE as u32;
        if ms.max() == u32::MAX {
            break;
        }
    }
    #[cfg(feature = "debug-swap-verbose")]
    for (i, alloc) in MEMORY_ALLOCATIONS.iter().enumerate() {
        if alloc.get_raw_vpn() != 0 || alloc.get_timestamp() != 0 {
            println!("  {:x}: {:08x}[{:x}]", i, alloc.get_raw_vpn(), alloc.get_timestamp());
        }
    }
    MEMORY_ALLOCATIONS.iter().max().unwrap().get_timestamp()
}

/// Initialize the memory map.
/// This will go through memory and map anything that the kernel is
/// using to process 1, then allocate a pagetable for this process
/// and place it at the usual offset.  The MMU will not be enabled yet,
/// as the process entry has not yet been created.
impl MemoryManager {
    const fn default_hack() -> Self {
        MemoryManager { ram_start: 0, ram_size: 0, ram_name: 0, last_ram_page: 0 }
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
            f(&mut *core::ptr::addr_of_mut!(MEMORY_MANAGER))
        }

        #[cfg(not(baremetal))]
        MEMORY_MANAGER.with(|ss| f(&mut ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&MemoryManager) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&*core::ptr::addr_of!(MEMORY_MANAGER))
        }

        #[cfg(not(baremetal))]
        MEMORY_MANAGER.with(|ss| f(&ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn init_from_memory(
        &mut self,
        rpt_base: usize,
        xpt_base: usize,
        args: &crate::args::KernelArguments,
    ) -> Result<(), xous_kernel::Error> {
        use core::slice;
        let mut args_iter = args.iter();
        let xarg_def = args_iter.next().expect("mm: no kernel arguments found");
        unsafe {
            assert!(EXTRA_REGIONS.is_empty(), "mm: self.extra.len() was {}, not 0", EXTRA_REGIONS.len());
        }
        assert!(xarg_def.name == u32::from_le_bytes(*b"XArg"), "mm: first tag wasn't XArg");
        assert!(xarg_def.data[1] == 1, "mm: XArg had unexpected version");
        self.ram_start = xarg_def.data[2] as usize;
        self.ram_size = xarg_def.data[3] as usize;
        self.ram_name = xarg_def.data[4];

        let mem_size = self.ram_size / PAGE_SIZE;
        let mut extra_size = 0;
        for tag in args_iter {
            if tag.name == u32::from_le_bytes(*b"MREx") {
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
                extra_size += range.mem_size as usize / PAGE_SIZE;
            }
        }
        #[cfg(feature = "swap")]
        unsafe {
            MEMORY_ALLOCATIONS = slice::from_raw_parts_mut(rpt_base as *mut SwapAlloc, mem_size);
            crate::swap::Swap::with_mut(|s| {
                s.init_rpt(
                    rpt_base,
                    ((mem_size * core::mem::size_of::<loader::swap::SwapAlloc>() + PAGE_SIZE - 1)
                        & !(PAGE_SIZE - 1))
                        / PAGE_SIZE,
                )
            });
        }
        #[cfg(not(feature = "swap"))]
        unsafe {
            MEMORY_ALLOCATIONS = slice::from_raw_parts_mut(rpt_base as *mut Option<PID>, mem_size)
        };
        unsafe { EXTRA_ALLOCATIONS = slice::from_raw_parts_mut(xpt_base as *mut Option<PID>, extra_size) }
        Ok(())
    }

    /// Print the number of RAM bytes used by the specified process.
    /// This does not include memory such as peripherals and CSRs.
    #[cfg(baremetal)]
    pub fn ram_used_by(&self, pid: PID) -> usize {
        let mut owned_bytes = 0;
        #[cfg(baremetal)]
        #[cfg(not(feature = "swap"))]
        unsafe {
            for owner in &MEMORY_ALLOCATIONS[0..self.ram_size / PAGE_SIZE] {
                if owner == &Some(pid) {
                    owned_bytes += PAGE_SIZE;
                }
            }
        }
        #[cfg(baremetal)]
        #[cfg(feature = "swap")]
        unsafe {
            for owner in &MEMORY_ALLOCATIONS[0..self.ram_size / PAGE_SIZE] {
                if owner.is_pid(pid) {
                    owned_bytes += PAGE_SIZE;
                }
            }
        }
        owned_bytes
    }

    #[cfg(all(baremetal, feature = "print-debug"))]
    pub fn print_ownership(&self) {
        println!("Ownership ({} bytes in all):", unsafe {
            MEMORY_ALLOCATIONS.len() + EXTRA_ALLOCATIONS.len()
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
                if MEMORY_ALLOCATIONS[offset + o].is_some() {
                    println!("        {:08x} => {}", self.ram_size + o * PAGE_SIZE, _allocation.get());
                }
            }
        }

        offset = 0;

        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        unsafe {
            for region in EXTRA_REGIONS {
                println!("    Region {}:", region);
                for o in 0..(region.mem_size as usize) / PAGE_SIZE {
                    if EXTRA_ALLOCATIONS[offset + o].is_some() {
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
    #[cfg(not(feature = "swap"))]
    pub fn alloc_page(&mut self, pid: PID) -> Result<usize, xous_kernel::Error> {
        // Go through all RAM pages looking for a free page.
        // println!("Allocating page for PID {}", pid);
        unsafe {
            let end_point = self.ram_size / PAGE_SIZE;
            let starting_point = self.last_ram_page.max(end_point);
            for (allocation, index) in MEMORY_ALLOCATIONS[starting_point..end_point]
                .iter_mut()
                .zip(starting_point..)
                .chain(MEMORY_ALLOCATIONS[..starting_point].iter_mut().zip(0..))
            {
                if allocation.is_none() {
                    *allocation = Some(pid);
                    self.last_ram_page = index + 1;
                    let page = index * PAGE_SIZE + self.ram_start;
                    return Ok(page);
                }
            }
        }
        Err(xous_kernel::Error::OutOfMemory)
    }

    #[cfg(baremetal)]
    #[cfg(feature = "swap")]
    /// This is an infalliable alloc_page, but, the number of cases where this may be called is small:
    ///   - mapping an L1 page table page
    ///   - creating a process
    /// As long the swapper manages to keep a handful of pages free, we should be able to absorb this
    /// so long as we don't do something like spawn a bazillion processes in a single quantum.
    pub fn alloc_page(&mut self, pid: PID, vaddr: Option<usize>) -> Result<usize, xous_kernel::Error> {
        // Go through all RAM pages looking for a free page.
        // println!("Allocating page for PID {}", pid);
        unsafe {
            let end_point = self.ram_size / PAGE_SIZE;
            let starting_point = self.last_ram_page.max(end_point);
            for (allocation, index) in MEMORY_ALLOCATIONS[starting_point..end_point]
                .iter_mut()
                .zip(starting_point..)
                .chain(MEMORY_ALLOCATIONS[..starting_point].iter_mut().zip(0..))
            {
                if allocation.is_none() {
                    allocation.update(Some(pid), vaddr);
                    self.last_ram_page = index + 1;
                    let page = index * PAGE_SIZE + self.ram_start;
                    return Ok(page);
                }
            }
        }
        Err(xous_kernel::Error::OutOfMemory)
    }

    #[cfg(baremetal)]
    #[cfg(feature = "swap")]
    // Take a physical address and indicate that it's been queried so it doesn't get suggested as LRU any time
    // soon. Addresses outside of RAM are just ignored.
    pub fn touch(&self, paddr: usize) {
        if paddr >= self.ram_start && paddr < self.ram_start + self.ram_size {
            unsafe {
                MEMORY_ALLOCATIONS[(paddr - self.ram_start) / PAGE_SIZE].touch();
            }
        }
    }

    #[cfg(baremetal)]
    #[cfg(feature = "swap")]
    pub fn memory_size(&self) -> usize { self.ram_size }

    #[cfg(feature = "swap")]
    pub fn rpt_base(&self) -> usize { unsafe { MEMORY_ALLOCATIONS.as_ptr() as usize } }

    #[cfg(feature = "debug-swap")]
    /// This function is "improper" in that it returns a bogus value if the memory allocations are
    /// out of range, but its purpose is only for debugging. This is not suitable for use in any
    /// other context.
    pub unsafe fn get_timestamp(&self, paddr: usize) -> u32 {
        if paddr >= self.ram_start && paddr < self.ram_start + self.ram_size {
            unsafe { MEMORY_ALLOCATIONS[(paddr - self.ram_start) / PAGE_SIZE].get_timestamp() }
        } else {
            0xDEAD_BEEF
        }
    }

    #[cfg(baremetal)]
    #[cfg(feature = "swap")]
    /// Similar to alloc_page(), but this implementation can only be called in one location because
    /// we need to know where to resume from after the OOM is recovered.
    pub fn alloc_page_oomable(&mut self, pid: PID, vaddr: usize) -> Result<usize, xous_kernel::Error> {
        // Go through all RAM pages looking for a free page.
        // println!("Allocating page for PID {}", pid);
        unsafe {
            let end_point = self.ram_size / PAGE_SIZE;
            let starting_point = self.last_ram_page.max(end_point);
            for (allocation, index) in MEMORY_ALLOCATIONS[starting_point..end_point]
                .iter_mut()
                .zip(starting_point..)
                .chain(MEMORY_ALLOCATIONS[..starting_point].iter_mut().zip(0..))
            {
                if allocation.is_none() {
                    allocation.update(Some(pid), Some(vaddr));
                    self.last_ram_page = index + 1;
                    let page = index * PAGE_SIZE + self.ram_start;
                    return Ok(page);
                }
            }
        }
        crate::swap::Swap::with_mut(|s| {
            s.hard_oom(vaddr);
        });
        // the call above actually diverges -- the final path will actually depend on how much memory
        // could be freed by the swapper.
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
        if !virt_ptr.is_null() {
            return Ok(virt_ptr);
        }

        // let process = Process::current();
        Process::with_inner_mut(|process_inner| {
            let (start, end, initial) = match kind {
                xous_kernel::MemoryType::Stack => return Err(xous_kernel::Error::BadAddress),
                xous_kernel::MemoryType::Heap => {
                    let new_virt = process_inner.mem_heap_base + process_inner.mem_heap_size + PAGE_SIZE;
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
                    process_inner.mem_message_base + 0x40_0000, // Limit to one superpage
                    process_inner.mem_message_last,
                ),
            };

            // Look for a sequence of `size` pages that are free.
            for potential_start in (initial..end - size).step_by(PAGE_SIZE) {
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => process_inner.mem_default_last = potential_start,
                        xous_kernel::MemoryType::Messages => process_inner.mem_message_last = potential_start,
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }

            for potential_start in (start..initial).step_by(PAGE_SIZE) {
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => process_inner.mem_default_last = potential_start,
                        xous_kernel::MemoryType::Messages => process_inner.mem_message_last = potential_start,
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }
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
        let virt = self.find_virtual_address(virt_ptr, size, xous_kernel::MemoryType::Default)? as usize;

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
        unsafe { xous_kernel::MemoryRange::new(virt_ptr as usize, size) }
    }

    /// Attempt to allocate a single page from the default section.
    /// Note that this will be backed by a real page.
    #[cfg(baremetal)]
    pub fn map_zeroed_page(&mut self, pid: PID, is_user: bool) -> Result<*mut usize, xous_kernel::Error> {
        let virt =
            self.find_virtual_address(core::ptr::null_mut(), PAGE_SIZE, xous_kernel::MemoryType::Default)?
                as usize;

        // Grab the next available page.  This claims it for this process.
        #[cfg(not(feature = "swap"))]
        let phys = self.alloc_page(pid)?;
        #[cfg(feature = "swap")]
        // this should not be OOMable because the callers for map_zeroed page are exclusively from kernel
        // services (process creation, queue creation)
        let phys = self.alloc_page(pid, Some(virt))?;

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
        let range_start = virt;
        let range_end = range_start.wrapping_add(PAGE_SIZE / core::mem::size_of::<usize>());
        unsafe {
            crate::mem::bzero(range_start, range_end);
        };
        if is_user {
            crate::arch::mem::hand_page_to_user(virt as _)?;
        }
        // klog!(
        //     "Mapped {:08x} -> {:08x} (user? {})",
        //     phys as usize, virt as usize, is_user
        // );
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
                    self.release_page((unmap_offset + phys) as *mut usize, pid).ok();
                }
                return Err(e);
            }
        }

        unsafe { MemoryRange::new(virt as usize, size) }
    }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn unmap_page(&mut self, virt: *mut usize) -> Result<usize, xous_kernel::Error> {
        let pid = crate::arch::process::current_pid();

        // If the virtual address has an assigned physical address, release that
        // address from this process.
        if let Ok(phys) = crate::arch::mem::virt_to_phys(virt as usize) {
            self.release_page(phys as *mut usize, pid).ok();
        }

        // Free the virtual address.
        crate::arch::mem::unmap_page_inner(self, virt as usize)
    }

    /// Move a page from one process into another, keeping its permissions.
    #[allow(dead_code)]
    pub fn move_page(
        &mut self,
        src_pid: PID,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
    ) -> Result<(), xous_kernel::Error> {
        let phys_addr = crate::arch::mem::virt_to_phys(src_addr as usize)?;
        crate::arch::mem::move_page_inner(self, src_mapping, src_addr, dest_pid, dest_mapping, dest_addr)?;
        self.claim_release_move(phys_addr as *mut usize, dest_pid, ClaimReleaseMove::Move(src_pid))
    }

    #[allow(dead_code)]
    /// Move the page in the process mapping listing without manipulating
    /// the pagetables at all.
    pub fn move_page_raw(&mut self, phys_addr: *mut usize, dest_pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(
            phys_addr as *mut usize,
            dest_pid,
            ClaimReleaseMove::Move(crate::arch::process::current_pid()),
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
            src_mapping,
            src_addr as _,
            dest_pid,
            dest_mapping,
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
        crate::arch::mem::return_page_inner(self, src_mapping, src_addr, dest_pid, dest_mapping, dest_addr)
    }

    #[cfg(baremetal)]
    pub fn ensure_page_exists(&mut self, address: usize) -> Result<(), xous_kernel::Error> {
        crate::arch::mem::ensure_page_exists_inner(address).and(Ok(()))
    }

    /// Claim the given memory for the given process, or release the memory
    /// back to the free pool.
    #[cfg(not(baremetal))]
    fn claim_release_move(
        &mut self,
        _addr: *mut usize,
        _pid: PID,
        _action: ClaimReleaseMove,
    ) -> Result<(), xous_kernel::Error> {
        Ok(())
    }

    #[cfg(baremetal)]
    fn claim_release_move(
        &mut self,
        addr: *mut usize,
        pid: PID,
        action: ClaimReleaseMove,
    ) -> Result<(), xous_kernel::Error> {
        /// Modify the memory tracking table to note which process owns
        /// the specified address.
        fn action_inner(
            owner_addr: &mut Option<PID>,
            pid: PID,
            action: ClaimReleaseMove,
        ) -> Result<(), xous_kernel::Error> {
            if let Some(current_pid) = *owner_addr {
                if current_pid != pid {
                    // klog!(
                    //     "In claim_or_release({}, {}, {:?}) -- addr is owned by {} not {}",
                    //     owner_addr.map(|v| v.get()).unwrap_or_default(),
                    //     pid,
                    //     action,
                    //     current_pid,
                    //     pid
                    // );
                    if let ClaimReleaseMove::Move(existing_pid) = action {
                        if existing_pid != current_pid {
                            return Err(xous_kernel::Error::MemoryInUse);
                        }
                    } else {
                        return Err(xous_kernel::Error::MemoryInUse);
                    }
                }
            }
            match action {
                ClaimReleaseMove::Claim | ClaimReleaseMove::Move(_) => {
                    *owner_addr = Some(pid);
                }
                ClaimReleaseMove::Release => {
                    *owner_addr = None;
                }
            }
            Ok(())
        }
        #[cfg(feature = "swap")]
        fn action_inner_tracking(
            owner_addr: &mut SwapAlloc,
            pid: PID,
            action: ClaimReleaseMove,
            addr: usize,
        ) -> Result<(), xous_kernel::Error> {
            if let Some(current_pid) = owner_addr.get_pid() {
                if current_pid != pid {
                    // klog!(
                    //     "In claim_or_release({}, {}, {:?}) -- addr is owned by {} not {}",
                    //     owner_addr.map(|v| v.get()).unwrap_or_default(),
                    //     pid,
                    //     action,
                    //     current_pid,
                    //     pid
                    // );
                    if let ClaimReleaseMove::Move(existing_pid) = action {
                        if existing_pid != current_pid {
                            return Err(xous_kernel::Error::MemoryInUse);
                        }
                    } else {
                        println!("ERR: {:?} != {:?}", current_pid, pid);
                        return Err(xous_kernel::Error::MemoryInUse);
                    }
                }
            }
            match action {
                ClaimReleaseMove::Claim | ClaimReleaseMove::Move(_) => {
                    unsafe { owner_addr.update(Some(pid), Some(addr)) };
                }
                ClaimReleaseMove::Release => {
                    unsafe { owner_addr.update(None, None) };
                }
            }
            Ok(())
        }

        let addr = addr as usize;

        // Ensure the address lies on a page boundary
        if cfg!(baremetal) && addr & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let mut offset = 0;
        // Happy path: The address is in main RAM
        if addr >= self.ram_start && addr < self.ram_start + self.ram_size {
            offset += (addr - self.ram_start) / PAGE_SIZE;
            #[cfg(not(feature = "swap"))]
            return unsafe { action_inner(&mut MEMORY_ALLOCATIONS[offset], pid, action) };
            #[cfg(feature = "swap")]
            return unsafe { action_inner_tracking(&mut MEMORY_ALLOCATIONS[offset], pid, action, addr) };
        }

        offset = 0;
        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        unsafe {
            for region in EXTRA_REGIONS {
                if addr >= (region.mem_start as usize) && addr < (region.mem_start + region.mem_size) as usize
                {
                    // -------------------------------
                    // FIXME: workaround to allow to share the same UART peripheral
                    //        between the kernel and xous-log processes
                    #[cfg(feature = "atsama5d27")]
                    {
                        let uart_base = crate::platform::atsama5d2::uart::HW_UART_BASE as usize;
                        if pid.get() != 1
                            && (addr == uart_base
                                || addr == uart_base + 0x1000
                                || addr == uart_base + 0x2000
                                || addr == uart_base + 0x3000)
                        {
                            klog!("[!] UART sharing workaround used for {:08x} address", addr);
                            return Ok(());
                        }
                    }
                    // -------------------------------

                    offset += (addr - (region.mem_start as usize)) / PAGE_SIZE;
                    return action_inner(&mut EXTRA_ALLOCATIONS[offset], pid, action);
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
        self.claim_release_move(addr, pid, ClaimReleaseMove::Claim)
    }

    /// Mark a given address as no longer being owned by the specified process ID
    fn release_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Release)
    }

    #[cfg(feature = "swap")]
    /// Same as `release_page`, but with public visibility when the `swap` feature is active
    pub fn release_page_swap(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Release)
    }

    /// Convert an offset in the `MEMORY_ALLOCATIONS` array into a physical address.
    #[cfg(baremetal)]
    fn allocation_offset_to_address(&self, offset: usize) -> Option<usize> {
        // If the offset is within the RAM size, simply turn it into
        // an address.
        if offset < self.ram_size as usize / PAGE_SIZE {
            Some(self.ram_start as usize + offset * PAGE_SIZE)
        } else {
            // No region was found.
            None
        }
    }

    /// Convert an offset in the `EXTRA_ALLOCATIONS` array into a physical address.
    #[cfg(baremetal)]
    fn allocation_offset_to_address_extra(&self, offset: usize) -> Option<usize> {
        // Loop through all regions looking for the address.
        // NOTE: This needs to be linear because each memory region has a different length.
        let mut offset_in_region = offset;
        unsafe {
            for region in EXTRA_REGIONS {
                // If the offset exceeds the current region, skip to the
                // next region.
                if offset_in_region >= (region.mem_size as usize / PAGE_SIZE) {
                    offset_in_region -= region.mem_size as usize / PAGE_SIZE;
                    continue;
                }
                return Some(region.mem_start as usize + (offset_in_region * PAGE_SIZE));
            }
        }

        // No region was found.
        None
    }

    /// Free all memory that belongs to a process. This does not unmap the
    /// memory from the process, it only marks it as free.
    /// This is very unsafe because the memory can immediately be re-allocated
    /// to another process, so only call this as part of destroying a process.
    pub unsafe fn release_all_memory_for_process(&mut self, _pid: PID) {
        // release the main memory allocs
        #[cfg(baremetal)]
        for (idx, owner) in MEMORY_ALLOCATIONS.iter_mut().enumerate() {
            // If this address has been allocated to this process, consider
            // freeing it or reparenting it.
            #[cfg(not(feature = "swap"))]
            if owner == &mut Some(_pid) {
                let phys_addr = self.allocation_offset_to_address(idx).unwrap();
                if crate::arch::mem::page_is_lent(phys_addr as *mut u8) {
                    // If the page is lent, reparent it to PID 1 so it will
                    // get freed when it is returned.
                    *owner = PID::new(1);
                } else {
                    // Mark this page as free, which allows it to be re-allocated.
                    *owner = None;
                }
            }
            #[cfg(feature = "swap")]
            if owner.is_pid(_pid) {
                let phys_addr = self.allocation_offset_to_address(idx).unwrap();
                if crate::arch::mem::page_is_lent(phys_addr as *mut u8) {
                    // If the page is lent, reparent it to PID 1 so it will
                    // get freed when it is returned.
                    owner.reparent(PID::new(1).unwrap());
                } else {
                    // Mark this page as free, which allows it to be re-allocated.
                    owner.update(None, None);
                }
            }
        }
        // release the extra allocs
        #[cfg(baremetal)]
        for (idx, owner) in EXTRA_ALLOCATIONS.iter_mut().enumerate() {
            if owner == &mut Some(_pid) {
                let phys_addr = self.allocation_offset_to_address_extra(idx).unwrap();
                if crate::arch::mem::page_is_lent(phys_addr as *mut u8) {
                    // If the page is lent, reparent it to PID 1 so it will
                    // get freed when it is returned.
                    *owner = PID::new(1);
                } else {
                    // Mark this page as free, which allows it to be re-allocated.
                    *owner = None;
                }
            }
        }
    }

    /// Adjust the flags on the given memory range. This allows for stripping flags from a memory
    /// range but does not allow adding flags. The memory range must exist, and the flags must be valid.
    pub fn update_memory_flags(
        &mut self,
        range: MemoryRange,
        flags: MemoryFlags,
    ) -> Result<(), xous_kernel::Error> {
        let virt = range.as_mut_ptr() as usize;
        let size = range.len();
        if virt & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        if size & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        // Pre-check the range to ensure the new flags are valid
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            let existing_flags = crate::arch::mem::page_flags(virt).ok_or(xous_kernel::Error::MemoryInUse)?;
            // If the new flags add to the range, return an error.
            if !(!existing_flags & flags).is_empty() {
                return Err(xous_kernel::Error::MemoryInUse);
            }
        }

        // Now that the flags are validated, perform the update. This is fine as long as
        // we're unicore.
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            let existing_flags = crate::arch::mem::page_flags(virt).ok_or(xous_kernel::Error::MemoryInUse)?;
            // If the new flags add to the range, return an error.
            if !(!existing_flags & flags).is_empty() {
                return Err(xous_kernel::Error::MemoryInUse);
            }

            crate::arch::mem::update_page_flags(virt, flags)?;
        }

        Ok(())
    }

    #[cfg(all(baremetal, target_arch = "riscv32"))]
    pub fn check_for_duplicates(&self) {
        use crate::services::SystemServices;

        SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Activate the debugging process and iterate through it,
            // noting down each active thread.
            for phys in (self.ram_start..self.ram_start + self.ram_size).step_by(PAGE_SIZE) {
                let mut owner = None;
                for pid in 1..crate::services::MAX_PROCESS_COUNT {
                    let pid = PID::new(pid as u8).unwrap();
                    let Ok(process) = system_services.get_process(pid) else {
                        continue;
                    };
                    let Ok(_) = process.activate() else {
                        continue;
                    };
                    match MemoryMapping::current().phys_to_virt(phys) {
                        Err(e) => {
                            println!("!!! ERROR {:?} !!!", e);
                            continue;
                        }
                        Ok(None) => continue,
                        Ok(Some(virt)) => {
                            let allocation_offset = (phys - self.ram_start) / PAGE_SIZE;
                            let existing_owner = unsafe { &MEMORY_ALLOCATIONS[allocation_offset] };
                            #[cfg(feature = "swap")]
                            let eo = &existing_owner.get_pid();
                            #[cfg(not(feature = "swap"))]
                            let eo = existing_owner;
                            if eo != &Some(pid) {
                                let is_lent = {
                                    if let Some(existing_owner) = eo {
                                        system_services
                                            .get_process(*existing_owner)
                                            .unwrap()
                                            .activate()
                                            .unwrap();
                                        let is_lent = if let Ok(Some(owned_address)) =
                                            MemoryMapping::current().phys_to_virt(phys)
                                        {
                                            crate::arch::mem::page_is_lent(owned_address as *mut u8)
                                        } else {
                                            false
                                        };
                                        system_services.get_process(pid).unwrap().activate().unwrap();
                                        is_lent
                                    } else {
                                        false
                                    }
                                };
                                println!(
                                    "!!! 0x{:08x} is owned by {} ({}) but is mapped to {} ({}) -- {}",
                                    phys,
                                    eo.map(|v| v.get() as isize).unwrap_or(-1),
                                    eo.map(|v| system_services.process_name(v).unwrap_or("<unknown>"))
                                        .unwrap_or("<none>"),
                                    pid.get(),
                                    system_services.process_name(pid).unwrap_or("<unknown>"),
                                    if is_lent { "page is lent" } else { "duplicate!" },
                                );
                            }
                            if !crate::arch::mem::page_is_lent(virt as *mut u8) {
                                if owner.is_none() {
                                    owner = Some((pid, virt));
                                } else {
                                    println!(
                                        "!!! DUPLICATE !!! Page {:08x} owned by both {} ({}) @ {:08x} and {} ({}) @ {:08x}",
                                        phys,
                                        owner.map(|v| v.0.get() as isize).unwrap_or(-1),
                                        owner
                                            .map(|v| system_services.process_name(v.0).unwrap_or("<unknown>"))
                                            .unwrap_or("<none>"),
                                        owner.map(|v| v.1).unwrap_or(0),
                                        pid.get(),
                                        system_services.process_name(pid).unwrap_or("<unknown>"),
                                        virt,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Restore the previous PID
            system_services.get_process(current_pid).unwrap().activate().unwrap();
        })
    }
}

pub unsafe fn bzero<T>(mut start: *mut T, end: *mut T)
where
    T: Copy,
{
    while start < end {
        // NOTE(volatile) to prevent this from being transformed into `memclr`
        core::ptr::write_volatile(start, core::mem::zeroed());
        start = start.offset(1);
    }
}
