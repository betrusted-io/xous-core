use core::{mem, slice};

use crate::*;

#[repr(C)]
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    pub satp: usize,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,
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
        bzero(
            runtime_page_tracker,
            runtime_page_tracker.add(rpt_pages / mem::size_of::<usize>()),
        );
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
        bzero(
            processes,
            processes.add((table_size / mem::size_of::<usize>()) as usize),
        );
    }
    cfg.processes = unsafe {
        slice::from_raw_parts_mut(processes as *mut InitialProcess, process_count as usize)
    };
}

pub fn copy_args(cfg: &mut BootConfig) {
    // Copy the args list to target RAM
    cfg.init_size += cfg.args.size();
    let runtime_arg_buffer = cfg.get_top();
    unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        memcpy(
            runtime_arg_buffer,
            cfg.args.base as *const usize,
            cfg.args.size() as usize,
        )
    };
    cfg.args = KernelArguments::new(runtime_arg_buffer);
}

/// Copy program data from the SPI flash into newly-allocated RAM
/// located at the end of memory space.
fn copy_processes(cfg: &mut BootConfig) {
    let mut _pid = 1;
    for tag in cfg.args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            _pid += 1;
            let mut page_addr: usize = 0;
            let mut previous_addr: usize = 0;
            let mut top = core::ptr::null_mut::<u8>();

            let inie = MiniElf::new(&tag);
            let mut src_addr = unsafe {
                cfg.base_addr
                    .add(inie.load_offset as usize / mem::size_of::<usize>())
            } as *const u8;

            // Example: Page starts at 0xf0c0 and is 8192 bytes long.
            // 1. Copy 3094 bytes to page 1
            // 2. Copy 4096 bytes to page 2
            // 3. Copy 192 bytes to page 3
            //
            // Example: Page starts at 0xf000 and is 4096 bytes long
            // 1. Copy 4096 bytes to page 1
            //
            // Example: Page starts at 0xf000 and is 128 bytes long
            // 1. Copy 128 bytes to page 1
            //
            // Example: Page starts at oxf0c0 and is 128 bytes long
            // 1. Copy 128 bytes to page 1
            println!("\n\nIniE {} has {} sections", _pid, inie.sections.len());
            println!("Initial top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}",
                cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
            for section in inie.sections.iter() {
                if (section.virt as usize) < previous_addr {
                    panic!("init section addresses are not strictly increasing (new virt: {:08x}, last virt: {:08x})", section.virt, previous_addr);
                }

                let this_page = section.virt as usize & !(PAGE_SIZE - 1);
                let mut bytes_to_copy = section.len();

                if VDBG {println!(
                    "Section is {} bytes long, loaded to {:08x}",
                    bytes_to_copy, section.virt
                );}
                // If this is not a new page, ensure the uninitialized values from between
                // this section and the previous one are all zeroed out.
                if this_page != page_addr || previous_addr == page_addr {
                    if VDBG {println!("New page @ {:08x}", this_page);}
                    if previous_addr != 0 && previous_addr != page_addr {
                        if VDBG {println!(
                            "Zeroing-out remainder of previous page: {:08x} (mapped to physical address {:08x})",
                            previous_addr, top as usize,
                        );}
                        unsafe {
                            bzero(
                                top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                                top.add(PAGE_SIZE as usize),
                            )
                        };
                    }

                    // Allocate a new page.
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;

                    // Zero out the page, if necessary.
                    unsafe { bzero(top, top.add(section.virt as usize & (PAGE_SIZE - 1))) };
                } else {
                    if VDBG {println!("Reusing existing page @ {:08x}", this_page);}
                }

                // Part 1: Copy the first chunk over.
                let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                if first_chunk_size > section.len() {
                    first_chunk_size = section.len();
                }
                let first_chunk_offset = section.virt as usize & (PAGE_SIZE - 1);
                if VDBG {println!(
                    "Section chunk is {} bytes, {} from {:08x}:{:08x} -> {:08x}:{:08x} (virt: {:08x})",
                    first_chunk_size,
                    if section.no_copy() { "zeroing" } else { "copying" },
                    src_addr as usize,
                    unsafe { src_addr.add(first_chunk_size) as usize },
                    unsafe { top.add(first_chunk_offset) as usize },
                    unsafe { top.add(first_chunk_size + first_chunk_offset)
                        as usize },
                    this_page + first_chunk_offset,
                );}
                // Perform the copy, if NOCOPY is not set
                if !section.no_copy() {
                    unsafe {
                        memcpy(top.add(first_chunk_offset), src_addr, first_chunk_size);
                        src_addr = src_addr.add(first_chunk_size);
                    }
                } else {
                    unsafe {
                        bzero(
                            top.add(first_chunk_offset),
                            top.add(first_chunk_offset + first_chunk_size),
                        );
                    }
                }
                bytes_to_copy -= first_chunk_size;

                // Part 2: Copy any full pages.
                while bytes_to_copy >= PAGE_SIZE {
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;
                    // println!(
                    //     "Copying next page from {:08x} {:08x} ({} bytes to go...)",
                    //     src_addr as usize, top as usize, bytes_to_copy
                    // );
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top, src_addr, PAGE_SIZE);
                            src_addr = src_addr.add(PAGE_SIZE);
                        }
                    } else {
                        unsafe { bzero(top, top.add(PAGE_SIZE)) };
                    }
                    bytes_to_copy -= PAGE_SIZE;
                }

                // Part 3: Copy the final residual partial page
                if bytes_to_copy > 0 {
                    if VDBG {println!(
                        "Copying final section -- {} bytes @ {:08x}",
                        bytes_to_copy,
                        section.virt + (section.len() as u32) - (bytes_to_copy as u32)
                    );}
                    cfg.extra_pages += 1;
                    top = cfg.get_top() as *mut u8;
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top, src_addr, bytes_to_copy);
                            src_addr = src_addr.add(bytes_to_copy);
                        }
                    } else {
                        unsafe { bzero(top, top.add(bytes_to_copy)) };
                    }
                }

                previous_addr = section.virt as usize + section.len();
                page_addr = previous_addr & !(PAGE_SIZE - 1);
                if VDBG {
                    println!("Looping to the next section");
                    println!("top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
                    println!("previous_addr: {:x}, page_addr: {:x}", previous_addr, page_addr);
                }
            }

            println!("Done with sections");
            if previous_addr as usize & (PAGE_SIZE - 1) != 0 {
                println!("Zeroing out remaining data");
                // Zero-out the trailing bytes
                unsafe {
                    bzero(
                        top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                        top.add(PAGE_SIZE as usize),
                    )
                };
            } else {
                println!("Skipping zero step -- we ended on a page boundary");
            }
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            let prog = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };

            // TEXT SECTION
            // Round it off to a page boundary
            let load_size_rounded = (prog.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            cfg.extra_pages += load_size_rounded / PAGE_SIZE;
            let top = cfg.get_top();
            println!("\n\nKernel top: {:x}, extra_pages: {:x}", top as u32, cfg.extra_pages);
            unsafe {
                // Copy the program to the target address, rounding it off to the load size.
                let src_addr = cfg
                    .base_addr
                    .add(prog.load_offset as usize / mem::size_of::<usize>());
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
                let src_addr = cfg.base_addr.add(
                    (prog.load_offset + prog.text_size + 4) as usize / mem::size_of::<usize>() - 1,
                );
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
            };
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            _pid += 1;
            let mut page_addr: usize = 0;
            let mut previous_addr: usize = 0;
            let mut previous_ram_zero_addr: usize = 0;
            let mut top = core::ptr::null_mut::<u8>();

            let inif = MiniElf::new(&tag);
            let mut src_addr = unsafe {
                cfg.base_addr
                    .add(inif.load_offset as usize / mem::size_of::<usize>())
            } as *const u8;
            // let mut accumulated_len = 0;

            println!("\n\nIniF {} has {} sections", _pid, inif.sections.len());
            println!("Initial top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
            for section in inif.sections.iter() {
                let flags = section.flags() as u8;
                // any section that requires "write" must be copied to RAM
                // note that ELF helpfully adds a 4096-byte gap between non-write pages and write-pages
                // allowing us to just trundle through the pages and not have to deal with partially
                // writeable pages.
                let copy_to_ram = flags & MINIELF_FLG_W != 0;
                if (section.virt as usize) < previous_addr {
                    panic!("init section addresses are not strictly increasing (new virt: {:08x}, last virt: {:08x})", section.virt, previous_addr);
                }
                if copy_to_ram {
                    let this_page = section.virt as usize & !(PAGE_SIZE - 1);
                    let mut bytes_to_copy = section.len();

                    if VDBG {println!(
                        "Section is {} bytes long, loaded to {:08x}",
                        bytes_to_copy, section.virt
                    );}
                    // If this is not a new page, ensure the uninitialized values from between
                    // this section and the previous one are all zeroed out.
                    if this_page != page_addr || previous_addr == page_addr {
                        if VDBG {println!("New page @ {:08x}", this_page);}
                        if previous_ram_zero_addr != 0 && previous_ram_zero_addr != page_addr && cfg.extra_pages > 0 {
                            if VDBG {println!(
                                "Zeroing-out remainder of previous page: {:08x} (mapped to physical address {:08x})",
                                previous_ram_zero_addr, top as usize,
                            );}
                            unsafe {
                                bzero(
                                    top.add(previous_ram_zero_addr as usize & (PAGE_SIZE - 1)),
                                    top.add(PAGE_SIZE as usize),
                                )
                            };
                        }

                        // Allocate a new page.
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;

                        // Zero out the page, if necessary.
                        unsafe { bzero(top, top.add(section.virt as usize & (PAGE_SIZE - 1))) };
                    } else {
                        if VDBG {println!("Reusing existing page @ {:08x}", this_page);}
                    }

                    // Part 1: Copy the first chunk over.
                    let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if first_chunk_size > section.len() {
                        first_chunk_size = section.len();
                    }
                    let first_chunk_offset = section.virt as usize & (PAGE_SIZE - 1);
                    if VDBG {println!(
                        "Section chunk is {} bytes, {} from {:08x}:{:08x} -> {:08x}:{:08x} (virt: {:08x})",
                        first_chunk_size,
                        if section.no_copy() { "zeroing" } else { "copying" },
                        src_addr as usize,
                        unsafe { src_addr.add(first_chunk_size) as usize },
                        unsafe { top.add(first_chunk_offset) as usize },
                        unsafe { top.add(first_chunk_size + first_chunk_offset)
                            as usize },
                        this_page + first_chunk_offset,
                    );}
                    // Perform the copy, if NOCOPY is not set
                    if !section.no_copy() {
                        unsafe {
                            memcpy(top.add(first_chunk_offset), src_addr, first_chunk_size);
                            src_addr = src_addr.add(first_chunk_size);
                        }
                    } else {
                        unsafe {
                            bzero(
                                top.add(first_chunk_offset),
                                top.add(first_chunk_offset + first_chunk_size),
                            );
                        }
                    }
                    bytes_to_copy -= first_chunk_size;

                    // Part 2: Copy any full pages.
                    while bytes_to_copy >= PAGE_SIZE {
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;
                        // println!(
                        //     "Copying next page from {:08x} {:08x} ({} bytes to go...)",
                        //     src_addr as usize, top as usize, bytes_to_copy
                        // );
                        if !section.no_copy() {
                            unsafe {
                                memcpy(top, src_addr, PAGE_SIZE);
                                src_addr = src_addr.add(PAGE_SIZE);
                            }
                        } else {
                            unsafe { bzero(top, top.add(PAGE_SIZE)) };
                        }
                        bytes_to_copy -= PAGE_SIZE;
                    }

                    // Part 3: Copy the final residual partial page
                    if bytes_to_copy > 0 {
                        if VDBG {println!(
                            "Copying final section -- {} bytes @ {:08x}",
                            bytes_to_copy,
                            section.virt + (section.len() as u32) - (bytes_to_copy as u32)
                        );}
                        cfg.extra_pages += 1;
                        top = cfg.get_top() as *mut u8;
                        if !section.no_copy() {
                            unsafe {
                                memcpy(top, src_addr, bytes_to_copy);
                                src_addr = src_addr.add(bytes_to_copy);
                            }
                        } else {
                            unsafe { bzero(top, top.add(bytes_to_copy)) };
                        }
                    }
                    previous_ram_zero_addr = section.virt as usize + section.len();
                } else {
                    top = cfg.get_top() as *mut u8;
                    src_addr = unsafe {src_addr.add(section.len())};
                }

                previous_addr = section.virt as usize + section.len();
                page_addr = previous_addr & !(PAGE_SIZE - 1);
                if VDBG {
                    println!("Looping to the next section");
                    println!("top: {:x}, extra_pages: {:x}, init_size: {:x}, base_addr: {:x}", cfg.get_top() as *mut u8 as u32, cfg.extra_pages, cfg.init_size, cfg.base_addr as u32);
                    println!("previous_addr: {:x}, page_addr: {:x}", previous_addr, page_addr);
                }
            }

            println!("Done with sections");
            if previous_addr as usize & (PAGE_SIZE - 1) != 0 {
                println!("Zeroing out remaining data");
                // Zero-out the trailing bytes
                unsafe {
                    bzero(
                        top.add(previous_addr as usize & (PAGE_SIZE - 1)),
                        top.add(PAGE_SIZE as usize),
                    )
                };
            } else {
                println!("Skipping zero step -- we ended on a page boundary");
            }
        }
    }
}

unsafe fn memcpy<T>(dest: *mut T, src: *const T, count: usize)
where
    T: Copy,
{
    if VDBG {println!(
        "COPY (align {}): {:08x} - {:08x} {} {:08x} - {:08x}",
        mem::size_of::<T>(),
        src as usize,
        src as usize + count,
        count,
        dest as usize,
        dest as usize + count
    );}
    core::ptr::copy_nonoverlapping(src, dest, count / mem::size_of::<T>());
}