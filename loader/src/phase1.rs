use core::{mem, slice};

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
pub fn phase_1(cfg: &mut BootConfig) {
    // Allocate space for the stack pointer.
    // The bootloader should have placed the stack pointer at the end of RAM
    // prior to jumping to our program, so allocate one page of data for
    // stack.
    // All other allocations will be placed below the stack pointer.
    //
    // As of Xous 0.8, the top page is bootloader stack, and the page below that is the 'clean suspend' page.
    cfg.init_size += GUARD_MEMORY_BYTES;

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
        copy_args(cfg);
    }

    // All further allocations must be page-aligned.
    cfg.init_size = (cfg.init_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // Additionally, from this point on all allocations come from
    // their respective processes rather than kernel memory.

    // Copy the processes to RAM, if requested.
    if !cfg.no_copy {
        println!("Copying processes");
        copy_processes(cfg);
    }

    // Mark all pages as in-use by the kernel.
    // NOTE: This causes the .text section to be owned by the kernel!  This
    // will require us to transfer ownership in `stage3`.
    // Note also that we skip the first four indices, causing the stack to be
    // returned to the process pool.

    // We also skip the an additional index as that is the clean suspend page. This
    // needs to be claimed by the susres server before the kernel allocates it.
    // Lower numbered indices corresponding to higher address pages.
    println!("Marking pages as in-use");
    for i in 4..(cfg.init_size / PAGE_SIZE) {
        cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - i] = 1;
    }
}

/// Allocate and initialize memory regions.
/// Returns a pointer to the start of the memory region.
pub fn allocate_regions(cfg: &mut BootConfig) {
    // Number of individual pages in the system
    let mut rpt_pages = cfg.sram_size / PAGE_SIZE;

    for region in cfg.regions.iter() {
        println!(
            "Discovered memory region {:08x} ({:08x} - {:08x}) -- {} bytes",
            region.name,
            region.start,
            region.start + region.length,
            region.length
        );
        let region_length_rounded = (region.length as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        rpt_pages += region_length_rounded / PAGE_SIZE;
    }

    // Round the tracker to a multiple of the page size, so as to keep memory
    // operations fast.
    rpt_pages = (rpt_pages + mem::size_of::<usize>() - 1) & !(mem::size_of::<usize>() - 1);

    cfg.init_size += rpt_pages * mem::size_of::<XousPid>();

    // Clear all memory pages such that they're not owned by anyone
    let runtime_page_tracker = cfg.get_top();
    assert!((runtime_page_tracker as usize) < (cfg.sram_start as usize) + cfg.sram_size);
    unsafe {
        bzero(runtime_page_tracker, runtime_page_tracker.add(rpt_pages / mem::size_of::<usize>()));
    }

    cfg.runtime_page_tracker =
        unsafe { slice::from_raw_parts_mut(runtime_page_tracker as *mut XousPid, rpt_pages) };
}

pub fn allocate_processes(cfg: &mut BootConfig) {
    let process_count = cfg.init_process_count + 1;
    let table_size = process_count * mem::size_of::<InitialProcess>();
    // Allocate the process table
    cfg.init_size += table_size;
    let processes = cfg.get_top();
    unsafe {
        bzero(processes, processes.add((table_size / mem::size_of::<usize>()) as usize));
    }
    cfg.processes =
        unsafe { slice::from_raw_parts_mut(processes as *mut InitialProcess, process_count as usize) };
}

pub fn copy_args(cfg: &mut BootConfig) {
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
                    unsafe { cfg.base_addr.add(inie.load_offset as usize / mem::size_of::<usize>()) }
                        as *const u8;

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
                // IniS does not necessarily exist in linear memory space.

                // TODO: access IniS image and figure out what regions are "write" and allocate/copy
                // those into RAM. We do this because there isn't a 1:1 mapping of these to the
                // images on disk which creates a problem for the swapper.

                // Then, for regions that *are* aligned, allocate them into the swapper's
                // memory space, and copy the data into swap.
                todo!("implement inis handling");
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
                    let src_addr = cfg.base_addr.add(prog.load_offset as usize / mem::size_of::<usize>());
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
                        top.add(prog.text_size as usize / mem::size_of::<usize>()) as usize,
                        top.add(load_size_rounded as usize / mem::size_of::<usize>()) as usize,
                    );

                    memcpy(top, src_addr, prog.text_size as usize + 1);

                    // Zero out the remaining data.
                    bzero(
                        top.add(prog.text_size as usize / mem::size_of::<usize>()),
                        top.add(load_size_rounded as usize / mem::size_of::<usize>()),
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
                        .add((prog.load_offset + prog.text_size + 4) as usize / mem::size_of::<usize>() - 1);
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
                        top.add(prog.data_size as usize / mem::size_of::<usize>()) as usize,
                        top.add(load_size_rounded as usize / mem::size_of::<usize>()) as usize
                    );
                    bzero(
                        top.add(prog.data_size as usize / mem::size_of::<usize>()),
                        top.add(load_size_rounded as usize / mem::size_of::<usize>()),
                    )
                }
            }
            _ => {}
        }
    }
}

unsafe fn memcpy<T>(dest: *mut T, src: *const T, count: usize)
where
    T: Copy,
{
    if VDBG {
        println!(
            "COPY (align {}): {:08x} - {:08x} {} {:08x} - {:08x}",
            mem::size_of::<T>(),
            src as usize,
            src as usize + count,
            count,
            dest as usize,
            dest as usize + count
        );
    }
    core::ptr::copy_nonoverlapping(src, dest, count / mem::size_of::<T>());
}
