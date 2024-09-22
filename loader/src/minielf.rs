use core::{mem, slice};

#[cfg(feature = "atsama5d27")]
use armv7::structures::paging::TranslationTableMemory;
#[cfg(all(feature = "atsama5d27", feature = "debug-print"))]
use armv7::{
    VirtualAddress,
    structures::paging::{
        InMemoryRegister, PageTableDescriptor, Readable, SMALL_PAGE_FLAGS, TranslationTableDescriptor,
        TranslationTableType,
    },
};

use crate::*;

pub const MINIELF_FLG_W: u8 = 1;
#[allow(dead_code)]
pub const MINIELF_FLG_NC: u8 = 2;
#[allow(dead_code)]
pub const MINIELF_FLG_X: u8 = 4;
#[allow(dead_code)]
pub const MINIELF_FLG_EHF: u8 = 8;
#[allow(dead_code)]
pub const MINIELF_FLG_EHH: u8 = 0x10;

#[repr(C)]
#[derive(Debug)]
pub struct MiniElfSection {
    // Virtual address of this section
    pub virt: u32,

    // A combination of the size and flags
    size_and_flags: u32,
}

impl MiniElfSection {
    pub fn len(&self) -> usize {
        // Strip off the top four bits, which contain the flags.
        let len = self.size_and_flags & !0xff00_0000;
        len as usize
    }

    pub fn flags(&self) -> usize {
        let le_bytes = self.size_and_flags >> 24;
        le_bytes as usize
    }

    pub fn no_copy(&self) -> bool { self.size_and_flags & (1 << 25) != 0 }
}

/// Describes a Mini ELF file, suitable for loading into RAM
pub struct MiniElf {
    /// Offset of the source data relative to the start of the image file
    pub load_offset: u32,

    /// Virtual address of the entrypoint
    pub entry_point: u32,

    /// All of the sections inside this file
    pub sections: &'static [MiniElfSection],
}

impl MiniElf {
    pub fn new(tag: &KernelArgument) -> Self {
        let ptr = tag.data.as_ptr();
        unsafe {
            MiniElf {
                load_offset: ptr.add(0).read(),
                entry_point: ptr.add(1).read(),
                sections: slice::from_raw_parts(
                    ptr.add(2) as *mut MiniElfSection,
                    (tag.size as usize - 8) / mem::size_of::<MiniElfSection>(),
                ),
            }
        }
    }

    /// Load the process into its own memory space.
    /// The process will have been already loaded in stage 1.  This simply assigns
    /// memory maps as necessary.
    pub fn load(
        &self,
        allocator: &mut BootConfig,
        load_offset: usize,
        pid: XousPid,
        params: &[u8],
        ini_type: IniType,
    ) -> usize {
        println!("Mapping PID {} starting at offset {:08x}", pid, load_offset);
        let mut allocated_bytes = 0;

        let mut current_page_addr: usize = 0;
        let mut previous_addr: usize = 0;
        let mut last_mapped_xip = 0;
        let image_phys_base = allocator.base_addr as usize + self.load_offset as usize;
        // It is a requirement that the image generator lay out the artifacts on disk such that
        // the page offsets line up for XIP sections. This assert confirms this necessary pre-condition.
        match ini_type {
            IniType::IniF => {
                assert!(
                    (image_phys_base & (PAGE_SIZE - 1)) == self.sections[0].virt as usize & (PAGE_SIZE - 1),
                    "Image generator did not align load offsets to page offsets!"
                );
            }
            _ => {
                println!(
                    "flash_map_offset: {:x} / base_addr {:x} load_offset {:x}",
                    image_phys_base as usize, allocator.base_addr as usize, self.load_offset as usize
                );
            }
        }

        // The load offset is the end of this process.  Shift it down by one page
        // so we get the start of the first page.
        let mut top = load_offset - PAGE_SIZE;
        let stack_addr = USER_STACK_TOP - USER_STACK_PADDING;

        // Allocate a page to handle the top-level memory translation
        #[cfg(not(feature = "atsama5d27"))]
        let (tt, _tt_address) = {
            let tt_address = allocator.alloc() as usize;
            let tt = unsafe { &mut *(tt_address as *mut PageTable) };

            allocator.map_page(
                tt,
                tt_address,
                PAGE_TABLE_ROOT_OFFSET,
                FLG_R | FLG_W | FLG_VALID,
                pid as XousPid,
            );
            #[cfg(feature = "swap")]
            allocator.mark_as_wired(tt_address); // don't allow the tt to be swapped in any process

            (tt, tt_address)
        };
        #[cfg(feature = "atsama5d27")]
        let (tt, _tt_address) = {
            let pid_idx = pid as usize - 1;

            // Allocate a page to handle the top-level memory translation
            let tt_address = allocator.alloc_l1_page_table(pid) as usize;
            allocator.processes[pid_idx].ttbr0 = tt_address;

            let translation_table = tt_address as *mut TranslationTableMemory;
            // Map all four pages of the translation table to the process' virtual address space
            for offset in 0..4 {
                let offset = offset * PAGE_SIZE;
                allocator.map_page(
                    translation_table,
                    tt_address + offset,
                    PAGE_TABLE_ROOT_OFFSET + offset,
                    FLG_R | FLG_W | FLG_VALID,
                    pid as XousPid,
                );
                #[cfg(feature = "swap")]
                allocator.mark_as_wired(tt_address + offset); // don't allow the tt to be swapped in any process
            }

            (translation_table, tt_address)
        };

        // Turn the satp address into a pointer
        println!("    Pagetable @ {:08x}", _tt_address);

        // Allocate thread contexts
        let thread_address = allocator.alloc() as usize;
        println!("    Thread contexts @ {:08x}", thread_address);
        allocator.map_page(tt, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID, pid);
        #[cfg(feature = "swap")]
        allocator.mark_as_wired(thread_address); // don't allow the thread contexts to be swapped in any process

        // Allocate stack pages.
        println!("    Stack");
        for i in 0..STACK_PAGE_COUNT {
            if i == 0 {
                // For the initial stack frame, allocate a valid page
                let sp_page = allocator.alloc() as usize;
                // Copy the params block into this first page
                let sp_page_slice = unsafe { slice::from_raw_parts_mut(sp_page as *mut u8, PAGE_SIZE) };
                let params_start = sp_page_slice.len() - params.len();
                sp_page_slice[params_start..].copy_from_slice(params);

                // Attach the page to the process
                allocator.map_page(
                    tt,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W | FLG_VALID,
                    pid,
                );
            } else {
                // Reserve every other stack page other than the 1st page.
                allocator.map_page(
                    tt,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W,
                    pid,
                );
            }
        }

        // this works to set the initial offset, but from here we have to track it by
        // adding the length of each section as we see it
        let mut section_start_phys_offset = 0;
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
        // Example: Page starts at 0xf0c0 and is 128 bytes long
        // 1. Copy 128 bytes to page 1
        for section in self.sections {
            if VDBG {
                println!("    Section @ {:08x}", section.virt as usize);
            }
            let flag_defaults = FLG_U
                | FLG_R
                | FLG_X
                | FLG_VALID
                | if section.flags() & 1 == 1 { FLG_W } else { 0 }
                | if section.flags() & 4 == 4 { FLG_X } else { 0 };

            let copy_to_ram = ((section.flags() as u8) & MINIELF_FLG_W) != 0;
            if (section.virt as usize) < previous_addr {
                panic!("init section addresses are not strictly increasing");
            }

            if (copy_to_ram || ini_type == IniType::IniE) && (ini_type != IniType::IniS) {
                let mut this_page = section.virt as usize & !(PAGE_SIZE - 1);
                let mut bytes_to_copy = section.len();

                // If this is not a new page, ensure the uninitialized values from between
                // this section and the previous one are all zeroed out.
                if this_page != current_page_addr || previous_addr == current_page_addr {
                    if VDBG {
                        println!("1       {:08x} -> {:08x}", top as usize, this_page);
                    }
                    allocator.map_page(tt, top as usize, this_page, flag_defaults, pid as XousPid);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                    this_page += PAGE_SIZE;

                    // Part 1: Copy the first chunk over.
                    let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if first_chunk_size > section.len() {
                        first_chunk_size = section.len();
                    }
                    bytes_to_copy -= first_chunk_size;
                } else {
                    if VDBG {
                        println!(
                            "This page is {:08x}, and last page was {:08x}",
                            this_page, current_page_addr
                        );
                    }
                    // This is a continuation of the previous section, and as a result
                    // the memory will have been copied already. Avoid copying this data
                    // to a new page.
                    let first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                    if VDBG {
                        println!("First chunk size: {}", first_chunk_size);
                    }
                    if bytes_to_copy < first_chunk_size {
                        bytes_to_copy = 0;
                        if VDBG {
                            println!("Clamping to 0 bytes");
                        }
                    } else {
                        bytes_to_copy -= first_chunk_size;
                        if VDBG {
                            println!(
                                "Clamping to {} bytes by cutting off {} bytes",
                                bytes_to_copy, first_chunk_size
                            );
                        }
                    }
                    this_page += PAGE_SIZE;
                }

                // Part 2: Copy any full pages.
                while bytes_to_copy >= PAGE_SIZE {
                    if VDBG {
                        println!("2       {:08x} -> {:08x}", top as usize, this_page);
                    }
                    allocator.map_page(tt, top as usize, this_page, flag_defaults, pid as XousPid);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                    this_page += PAGE_SIZE;
                    bytes_to_copy -= PAGE_SIZE;
                }

                // Part 3: Copy the final residual partial page
                if bytes_to_copy > 0 {
                    let this_page = (section.virt as usize + section.len()) & !(PAGE_SIZE - 1);
                    if VDBG {
                        println!("3       {:08x} -> {:08x}", top as usize, this_page);
                    }
                    allocator.map_page(tt, top as usize, this_page, flag_defaults, pid as XousPid);
                    allocated_bytes += PAGE_SIZE;
                    top -= PAGE_SIZE;
                }
            } else {
                // --- calculate how many pages need mapping ---
                let mut bytes_to_map = section.len();
                assert!(bytes_to_map > 0, "no data to map");
                let mut pages_to_map = 1;
                let start_page = section.virt as usize & !(PAGE_SIZE - 1);

                let unaligned_start_len = (start_page + PAGE_SIZE) - section.virt as usize;
                if unaligned_start_len >= bytes_to_map {
                    // we're done: the page is already mapped and it holds all the data we intend to map
                } else {
                    // remaining data from the first aligned page to end of mapped region
                    bytes_to_map -= unaligned_start_len;
                    // convert this to pages_to_map
                    pages_to_map += bytes_to_map / PAGE_SIZE;
                    if (bytes_to_map % PAGE_SIZE) != 0 {
                        // unaligned end page adds one more mapped page
                        pages_to_map += 1;
                    }
                }

                // --- calculate starting offset of section from image base ---
                let mut section_map_phys_offset = section_start_phys_offset;

                // --- avoid double-mapping the previous section's end ---
                // check if last_mapped_xip has already been mapped so we don't double-map overlapping pages
                // assume: sections are always increasing in size
                let mut virt_page = start_page;
                if last_mapped_xip == start_page {
                    if VDBG {
                        println!(
                            "Skipping a page to avoid double-mapping: pa {:x} -> va {:x}",
                            (image_phys_base + section_start_phys_offset) & !(PAGE_SIZE - 1),
                            virt_page
                        );
                    }
                    virt_page += PAGE_SIZE;
                    section_map_phys_offset += PAGE_SIZE;
                    pages_to_map -= 1;
                }
                if VDBG {
                    println!("section is 0x{:x} bytes long; mapping {} pages", section.len(), pages_to_map);
                }

                // --- map FLASH or swap pages to virtual memory ---
                while pages_to_map > 0 {
                    let map_phys_addr = if ini_type != IniType::IniS {
                        (image_phys_base + section_map_phys_offset) & !(PAGE_SIZE - 1)
                    } else {
                        0 // IniS physical addresses are non-existent; make that abundantly clear
                    };
                    let flags = if ini_type == IniType::IniS {
                        flag_defaults & !FLG_VALID | FLG_P
                    } else {
                        flag_defaults
                    };
                    allocator.map_page(tt, map_phys_addr, virt_page, flags, pid as XousPid);
                    #[cfg(feature = "swap")]
                    if ini_type == IniType::IniS {
                        allocator.map_swap(allocator.last_swap_page * 0x1000, virt_page, pid);
                        allocator.last_swap_page += 1;
                    }
                    last_mapped_xip = virt_page;

                    section_map_phys_offset += PAGE_SIZE;
                    virt_page += PAGE_SIZE;
                    pages_to_map -= 1;
                }
            }
            section_start_phys_offset += section.len(); // the length of the section on disk

            previous_addr = section.virt as usize + section.len();
            current_page_addr = previous_addr & !(PAGE_SIZE - 1);
        }

        let process = &mut allocator.processes[pid as usize - 1];
        process.entrypoint = self.entry_point as usize;
        process.sp = (stack_addr - params.len()) & !0xf;
        process.env = stack_addr - params.len() + USER_STACK_PADDING;
        #[cfg(not(feature = "atsama5d27"))]
        {
            process.satp = 0x8000_0000 | ((pid as usize) << 22) | (_tt_address >> 12);
        }
        #[cfg(feature = "atsama5d27")]
        {
            process.asid = pid;
        }

        println!("load allocated 0x{:x} bytes", allocated_bytes);
        allocated_bytes
    }

    /// Page through a processes allocated pages and check against the file spec.
    #[cfg(any(feature = "debug-print", feature = "swap"))]
    pub fn check(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid, ini_type: IniType) {
        println!("Checking {:?} PID {} starting at offset {:08x}", ini_type, pid, load_offset);
        let image_phys_base = match ini_type {
            IniType::IniE | IniType::IniF => allocator.base_addr as usize + self.load_offset as usize,
            IniType::IniS => load_offset,
        };
        // the process offset is always 1 less than the PID, because that's how we built the table.
        #[cfg(not(feature = "atsama5d27"))]
        let tt = match ini_type {
            IniType::IniE | IniType::IniF => allocator.processes[pid as usize - 1].satp,
            IniType::IniS => {
                #[cfg(feature = "swap")]
                {
                    allocator.swap_root[pid as usize - 1] >> 12
                }
                #[cfg(not(feature = "swap"))]
                {
                    0
                }
            }
        };
        #[cfg(feature = "atsama5d27")]
        let tt = { allocator.processes[pid as usize - 1].ttbr0 };

        let mut section_offset = 0;
        for (_index, section) in self.sections.iter().enumerate() {
            match ini_type {
                IniType::IniE | IniType::IniF => {
                    if let Some(dest_offset) = pt_walk(tt, section.virt as usize) {
                        println!(
                            "  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), 0x{:x}(PA dst) len {}/0x{:x}",
                            _index,
                            section_offset + image_phys_base,
                            section.virt as usize,
                            dest_offset,
                            section.len(),
                            section.len()
                        );
                        // dumping routines
                        let dump_pa_src = section_offset + image_phys_base;
                        let dump_pa_dst = dest_offset;
                        let dump_pa_end_dst = pt_walk(tt, section.virt as usize + section.len() - 20);

                        dump_addr(dump_pa_src, "    Src [:20]  ");
                        dump_addr(dump_pa_dst, "    Dst [:20]  ");
                        dump_addr(dump_pa_src + section.len() - 20, "    Src [-20:] ");
                        // recompute the end section mapping, because PA/VA mappings don't have to be linear
                        // (in fact they go in the opposite direction)
                        if let Some(pa_dst_end) = dump_pa_end_dst {
                            dump_addr(pa_dst_end, "    Dst [-20:] ");
                        } else {
                            println!(
                                "   End of destination VA 0x{:x}, ERR UNMAPPED!",
                                section.virt as usize + section.len() - 20
                            );
                        }
                    } else {
                        println!(
                            "  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), ERR UNMAPPED!!",
                            _index,
                            section_offset + image_phys_base,
                            section.virt as usize + section_offset
                        );
                    }
                }
                IniType::IniS => {
                    #[cfg(feature = "swap")]
                    if let Some(dest_offset) = pt_walk_swap(
                        tt,
                        section.virt as usize,
                        allocator.processes[SWAPPER_PID as usize - 1].satp,
                    ) {
                        println!(
                            "  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), 0x{:x}(PA dst) len {}/0x{:x}",
                            _index,
                            section_offset + image_phys_base,
                            section.virt as usize,
                            dest_offset,
                            section.len(),
                            section.len()
                        );

                        // dumping routines
                        let dump_pa_src = section_offset + image_phys_base;
                        let dump_pa_end_dst = pt_walk_swap(
                            tt,
                            section.virt as usize + section.len() - 20,
                            allocator.processes[SWAPPER_PID as usize - 1].satp,
                        );
                        #[cfg(feature = "swap")]
                        if let Some(swap) = allocator.swap_hal.as_mut() {
                            if !section.no_copy() {
                                let dump_disk = swap.decrypt_src_page_at(dump_pa_src & !(PAGE_SIZE - 1));
                                dump_slice(&dump_disk[dump_pa_src & (PAGE_SIZE - 1)..], "    Src [:20]  ");
                            } else {
                                println!("    -- nocopy --");
                            };
                            let dump_swap = swap.decrypt_swap_from(
                                dest_offset & !(PAGE_SIZE - 1),
                                (section.virt as usize) & !(PAGE_SIZE - 1),
                                pid,
                            );
                            dump_slice(
                                &dump_swap[(section.virt as usize) & (PAGE_SIZE - 1)..],
                                "    Dst [:20]  ",
                            );
                            if !section.no_copy() {
                                let dump_disk = swap.decrypt_src_page_at(
                                    (dump_pa_src + section.len() - 20) & !(PAGE_SIZE - 1),
                                );
                                dump_slice(
                                    &dump_disk[(dump_pa_src + section.len() - 20) & (PAGE_SIZE - 1)..],
                                    "    Src [-20:] ",
                                );
                            } else {
                                println!("    -- nocopy --");
                            }
                            // recompute the end section mapping, because PA/VA mappings don't have to be
                            // linear (in fact they go in the opposite direction)
                            if let Some(pa_dst_end) = dump_pa_end_dst {
                                let dump_swap = swap.decrypt_swap_from(
                                    pa_dst_end & !(PAGE_SIZE - 1),
                                    (section.virt as usize + section.len() - 20) & !(PAGE_SIZE - 1),
                                    pid,
                                );
                                dump_slice(
                                    &dump_swap
                                        [(section.virt as usize + section.len() - 20) & (PAGE_SIZE - 1)..],
                                    "    Dst [-20:] ",
                                );
                            } else {
                                println!(
                                    "   End of destination VA 0x{:x}, ERR UNMAPPED!",
                                    section.virt as usize + section.len() - 20
                                );
                            }
                        }
                    } else {
                        println!(
                            "  Section {} start 0x{:x}(PA src), 0x{:x}(VA dst), ERR UNMAPPED!!",
                            _index,
                            section_offset + image_phys_base,
                            section.virt as usize + section_offset
                        );
                    }
                }
            }
            section_offset += section.len();
        }
    }
}

#[cfg(any(feature = "debug-print", feature = "swap"))]
fn dump_addr(addr: usize, _label: &str) {
    // A lot of variables are _'d because when debug-print is turned off, the variable resolves
    // as unused (because the print macros are dummies).
    print!("{}", _label);
    let slice = unsafe { core::slice::from_raw_parts(addr as *const u8, 20) };
    for &_b in slice {
        print!("{:02x}", _b);
    }
    print!("\n\r");
}
#[cfg(feature = "swap")]
fn dump_slice(slice: &[u8], _label: &str) {
    print!("{}", _label);
    // handle case that our decrypt region isn't 20 bytes long...
    let len = slice.len().min(20);
    for &_b in &slice[..len] {
        print!("{:02x}", _b);
    }
    print!("\n\r");
}

#[cfg(all(any(feature = "debug-print", feature = "swap"), not(feature = "atsama5d27")))]
pub fn pt_walk(root: usize, va: usize) -> Option<usize> {
    let l1_pt = unsafe { &mut (*((root << 12) as *mut PageTable)) };
    let l1_entry = l1_pt.entries[(va & 0xFFC0_0000) >> 22];
    if l1_entry != 0 {
        let l0_pt = unsafe { &mut (*(((l1_entry >> 10) << 12) as *mut PageTable)) };
        let l0_entry = l0_pt.entries[(va & 0x003F_F000) >> 12];
        if l0_entry & 1 != 0 {
            // bit 1 is the "valid" bit
            Some(((l0_entry >> 10) << 12) | va & 0xFFF)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(all(any(feature = "debug-print", feature = "swap"), not(feature = "atsama5d27")))]
pub fn pt_walk_swap(root: usize, va: usize, swap_root: usize) -> Option<usize> {
    let l1_pt = unsafe { &mut (*((root << 12) as *mut PageTable)) };
    let l1_entry_va = (l1_pt.entries[(va & 0xFFC0_0000) >> 22] >> 10) << 12;
    if l1_entry_va != 0 {
        // this entry is a *virtual address*, mapped into the PID 2 space. Resolve it.
        let l1_entry = pt_walk(swap_root, l1_entry_va).expect("Physical address should exist!");

        let l0_pt = unsafe { &mut (*(l1_entry as *mut PageTable)) };
        let l0_entry = l0_pt.entries[(va & 0x003F_F000) >> 12];
        if l0_entry & 1 != 0 {
            // bit 1 is the "valid" bit
            Some(((l0_entry >> 10) << 12) | va & 0xFFF)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(all(feature = "debug-print", feature = "atsama5d27"))]
pub fn pt_walk(root: usize, va: usize) -> Option<usize> {
    if va & 3 != 0 {
        return None;
    }
    let v = VirtualAddress::new(va as u32);
    let vpn1 = v.translation_table_index();
    let vpn2 = v.page_table_index();
    assert!(vpn1 < 4096);
    assert!(vpn2 < 256);

    let existing_l1_entry =
        unsafe { ((root as *mut u32).add(vpn1) as *mut TranslationTableDescriptor).read_volatile() };
    if existing_l1_entry.get_type() == TranslationTableType::Invalid {
        return None;
    }
    let l2_pt_addr = (existing_l1_entry.as_u32() & 0xfff) + vpn1 as u32 * PAGE_SIZE as u32;
    let existing_l2_entry_addr = unsafe { (l2_pt_addr as *mut u32).add(vpn2) as *mut PageTableDescriptor };
    let current_entry: PageTableDescriptor = unsafe { existing_l2_entry_addr.read_volatile() };
    let flags_u32 = current_entry.get_flags().expect("flags");
    let flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> = InMemoryRegister::new(flags_u32);
    let is_valid = flags.read(SMALL_PAGE_FLAGS::VALID) != 0;
    let phys = (current_entry.as_u32() & !0xfff) as usize;
    if is_valid {
        return Some(phys);
    }

    None
}
