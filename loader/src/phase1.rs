use core::{mem::size_of, slice};

#[cfg(feature = "atsama5d27")]
pub use crate::platform::atsama5d27::load::InitialProcess;
use crate::*;

#[repr(C)]
#[cfg(not(feature = "atsama5d27"))]
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    pub satp: usize,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,

    /// Address of the start of the env block
    pub env: usize,
}

/// Phase 1:
///
/// Copy processes from FLASH to RAM, allocating memory one page at a time starting from high
/// addresses and working down. The allocations are computed from the kernel arguments, and the
/// allocated amount is re-computed and used in phase 2 to setup the page tables.
///
/// We don't memorize the allocated results (in part because we don't have malloc/alloc to stick
/// the table, and we don't know a priori how big it will be); we simply memorize the maximum extent,
/// after which we allocate the book-keeping tables.
pub fn phase_1(cfg: &mut BootConfig, detached_app: bool) {
    // Allocate space for the stack pointer.
    // The bootloader should have placed the stack pointer at the end of RAM
    // prior to jumping to our program. Reserve space for the stack, so that it does not smash
    // run time allocations.
    //
    // All other allocations will be placed below the stack pointer.
    //
    // As of Xous 0.8, the top page is bootloader stack, and the page below that is the 'clean suspend' page.
    cfg.init_size += GUARD_MEMORY_BYTES;
    println!("Loader runtime stack should not exceed: {:x}", cfg.get_top() as usize);

    // The first region is defined as being "main RAM", which will be used
    // to keep track of allocations.
    println!("Allocating regions");
    allocate_regions(cfg);

    // The kernel, as well as initial processes, are all stored in RAM.
    println!("Allocating processes");
    allocate_processes(cfg);

    // Copy the arguments, if requested
    if cfg.no_copy {
        // TODO: place args into cfg.args
    } else {
        println!("Copying args");
        copy_args(cfg, detached_app);
    }
    if VDBG {
        let check_iter = cfg.args.iter();
        for a in check_iter {
            crate::println!(
                "{}: base {:x} size {:x} data {:x?}",
                core::str::from_utf8(&a.name.to_le_bytes()).unwrap_or("invalid name"),
                a.this as usize,
                a.size,
                a.data
            );
        }
    }

    // All further allocations must be page-aligned.
    #[cfg(feature = "swap")]
    if SDBG {
        println!(" -> cfg end pre-alignment: {:x}(-{:x})", cfg.get_top() as usize, cfg.init_size);
    }
    cfg.init_size = (cfg.init_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    #[cfg(feature = "swap")]
    if SDBG {
        println!(" -> cfg end post-alignment: {:x}(-{:x})", cfg.get_top() as usize, cfg.init_size);
    }

    #[cfg(feature = "swap")]
    allocate_swap(cfg);

    // debug addresses of all key cfg fields
    #[cfg(feature = "debug-print")]
    {
        println!("*** CFG debug ***");
        println!("  base_addr: {:x}", cfg.base_addr as usize);
        println!(
            "  regions: {:x} -> {:x}",
            cfg.regions.as_ptr() as usize,
            cfg.regions.as_ptr() as usize + cfg.regions.len() * size_of::<MemoryRegionExtra>()
        );
        println!(
            "  sram_start: {:x}, sram_end: {:x}",
            cfg.sram_start as usize,
            cfg.sram_start as usize + cfg.sram_size
        );
        println!(
            "  RPT: {:x} -> {:x}",
            cfg.runtime_page_tracker.as_ptr() as usize,
            cfg.runtime_page_tracker.as_ptr() as usize
                + cfg.runtime_page_tracker.len() * size_of::<XousAlloc>()
        );
        println!(
            "  XPT: {:x} -> {:x}",
            cfg.extra_page_tracker.as_ptr() as usize,
            cfg.extra_page_tracker.as_ptr() as usize + cfg.extra_page_tracker.len() * size_of::<XousPid>()
        );
        println!(
            "  processes: {:x} -> {:x}",
            cfg.processes.as_ptr() as usize,
            cfg.processes.as_ptr() as usize + cfg.processes.len() * size_of::<InitialProcess>()
        );
        #[cfg(feature = "swap")]
        {
            if let Some(hal) = cfg.swap_hal.as_ref() {
                println!(
                    "    (Stack) HAL: {:x} -> {:x}",
                    hal as *const SwapHal as usize,
                    hal as *const SwapHal as usize + size_of::<SwapHal>()
                );
            } else {
                println!("  NO SWAP HAL!!!");
            }
            if let Some(hal) = cfg.swap_hal.as_ref() {
                println!(
                    "    (Stack) HAL decrypt buf: {:x} -> {:x}",
                    hal.buf_as_ref().as_ptr() as usize,
                    hal.buf_as_ref().as_ptr() as usize + hal.buf_as_ref().len() * size_of::<u8>()
                );
            } else {
                println!("  NO SWAP HAL!!!");
            }
            println!(
                "  swap_root: {:x} -> {:x}",
                cfg.swap_root.as_ptr() as usize,
                cfg.swap_root.as_ptr() as usize + cfg.swap_root.len() * size_of::<usize>()
            );
        }
    }

    println!("Runtime Page Tracker: {} bytes", cfg.runtime_page_tracker.len());
    #[cfg(feature = "swap")]
    if SDBG {
        println!("Occupied RPT entries (at this point, the list should be nil):");
        for (_i, entry) in cfg.runtime_page_tracker.iter().enumerate() {
            if entry.raw_vpn() != 0 {
                println!("  {:x}: {:x} [{}]", _i, entry.raw_vpn(), entry.timestamp());
            }
        }
    }

    // Additionally, from this point on all allocations come from
    // their respective processes rather than kernel memory.

    // Copy the processes to RAM, if requested.
    if !cfg.no_copy {
        println!("Copying processes");
        copy_processes(cfg);
    }
    // activate this to debug stack-smashing during copy_process(). The RPT is the first structure that gets
    // smashed if the stack overflows! It should be all 0's if the stack did not overrun.
    #[cfg(feature = "swap")]
    if SDBG && VDBG {
        for (_i, _r) in cfg.runtime_page_tracker
            [cfg.runtime_page_tracker.len() - 1024.min(cfg.runtime_page_tracker.len())..]
            .chunks(32)
            .enumerate()
        {
            println!("  rpt {:08x}: {:02x?}", cfg.runtime_page_tracker.len() - 1024 + _i * 32, _r);
        }
    }

    // Mark all pages as in-use by the kernel.
    // NOTE: This causes the .text section to be owned by the kernel!  This
    // will require us to transfer ownership in `stage3`.
    // Note also that we skip the first four indices, causing the stack to be
    // returned to the process pool.

    // We also skip the an additional index as that is the clean suspend page. This
    // needs to be claimed by the susres server before the kernel allocates it.
    // Lower numbered indices corresponding to higher address pages.
    println!("Marking loader pages as in-use");
    for i in ((GUARD_MEMORY_BYTES / PAGE_SIZE) + 1)..(cfg.init_size / PAGE_SIZE) {
        cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - i] = XousAlloc::from(1);
    }
}

/// Allocate and initialize memory regions.
/// Returns a pointer to the start of the memory region.
pub fn allocate_regions(cfg: &mut BootConfig) {
    // Number of individual pages in the system
    let mut rpt_pages = cfg.sram_size / PAGE_SIZE;
    // Round the tracker to a multiple of the pointer size, so as to keep memory
    // operations fast.
    rpt_pages = (rpt_pages + size_of::<usize>() - 1) & !(size_of::<usize>() - 1);

    // allocate the RPT
    #[cfg(not(feature = "swap"))]
    {
        cfg.init_size += rpt_pages * mem::size_of::<XousAlloc>();
    }
    #[cfg(feature = "swap")]
    {
        println!("RPT raw pages: {:x}", rpt_pages);

        // Round the allocation to a multiple of the page size, so that it can be mapped into userspace
        let proposed_alloc = rpt_pages * mem::size_of::<XousAlloc>();
        let page_aligned_alloc = (proposed_alloc + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        assert!(cfg.init_size & 0xFFF == 0, "init_size should be page aligned going into the RPT alloc");
        cfg.init_size += page_aligned_alloc;
        assert!(cfg.init_size & 0xFFF == 0, "init_size should be page aligned leaving the RPT alloc");
        println!(
            "RPT ALLOC init_size before: {:x}, ext pages: {:x}, minimal alloc: {:x}, page-aligned alloc: {:x}, sizeof entry: {:x}",
            cfg.init_size,
            cfg.extra_pages,
            proposed_alloc,
            page_aligned_alloc,
            size_of::<XousAlloc>(),
        );
    }

    // Clear all memory pages such that they're not owned by anyone
    let runtime_page_tracker = cfg.get_top();
    println!("rpt value: {:x}", runtime_page_tracker as usize);
    #[cfg(feature = "swap")]
    {
        assert!(runtime_page_tracker as usize & 0xFFF == 0); // this needs to be page-aligned for swap to work
    }
    assert!((runtime_page_tracker as usize) < (cfg.sram_start as usize) + cfg.sram_size);
    unsafe {
        bzero(
            runtime_page_tracker as *mut XousAlloc,
            (runtime_page_tracker as *mut XousAlloc).add(rpt_pages),
        );
    }

    cfg.runtime_page_tracker =
        unsafe { slice::from_raw_parts_mut(runtime_page_tracker as *mut XousAlloc, rpt_pages) };
    #[cfg(feature = "swap")]
    if SDBG {
        println!(
            " -> RPT range: {:x} - {:x}",
            runtime_page_tracker as usize,
            runtime_page_tracker as usize + rpt_pages * size_of::<XousAlloc>()
        );
    }

    // allocate the XPT
    let mut xpt_pages = 0;
    for region in cfg.regions.iter() {
        println!(
            "Discovered memory region {:08x} ({:08x} - {:08x}) -- {} bytes",
            region.name,
            region.start,
            region.start + region.length,
            region.length
        );
        let region_length_rounded = (region.length as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        xpt_pages += region_length_rounded / PAGE_SIZE;
    }
    // Round the tracker to a multiple of the pointer size, so as to keep memory
    // operations fast.
    xpt_pages = (xpt_pages + size_of::<usize>() - 1) & !(size_of::<usize>() - 1);
    println!(
        "XPT ALLOC init_size bef: {:x}, ext pages: {:x}, rpt alloc: {:x}",
        cfg.init_size,
        cfg.extra_pages,
        xpt_pages * size_of::<XousPid>()
    );

    cfg.init_size += xpt_pages * size_of::<XousPid>();

    // Clear all memory pages such that they're not owned by anyone
    let extra_page_tracker = cfg.get_top();
    println!("xpt value: {:x}", extra_page_tracker as usize);
    assert!((extra_page_tracker as usize) < (cfg.sram_start as usize) + cfg.sram_size);
    unsafe {
        bzero(extra_page_tracker as *mut XousPid, (extra_page_tracker as *mut XousPid).add(xpt_pages));
    }

    cfg.extra_page_tracker =
        unsafe { slice::from_raw_parts_mut(extra_page_tracker as *mut XousPid, xpt_pages) };
    #[cfg(feature = "swap")]
    if SDBG {
        println!(
            " -> XPT range: {:x} - {:x}",
            extra_page_tracker as usize,
            extra_page_tracker as usize + xpt_pages * size_of::<XousPid>()
        );
    }
}

pub fn allocate_processes(cfg: &mut BootConfig) {
    let process_count = cfg.init_process_count + 1;
    println!("Allocating tables for {} processes", process_count);
    let table_size = process_count * mem::size_of::<InitialProcess>();
    // Allocate the process table
    cfg.init_size += table_size;
    let processes = cfg.get_top();
    unsafe {
        bzero(
            processes as *mut InitialProcess,
            (processes as *mut InitialProcess).add(process_count as usize),
        );
    }
    cfg.processes =
        unsafe { slice::from_raw_parts_mut(processes as *mut InitialProcess, process_count as usize) };

    #[cfg(feature = "swap")]
    {
        if SDBG {
            println!(
                " -> Processes range: {:x} - {:x}",
                processes as usize,
                processes as usize + cfg.processes.len() * mem::size_of::<InitialProcess>()
            );
        }
        let swap_root_pt_size = process_count * mem::size_of::<usize>();
        cfg.init_size += swap_root_pt_size;
        let swap_root_pt = cfg.get_top();
        unsafe {
            bzero(swap_root_pt, swap_root_pt.add(process_count));
        }
        cfg.swap_root = unsafe { slice::from_raw_parts_mut(swap_root_pt, process_count as usize) };
        if SDBG {
            println!(
                " -> Swap root pt range: {:x} - {:x}",
                swap_root_pt as usize,
                swap_root_pt as usize + cfg.swap_root.len() * mem::size_of::<usize>()
            );
        }
    }
}

#[cfg(feature = "swap")]
pub fn allocate_swap(cfg: &mut BootConfig) {
    let process_count = cfg.init_process_count + 1;
    let swap_pt_size = process_count * size_of::<PageTable>();
    cfg.init_size += swap_pt_size;
    cfg.swap_offset += swap_pt_size;
    let swap_pt_base = cfg.get_top();
    unsafe {
        bzero(swap_pt_base as *mut PageTable, (swap_pt_base as *mut PageTable).add(cfg.swap_root.len()))
    }
    // The page table proper is "unbound": we don't put it in a slice, we simply put references
    // to each page in cfg.swap_root[]. I wonder if this is UB?
    for (index, root) in cfg.swap_root.iter_mut().enumerate() {
        *root = swap_pt_base as usize + index * mem::size_of::<PageTable>();
    }

    if SDBG {
        println!(
            " -> Swap pt data range: {:x} - {:x}",
            swap_pt_base as usize,
            swap_pt_base as usize + cfg.swap_root.len() * mem::size_of::<PageTable>()
        );
    }
}

#[cfg(all(feature = "swap", feature = "bao1x"))]
pub fn copy_args(cfg: &mut BootConfig, _detached_app: bool) {
    // With swap enabled, copy_args also merges the IniS arguments from the swap region into the kernel
    // arguments, and patches the length field accordingly.

    // Read in the swap arguments: should be located at beginning of the first page of swap.
    // Safety: only safe because we know that the decrypt was setup by read_swap_config(), and no pages
    // were decrypted between then and now!
    let page0 = unsafe { cfg.swap_hal.as_mut().unwrap().get_decrypt() };
    let swap_args = KernelArguments::new(page0.as_ptr() as *const usize);

    // Carve out a much larger region than anticipated for argument processing. A "typical" target for
    // the new argument stack is around 1800 bytes.
    //
    // It's assumed we have at least 4 pages of RAM at this point because we haven't allocated anything and
    // our target should have more memory than that. So, pass a total of 16,384 bytes region with the
    // anticipation that the actual allocation will be much smaller than that.
    let dest_region = unsafe {
        core::slice::from_raw_parts_mut(
            (cfg.get_top() as usize - PAGE_SIZE * 4) as *mut u32,
            PAGE_SIZE * 4 / size_of::<u32>(),
        )
    };

    // Replace the original args pointer in FLASH with a new copy in RAM that incorporates any merged
    // regions that the kernel should be aware of.
    let args_size: usize;
    (cfg.args, args_size) = unsafe {
        cfg.args
            .merge_args(&[swap_args], &[u32::from_le_bytes(*b"IniS")], dest_region)
            .expect("Couldn't merge kernel arguments")
    };

    // Advance the init_size pointer by the actual allocation consumed by the merge_args() function.
    cfg.init_size += args_size;
    crate::println!("Argument merge done, total size consumed: {}", args_size);
}

/// No swap but bao1x implies a dabao configuration. Check for a detached-app image.
#[cfg(all(not(feature = "swap"), feature = "bao1x"))]
pub fn copy_args(cfg: &mut BootConfig, detached_app: bool) {
    if detached_app {
        // Read in the detached app arguments.
        // Safety: only safe because the image is aligned.
        let app_args = KernelArguments::new(
            (bao1x_api::offsets::dabao::APP_RRAM_START + bao1x_api::signatures::SIGBLOCK_LEN) as *const usize,
        );

        // Carve out a much larger region than anticipated for argument processing. A "typical" target for
        // the new argument stack is around 1800 bytes.
        //
        // It's assumed we have at least 4 pages of RAM at this point because we haven't allocated
        // anything and our target should have more memory than that. So, pass a total of
        // 16,384 bytes region with the anticipation that the actual allocation will be much
        // smaller than that.
        let dest_region = unsafe {
            core::slice::from_raw_parts_mut(
                (cfg.get_top() as usize - PAGE_SIZE * 4) as *mut u32,
                PAGE_SIZE * 4 / size_of::<u32>(),
            )
        };

        // Replace the original args pointer in FLASH with a new copy in RAM that incorporates any merged
        // regions that the kernel should be aware of.
        let args_size: usize;
        (cfg.args, args_size) = unsafe {
            cfg.args
                .merge_args(&[app_args], &[u32::from_le_bytes(*b"IniF")], dest_region)
                .expect("Couldn't merge kernel arguments")
        };

        // Advance the init_size pointer by the actual allocation consumed by the merge_args() function.
        cfg.init_size += args_size;
        crate::println!("Argument merge done, total size consumed: {}", args_size);
    } else {
        // Straight copy of the kernel arguments to RAM, no merging.
        cfg.init_size += cfg.args.size();
        let runtime_arg_buffer = cfg.get_top();
        unsafe { memcpy(runtime_arg_buffer, cfg.args.base as *const usize, cfg.args.size() as usize) };
        cfg.args = KernelArguments::new(runtime_arg_buffer);
    }
}

#[cfg(feature = "swap")]
fn remaining_in_page(addr: usize) -> usize { PAGE_SIZE - (addr & (PAGE_SIZE - 1)) }

#[cfg(all(not(feature = "swap"), not(feature = "bao1x")))]
pub fn copy_args(cfg: &mut BootConfig, _detached_app: bool) {
    // Copy the args list to target RAM
    cfg.init_size += cfg.args.size();
    let runtime_arg_buffer = cfg.get_top();
    unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        memcpy(runtime_arg_buffer, cfg.args.base as *const usize, cfg.args.size() as usize)
    };
    cfg.args = KernelArguments::new(runtime_arg_buffer);
}

#[derive(Eq, PartialEq)]
enum TagType {
    IniE,
    IniF,
    IniS,
    XKrn,
    Other,
}
impl From<u32> for TagType {
    fn from(code: u32) -> Self {
        if code == u32::from_le_bytes(*b"IniE") {
            TagType::IniE
        } else if code == u32::from_le_bytes(*b"IniF") {
            TagType::IniF
        } else if code == u32::from_le_bytes(*b"IniS") {
            TagType::IniS
        } else if code == u32::from_le_bytes(*b"XKrn") {
            TagType::XKrn
        } else {
            TagType::Other
        }
    }
}
impl TagType {
    #[cfg(feature = "debug-print")]
    pub fn to_str(&self) -> &'static str {
        match self {
            TagType::IniE => "IniE",
            TagType::IniF => "IniF",
            TagType::IniS => "IniS",
            TagType::XKrn => "XKrn",
            TagType::Other => "Other",
        }
    }
}

/// Copy program data from the SPI flash into newly-allocated RAM
/// located at the end of memory space.
fn copy_processes(cfg: &mut BootConfig) {
    let mut _pid = 1;
    for tag in cfg.args.iter() {
        let tag_type = TagType::from(tag.name);
        match tag_type {
            TagType::IniF | TagType::IniE => {
                _pid += 1;
                let mut top = core::ptr::null_mut::<u8>();

                let inie = MiniElf::new(&tag);
                let mut src_paddr =
                    unsafe { cfg.base_addr.add(inie.load_offset as usize / size_of::<usize>()) } as *const u8;

                println!("\n\n{} {} has {} sections", tag_type.to_str(), _pid, inie.sections.len());
                println!(
                    "Initial top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}",
                    cfg.get_top() as *mut u8 as u32,
                    cfg.extra_pages,
                    cfg.init_size,
                    cfg.base_addr as u32
                );

                let mut last_page_vaddr = 0;
                let mut last_section_perfect_fit = false;

                for section in inie.sections.iter() {
                    let flags = section.flags() as u8;
                    // any section that requires "write" must be copied to RAM
                    // note that ELF helpfully adds a 4096-byte gap between non-write pages and write-pages
                    // allowing us to just trundle through the pages and not have to deal with partially
                    // writeable pages.
                    // IniE is always copy_to_ram
                    let copy_to_ram = (flags & MINIELF_FLG_W != 0) || (tag_type == TagType::IniE);

                    if (section.virt as usize) < last_page_vaddr {
                        panic!(
                            "init section addresses are not strictly increasing (new virt: {:08x}, last virt: {:08x})",
                            section.virt, last_page_vaddr
                        );
                    }

                    // cfg.extra_pages tracks how many pages of RAM we've allocated so far
                    // cfg.top() points to the bottom of the most recently allocated page
                    //    - so if cfg.extra_pages is 0, nothing is allocated, and cfg.top() points to
                    //      previously reserved space
                    //
                    // The section length always matches the stride between sections in physical memory.
                    //
                    // However, the section length has nothing to do with the distance between sections in
                    // virtual memory; the virtual start address is allowed to be an
                    // arbitrary number of bytes higher than the previous section end, for
                    // alignment and padding reasons.
                    if copy_to_ram {
                        let mut dst_page_vaddr = section.virt as usize;
                        let mut bytes_to_copy = section.len();

                        if (last_page_vaddr & !(PAGE_SIZE - 1)) != (dst_page_vaddr & !(PAGE_SIZE - 1))
                            || last_section_perfect_fit
                        {
                            // this condition is always true for the first section's first iteration, because
                            // current_vpage_addr starts as NULL; thus we are guaranteed to always
                            // trigger the page allocate/zero mechanism the first time through the loop
                            //
                            // `last_section_perfect_fit` triggers a page allocation as well, because in this
                            // case we had exactly enough data to fill out the
                            // previous section, so we have no more space left in
                            // the current page. We don't automatically allocate a new page because
                            // if it was actually the very last section we *shouldn't* allocate another page;
                            // and we can only know if there's another section
                            // available by dropping off the end of the
                            // loop and coming back to the surrounding for-loop iterator.
                            cfg.extra_pages += 1;
                            top = cfg.get_top() as *mut u8;
                            unsafe {
                                bzero(top, top.add(PAGE_SIZE as usize));
                            }
                        }

                        // Copy the start copying the source data into virtual memory, until the current
                        // page is exhausted.
                        while bytes_to_copy > 0 {
                            let bytes_remaining_in_vpage = PAGE_SIZE - (dst_page_vaddr & (PAGE_SIZE - 1));
                            let copyable_bytes = bytes_remaining_in_vpage.min(bytes_to_copy);
                            last_section_perfect_fit = bytes_remaining_in_vpage == bytes_to_copy;
                            if !section.no_copy() {
                                unsafe {
                                    memcpy(
                                        top.add(dst_page_vaddr & (PAGE_SIZE - 1)),
                                        src_paddr,
                                        copyable_bytes,
                                    );
                                    src_paddr = src_paddr.add(copyable_bytes);
                                }
                            } else {
                                // chunk is already zeroed, because we zeroed the whole page when we got it.
                            }
                            bytes_to_copy -= copyable_bytes;
                            dst_page_vaddr += copyable_bytes;

                            if copyable_bytes == bytes_remaining_in_vpage && bytes_to_copy > 0 {
                                // we've reached the end of the vpage, and there's more to copy:
                                // grab a new page
                                cfg.extra_pages += 1;
                                top = cfg.get_top() as *mut u8;
                                if bytes_to_copy < PAGE_SIZE {
                                    // pre-zero out the page if the remaining data won't fill it.
                                    unsafe {
                                        bzero(top, top.add(PAGE_SIZE as usize));
                                    }
                                }
                            }
                        }
                        // set the vpage based on our current vpage. This allows us to allocate
                        // a new vpage on the next iteration in case there is surprise padding in
                        // the section load address.
                        last_page_vaddr = dst_page_vaddr;
                    } else {
                        top = cfg.get_top() as *mut u8;
                        // forward the FLASH address pointer by the length of the section.
                        src_paddr = unsafe { src_paddr.add(section.len()) };
                    }

                    if VDBG {
                        println!("Looping to the next section");
                        println!(
                            "top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}",
                            cfg.get_top() as *mut u8 as u32,
                            cfg.extra_pages,
                            cfg.init_size,
                            cfg.base_addr as u32
                        );
                        println!("last_page_vaddr: {:x}", last_page_vaddr);
                    }
                }
                println!("Done with sections");
            }
            TagType::IniS => {
                // if swap is not enabled, don't pull this code in, to keep the bootloader light-weight
                #[cfg(feature = "swap")]
                {
                    // IniS does not necessarily exist in linear memory space, so it requires special
                    // handling. Instead of copying the IniS data into RAM, it's copied
                    // into encrypted swap (e.g. the RAM area (again, not necessarily in
                    // linear space) reserved for swap processes).

                    /*
                    Example of an IniS section:
                    1    IniS: entrypoint @ 00021e68, loaded from 00001114.  Sections:
                    Physical offset in swap source image                     Destination range in virtual memory
                             src_swap_img_addr                                          dst_page_vaddr
                                  |                                                              |
                                  v                                                              v
                    Loaded from 00001114 - Section .gcc_except_table   4056 bytes loading into 00010114..000110ec flags: NONE
                    Loaded from 000020ec - Section .rodata        19080 bytes loading into 000110f0..00015b78 flags: NONE
                    Loaded from 00006b74 - Section .eh_frame_hdr   2172 bytes loading into 00015b78..000163f4 flags: EH_HEADER
                    Loaded from 000073f0 - Section .eh_frame       7740 bytes loading into 000163f4..00018230 flags: EH_FRAME
                    Loaded from 0000922c - Section .text          67428 bytes loading into 00019230..00029994 flags: EXECUTE
                    Loaded from 00019990 - Section .data              4 bytes loading into 0002a994..0002a998 flags: WRITE
                    Loaded from 00019994 - Section .sdata            32 bytes loading into 0002a998..0002a9b8 flags: WRITE
                    Loaded from 000199b4 - Section .sbss             64 bytes loading into 0002a9b8..0002a9f8 flags: WRITE | NOCOPY
                    Loaded from 000199f4 - Section .bss             532 bytes loading into 0002a9f8..0002ac0c flags: WRITE | NOCOPY

                    Note that we have full control over what swap block we put things into, but the swap block's
                    address offsets should have a 1:1 correlation to the *virtual* destination addresess. We track
                    the current swap page with `working_page_swap_offset`.
                    */

                    _pid += 1;
                    let mut working_page_swap_offset: Option<usize> = None;
                    let mut working_buf = [0u8; 4096];
                    let mut working_buf_dirty = false;

                    let inis = MiniElf::new(&tag);
                    let mut src_swap_img_addr = inis.load_offset as usize;

                    println!("\n\n{} {} has {} sections", tag_type.to_str(), _pid, inis.sections.len());
                    println!("Swap free page at swap addr: {:x}", cfg.swap_free_page,);

                    let mut last_copy_vaddr = 0;

                    for (index, section) in inis.sections.iter().enumerate() {
                        if SDBG {
                            println!("Section {}: {:x?}", index, section);
                        }
                        let mut dst_page_vaddr = section.virt as usize;
                        let mut bytes_to_copy = section.len();

                        if let Some(swap_offset) = working_page_swap_offset {
                            if (last_copy_vaddr & !(PAGE_SIZE - 1)) != (dst_page_vaddr & !(PAGE_SIZE - 1)) {
                                if SDBG {
                                    println!(
                                        "New section not aligned: last_copy_vaddr {:x}, dst_page_vaddr {:x}",
                                        last_copy_vaddr, dst_page_vaddr
                                    );
                                }
                                // handle case that the new section destination address is outside of the
                                // current page
                                cfg.swap_hal.as_mut().expect("swap HAL uninit").encrypt_swap_to(
                                    &mut working_buf,
                                    swap_offset * 0x1000,
                                    last_copy_vaddr & !(PAGE_SIZE - 1),
                                    _pid,
                                );
                                working_buf.fill(0);
                                working_page_swap_offset = Some(cfg.swap_free_page);
                                working_buf_dirty = false;
                                cfg.swap_free_page += 1;
                            }
                        } else {
                            // very first time through the loop. working_buf is guaranteed to be zero.
                            working_page_swap_offset = Some(cfg.swap_free_page);
                            cfg.swap_free_page += 1;
                        }

                        // Decrypt the source image data and re-encrypt it to swap for the section at hand.
                        //   - dst_page_vaddr is the virtual address of the section. We only care about this
                        //     for tracking offsets in pages, at this stage.
                        //   - working_page_swap_offset is the current destination swap RAM page
                        //   - src_swap_img_addr is the offset of the section in source swap FLASH.
                        //   - no_copy sections need to set the corresponding bytes in swap RAM to zero.
                        //
                        //
                        while bytes_to_copy > 0 {
                            // here are the cases we have to handle:
                            //   - the available decrypted data is larger than the target region to encrypt
                            //   - the available decrypted data is smaller than the target region to encrypt
                            //   - the available decrypted data is equal to the target region to encrypt
                            let src_swap_img_page = src_swap_img_addr & !(PAGE_SIZE - 1);
                            let src_swap_img_offset = src_swap_img_addr & (PAGE_SIZE - 1);
                            // it's almost free to check, so we check at every loop start
                            if (cfg.swap_hal.as_ref().expect("swap HAL uninit").decrypt_page_addr()
                                != src_swap_img_page)
                                && !section.no_copy()
                            {
                                cfg.swap_hal
                                    .as_mut()
                                    .expect("swap HAL uninit")
                                    .decrypt_src_page_at(src_swap_img_page)
                                    .unwrap();
                            }
                            let decrypt_avail = remaining_in_page(src_swap_img_addr);
                            let dst_page_avail = remaining_in_page(dst_page_vaddr);
                            let dst_page_offset = dst_page_vaddr & (PAGE_SIZE - 1);
                            let copyable = if decrypt_avail >= dst_page_avail {
                                dst_page_avail.min(bytes_to_copy)
                            } else {
                                decrypt_avail.min(bytes_to_copy)
                            };
                            if !section.no_copy() {
                                working_buf[dst_page_offset..dst_page_offset + copyable].copy_from_slice(
                                    &cfg.swap_hal.as_ref().expect("swap HAL uninit").buf_as_ref()
                                        [src_swap_img_offset..src_swap_img_offset + copyable],
                                );
                                working_buf_dirty = true;
                            } else {
                                // do nothing, because working_buff is filled with 0 on alloc
                                // but, mark the buffer as dirty, because, it still needs to be committed
                                working_buf_dirty = true;
                            }
                            bytes_to_copy -= copyable;
                            dst_page_vaddr += copyable;
                            src_swap_img_addr += copyable;
                            // if we filled up the destination, grab another page.
                            if (dst_page_vaddr & (PAGE_SIZE - 1)) == 0 {
                                // the current vaddr is pointing to a new, empty vaddr; we want to write
                                // out the previous, full page. Compute that here.
                                let full_page_vaddr = dst_page_vaddr - PAGE_SIZE;
                                // we copied exactly dst_page_avail, causing us to wrap around to 0
                                // write the existing page to swap, and allocate a new swap page
                                cfg.swap_hal.as_mut().expect("swap HAL uninit").encrypt_swap_to(
                                    &mut working_buf,
                                    working_page_swap_offset.unwrap() * 0x1000,
                                    full_page_vaddr & !(PAGE_SIZE - 1),
                                    _pid,
                                );
                                working_buf.fill(0);
                                working_page_swap_offset = Some(cfg.swap_free_page);
                                working_buf_dirty = false;
                                cfg.swap_free_page += 1;
                            }
                        }
                        // set the vpage based on our current vpage. This allows us to allocate
                        // a new vpage on the next iteration in case there is surprise padding in
                        // the section load address.
                        last_copy_vaddr = dst_page_vaddr;

                        if SDBG && VVDBG {
                            println!("Looping to the next section (swap)");
                            println!(
                                "  swap_free_page: {:x}, dst_page_vaddr: {:x}, src_swap_img_addr: {:x}",
                                cfg.swap_free_page, dst_page_vaddr, src_swap_img_addr,
                            );
                            println!("  last_copy_vaddr: {:x}", last_copy_vaddr);
                        }
                    }
                    // flush the encryption buffer
                    if working_buf_dirty {
                        cfg.swap_hal.as_mut().expect("swap HAL uninit").encrypt_swap_to(
                            &mut working_buf,
                            working_page_swap_offset.unwrap() * 0x1000,
                            last_copy_vaddr & !(PAGE_SIZE - 1),
                            _pid,
                        );
                    } else {
                        // we didn't use the current page, de-allocate it
                        cfg.swap_free_page -= 1;
                    }
                    println!("Done with sections");
                }
            }
            TagType::XKrn => {
                let prog = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };

                // TEXT SECTION
                // Round it off to a page boundary
                let load_size_rounded = (prog.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                cfg.extra_pages += load_size_rounded / PAGE_SIZE;
                let top = cfg.get_top();
                println!("\n\nKernel top: {:x}, extra_pages: {:x}", top as u32, cfg.extra_pages);
                unsafe {
                    // Copy the program to the target address, rounding it off to the load size.
                    let src_addr = cfg.base_addr.add(prog.load_offset as usize / size_of::<usize>());
                    println!(
                        "    Copying TEXT from {:08x}-{:08x} to {:08x}-{:08x} ({} bytes long)",
                        src_addr as usize,
                        src_addr as u32 + prog.text_size,
                        top as usize,
                        top as u32 + prog.text_size + 4,
                        prog.text_size + 4
                    );
                    println!(
                        "    Zeroing out TEXT from {:08x}-{:08x}",
                        top.add(prog.text_size as usize / size_of::<usize>()) as usize,
                        top.add(load_size_rounded as usize / size_of::<usize>()) as usize,
                    );

                    memcpy(top, src_addr, prog.text_size as usize + 1);

                    // Zero out the remaining data.
                    bzero(
                        top.add(prog.text_size as usize / size_of::<usize>()),
                        top.add(load_size_rounded as usize / size_of::<usize>()),
                    )
                };

                // DATA SECTION
                // Round it off to a page boundary
                let load_size_rounded =
                    ((prog.data_size + prog.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                cfg.extra_pages += load_size_rounded / PAGE_SIZE;
                let top = cfg.get_top();
                unsafe {
                    // Copy the program to the target address, rounding it off to the load size.
                    let src_addr = cfg
                        .base_addr
                        .add((prog.load_offset + prog.text_size + 4) as usize / size_of::<usize>() - 1);
                    println!(
                        "    Copying DATA from {:08x}-{:08x} to {:08x}-{:08x} ({} bytes long)",
                        src_addr as usize,
                        src_addr as u32 + prog.data_size,
                        top as usize,
                        top as u32 + prog.data_size,
                        prog.data_size
                    );
                    memcpy(top, src_addr, prog.data_size as usize + 1);

                    // Zero out the remaining data.
                    println!(
                        "    Zeroing out DATA from {:08x} - {:08x}",
                        top.add(prog.data_size as usize / size_of::<usize>()) as usize,
                        top.add(load_size_rounded as usize / size_of::<usize>()) as usize
                    );
                    bzero(
                        top.add(prog.data_size as usize / size_of::<usize>()),
                        top.add(load_size_rounded as usize / size_of::<usize>()),
                    )
                }
            }
            _ => {}
        }
    }
}

pub unsafe fn memcpy<T>(dest: *mut T, src: *const T, count: usize)
where
    T: Copy,
{
    if VDBG {
        println!(
            "COPY (align {}): {:08x} - {:08x} {} {:08x} - {:08x}",
            size_of::<T>(),
            src as usize,
            src as usize + count,
            count,
            dest as usize,
            dest as usize + count
        );
    }
    core::ptr::copy_nonoverlapping(src, dest, count / size_of::<T>());
}
