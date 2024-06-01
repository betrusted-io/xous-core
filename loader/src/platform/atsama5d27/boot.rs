// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::mem;
use core::num::NonZeroUsize;

use armv7::structures::paging::{
    PageTable as L2PageTable, PageTableDescriptor, PageTableMemory, PageTableType, TranslationTable,
    TranslationTableDescriptor, TranslationTableMemory, TranslationTableType, PAGE_TABLE_FLAGS,
    SMALL_PAGE_FLAGS,
};
use armv7::{PhysicalAddress, VirtualAddress};

use crate::consts::{
    EXCEPTION_STACK_TOP, FLG_R, FLG_U, FLG_VALID, FLG_W, FLG_X, GUARD_MEMORY_BYTES, IRQ_STACK_TOP,
    KERNEL_ARGUMENT_OFFSET, KERNEL_STACK_PAGE_COUNT, LOADER_CODE_ADDRESS, PAGE_TABLE_OFFSET,
};
use crate::platform::atsama5d27::load::InitialProcess;
use crate::{bzero, println, BootConfig, XousPid, PAGE_SIZE, WORD_SIZE};

const DEBUG_PAGE_MAPPING: bool = false;
macro_rules! dprint {
    ($($args:tt)*) => ({
        if DEBUG_PAGE_MAPPING {
            crate::print!($($args)*)
        }
    });
}
macro_rules! dprintln {
    ($($args:tt)*) => ({
        if DEBUG_PAGE_MAPPING {
            crate::println!($($args)*)
        }
    });
}

impl BootConfig {
    pub fn get_top(&self) -> *mut usize {
        let val = unsafe {
            self.sram_start.add(
                (self.sram_size - self.init_size - self.extra_pages * PAGE_SIZE) / mem::size_of::<usize>(),
            )
        };
        assert!((val as usize) >= (self.sram_start as usize));
        assert!(
            (val as usize) < (self.sram_start as usize) + self.sram_size,
            "top address {:08x} > (start + size) {:08x} + {} = {:08x}",
            val as usize,
            self.sram_start as usize,
            self.sram_size,
            self.sram_start as usize + self.sram_size
        );
        val
    }

    /// Zero-alloc a new page, mark it as owned by PID1, and return it.
    /// Decrement the `next_page_offset` (npo) variable by one page.
    pub fn alloc(&mut self) -> *mut usize {
        self.extra_pages += 1;
        let pg = self.get_top();
        unsafe {
            // Grab the page address and zero it out
            bzero(pg as *mut usize, pg.add(PAGE_SIZE / mem::size_of::<usize>()) as *mut usize);
        }
        // Mark this page as in-use by the kernel
        let extra_bytes = self.extra_pages * PAGE_SIZE;
        self.runtime_page_tracker[(self.sram_size - (extra_bytes + self.init_size)) / PAGE_SIZE] =
            XousPid::from(1);

        dprintln!("Allocated a physical page: {:08x}", pg as usize);

        // Return the address
        pg as *mut usize
    }

    /// Allocates four 4K pages for L1 translation table
    /// May waste some pages as dummy pages due to alignment requirements.
    /// Sometimes there are none, but could be up to 3 pages wasted.
    pub fn alloc_l1_page_table(&mut self, pid: XousPid) -> *mut usize {
        // ARMv7A Level 1 Translation Table is required to be aligned at 16K boundary
        const ALIGNMENT_16K: usize = 16 * 1024;

        // It should take no more than 4 tries to get to the next 16K-aligned 4K sized page
        let mut num_alloc_pages = 0;
        for _ in 0..4 {
            let mut allocated_page_ptr = self.alloc();
            #[cfg(feature = "swap")]
            self.mark_as_wired(allocated_page_ptr);
            num_alloc_pages += 1;
            let is_aligned = allocated_page_ptr as usize & (ALIGNMENT_16K - 1) == 0;
            self.change_owner(pid, allocated_page_ptr as usize);

            if is_aligned {
                return if num_alloc_pages != 4 {
                    dprintln!(
                        "Allocated a dummy page (aligned but not enough pages allocated yet): {:08x}",
                        allocated_page_ptr as usize
                    );

                    // Allocate 4 more pages for a whole L1 translation table
                    for _ in 0..4 {
                        dprintln!(
                            "Allocated a page {:08x} for PID {} L1 page table",
                            allocated_page_ptr as usize,
                            pid
                        );
                        allocated_page_ptr = self.alloc();
                        #[cfg(feature = "swap")]
                        self.mark_as_wired(allocated_page_ptr);
                        self.change_owner(pid, allocated_page_ptr as usize);
                    }

                    dprintln!("Allocated a L1 page table at {:08x}", allocated_page_ptr as usize);

                    allocated_page_ptr
                } else {
                    allocated_page_ptr
                };
            } else {
                dprintln!("Allocated a dummy page for alignment: {:08x}", allocated_page_ptr as usize);
            }
        }

        unreachable!("Couldn't allocate a 16K-aligned page for L1 page table base")
    }

    pub fn change_owner(&mut self, pid: XousPid, addr: usize) {
        dprintln!("A new owner of {:08x} page is {}", addr, pid);

        // First, check to see if the region is in RAM,
        if addr >= self.sram_start as usize && addr < self.sram_start as usize + self.sram_size {
            // Mark this page as in-use by the PID
            self.runtime_page_tracker[(addr - self.sram_start as usize) / PAGE_SIZE] = XousPid::from(pid);
            return;
        }
        // The region isn't in RAM, so check the other memory regions.
        let mut rpt_offset = self.sram_size / PAGE_SIZE;

        for region in self.regions.iter() {
            let rstart = region.start as usize;
            let rlen = region.length as usize;
            if addr >= rstart && addr < rstart + rlen {
                self.runtime_page_tracker[rpt_offset + (addr - rstart) / PAGE_SIZE] = XousPid::from(pid);
                return;
            }
            rpt_offset += rlen / PAGE_SIZE;
        }
        panic!("Tried to change region {:08x} that isn't in defined memory!", addr);
    }

    /// Map the given page to the specified process table.  If necessary,
    /// allocate a new page.
    ///
    /// # Panics
    ///
    /// * If you try to map a page twice
    pub fn map_page(
        &mut self,
        translation_table: *mut TranslationTableMemory,
        phys: usize,
        virt: usize,
        flags: usize,
        pid: XousPid,
    ) {
        assert!(!(phys == 0 && flags & FLG_VALID != 0), "cannot map zero page");
        if flags & FLG_VALID != 0 {
            self.change_owner(owner, phys);
        }
        match WORD_SIZE {
            4 => self.map_page_32(translation_table, phys, virt, flags),
            8 => panic!("map_page doesn't work on 64-bit devices"),
            _ => panic!("unrecognized word size: {}", WORD_SIZE),
        }
    }

    pub fn map_page_32(
        &mut self,
        translation_table: *mut TranslationTableMemory,
        phys: usize,
        virt: usize,
        flags: usize,
        pid: XousPid,
    ) {
        dprintln!("PageTable: {:p} {:08x}", translation_table, translation_table as usize);
        dprint!("MAP: p0x{:08x} -> v0x{:08x} ", phys, virt);
        print_flags(flags);
        dprintln!();

        let v = VirtualAddress::new(virt as u32);
        let vpn1 = v.translation_table_index();
        let vpn2 = v.page_table_index();

        let p = phys & !(0xfff);
        let ppn2 = (p >> 12) & 0xff;

        assert!(vpn1 < 4096);
        assert!(vpn2 < 256);
        assert!(ppn2 < 256);

        dprintln!("vpn1: {:04x}, vpn2: {:02x}, ppn2: {:08x}, phys frame addr: {:08x}", vpn1, vpn2, ppn2, p);

        let mut tt = TranslationTable::new(translation_table);
        let tt = unsafe { tt.table_mut() };
        let mut new_addr = None;

        // Allocate a new level 1 translation table entry if one doesn't exist.
        dprintln!("tt[{:08x}] = {:032b}", vpn1, tt[vpn1]);

        if tt[vpn1].get_type() == TranslationTableType::Invalid {
            dprintln!("Previously unmapped L1 entry");

            let na = self.alloc();
            #[cfg(feature = "swap")]
            self.mark_as_wired(na);
            let phys = PhysicalAddress::from_ptr(na);
            let entry_flags =
                u32::from(PAGE_TABLE_FLAGS::VALID::Enable) | u32::from(PAGE_TABLE_FLAGS::DOMAIN.val(0xf));
            let descriptor = TranslationTableDescriptor::new(TranslationTableType::Page, phys, entry_flags)
                .expect("tt descriptor");
            dprintln!("New TT descriptor: {:032b}", descriptor);
            tt[vpn1] = descriptor;

            dprintln!("new tt[{:08x}] = {:032b}", vpn1, tt[vpn1]);
            new_addr = Some(NonZeroUsize::new(na as usize).unwrap());
        }

        let existing_entry = tt[vpn1];
        dprintln!("existing tt[{:08x}] = {:032b}", vpn1, existing_entry);
        match existing_entry.get_type() {
            TranslationTableType::Page => {
                let l2_phys_addr = existing_entry.get_addr().expect("invalid l1 entry");
                let ptr: *mut PageTableMemory = l2_phys_addr.as_mut_ptr();
                let mut l2_pt = unsafe { L2PageTable::new_from_ptr(ptr) };
                let l2_pt = unsafe { l2_pt.table_mut() };

                dprintln!("l2 ptr: {:p}", l2_pt);

                let existing_l2_entry = l2_pt[vpn2];

                dprintln!("({:08x}) l2_pt[{:08x}] = {:032b}", l2_phys_addr, vpn2, existing_l2_entry);

                if existing_l2_entry.get_type() == PageTableType::SmallPage {
                    let mapped_addr =
                        existing_l2_entry.get_addr().expect("invalid l2 entry").as_u32() as usize;

                    dprintln!("L2 entry {:02x} already mapped to {:08x}", vpn2, mapped_addr);

                    // Ensure the entry hasn't already been mapped to a different address.
                    if mapped_addr != p {
                        panic!(
                            "Page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
                            virt, mapped_addr, phys
                        );
                    }
                }

                // Map the L2 entry
                let mut small_page_flags = 0;
                let is_valid = flags & FLG_VALID != 0;
                if is_valid {
                    small_page_flags |= u32::from(SMALL_PAGE_FLAGS::VALID::Enable);

                    if flags & FLG_X == 0 {
                        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::XN::Enable);
                    }
                }

                if flags & FLG_U != 0 {
                    small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP::FullAccess);
                }
                if flags & FLG_W == 0 {
                    small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP2::Enable);
                }

                let new_entry = PageTableDescriptor::new(
                    PageTableType::SmallPage,
                    PhysicalAddress::new(p as u32),
                    small_page_flags,
                )
                .expect("new l2 entry");
                l2_pt[vpn2] = new_entry;
                dprintln!("new ({:08x}) l2_pt[{:08x}] = {:032b}", l2_phys_addr, vpn2, l2_pt[vpn2]);

                // If we had to allocate a translation table (L1) entry, ensure that it's
                // mapped into our address space, owned by PID 1.
                if let Some(addr) = new_addr {
                    let page_virt_addr = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
                    dprintln!(
                        ">>> Recursively mapping new address {:08x} -> {:08x}",
                        addr.get(),
                        page_virt_addr,
                    );
                    self.map_page(
                        translation_table,
                        addr.get(),
                        page_virt_addr,
                        FLG_R | FLG_W | FLG_VALID,
                        pid,
                    );
                    dprintln!("<<< Done mapping new address");
                }
            }

            _ => panic!("Invalid translation table entry type: {:?}", existing_entry.get_type()),
        }
    }
}

pub fn map_structs_to_kernel(cfg: &mut BootConfig, table_addr: usize, krn_struct_start: usize) {
    let tt = table_addr as *mut TranslationTableMemory;

    // Create a transparent mapping for a single page of the loader code.
    // The loader code will setup and enable MMU and then jump to the kernel entrypoint.
    // We want to map the same virtual address to the same physical address so it won't fail
    // as soon as the MMU is enabled
    dprintln!("Making the first 4K of the loader code visible");
    let translation_table = cfg.processes[0].ttbr0 as *mut TranslationTableMemory;
    cfg.map_page(
        translation_table,
        LOADER_CODE_ADDRESS,
        LOADER_CODE_ADDRESS,
        FLG_R | FLG_X | FLG_VALID,
        1 as XousPid,
    );

    // Map the last stack page (4K) to the kernel to make it visible from the trampoline code
    // Otherwise some arguments passed via stack won't be available after the MMU is turned on
    dprintln!("Making the loader stack visible");
    cfg.map_page(translation_table, 0x200ff000, 0x200ff000, FLG_R | FLG_X | FLG_VALID, 1 as XousPid);

    // Identity map the page that contains kernel arguments
    dprintln!("Making the kernel arguments visible");
    cfg.map_page(translation_table, 0x20100000, 0x20100000, FLG_R | FLG_X | FLG_VALID, 1 as XousPid);

    for addr in (0..cfg.init_size - GUARD_MEMORY_BYTES + cfg.swap_offset).step_by(PAGE_SIZE) {
        cfg.map_page(
            tt,
            addr + krn_struct_start,
            addr + KERNEL_ARGUMENT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
            1 as XousPid,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn map_kernel_to_processes(
    cfg: &mut BootConfig,
    ktext_offset: usize,
    ktext_size: usize,
    ktext_virt_offset: usize,
    kdata_offset: usize,
    kdata_size: usize,
    kdata_virt_offset: usize,
    kernel_exception_sp: usize,
    kernel_irq_sp: usize,
    krn_struct_start: usize,
) {
    let processes = unsafe { core::mem::transmute::<_, &[InitialProcess]>(&*cfg.processes) };

    assert_ne!(kernel_exception_sp, 0, "No exception stack allocated for the kernel!");

    for process in processes[1..].iter() {
        println!("Mapping kernel (PID1) text to process PID{}", process.asid);
        println!("Offset: {:08x}, size: {:08x}", ktext_offset, ktext_size);
        let translation_table = process.ttbr0 as *mut TranslationTableMemory;
        for addr in (0..ktext_size).step_by(PAGE_SIZE) {
            let phys = ktext_offset + addr;
            let virt = ktext_virt_offset + addr;
            println!("MAP ({:08x}): {:08x} -> {:08x}", translation_table as usize, virt, phys);

            cfg.map_page(translation_table, phys, virt, FLG_VALID | FLG_R | FLG_X, 1 as XousPid);
        }
        println!("Mapping kernel (PID1) data to process PID{}", process.asid);
        println!("Offset: {:08x}, size: {:08x}", kdata_offset, kdata_size);
        for addr in (0..kdata_size).step_by(PAGE_SIZE) {
            let phys = kdata_offset + addr;
            let virt = kdata_virt_offset + addr;
            println!("MAP ({:08x}): {:08x} -> {:08x}", translation_table as usize, virt, phys);

            cfg.map_page(translation_table, phys, virt, FLG_VALID | FLG_R | FLG_W, 1 as XousPid);
        }

        println!("Mapping kernel exception stack pages to the process PID{}", process.asid);
        for i in 0..KERNEL_STACK_PAGE_COUNT {
            let virt = EXCEPTION_STACK_TOP - (PAGE_SIZE * KERNEL_STACK_PAGE_COUNT) + (PAGE_SIZE * i);
            let phys = kernel_exception_sp - (PAGE_SIZE * KERNEL_STACK_PAGE_COUNT) + (PAGE_SIZE * (i + 1));
            println!("MAP ({:08x}): {:08x} -> {:08x}", translation_table as usize, virt, phys);
            cfg.map_page(translation_table, phys, virt, FLG_VALID | FLG_R | FLG_W, 1 as XousPid);
        }

        println!("Mapping irq stack page to the process PID{}", process.asid);
        let virt = IRQ_STACK_TOP;
        let phys = kernel_irq_sp;
        println!("MAP ({:08x}): {:08x} -> {:08x}", translation_table as usize, virt, phys);
        cfg.map_page(translation_table, phys, virt, FLG_VALID | FLG_R | FLG_W, 1 as XousPid);

        // TODO: For now, make the UART visible for all the processes.
        //       Later on we may reuse the mapping made by the kernel.
        println!("Mapping UART registers to the process PID{}", process.asid);
        for i in 0..4 {
            let virt = 0xffcf_0000 + (i * PAGE_SIZE);
            let phys = 0xf802_0000 + (i * PAGE_SIZE);
            println!("MAP ({:08x}): {:08x} -> {:08x}", translation_table as usize, virt, phys);
            cfg.map_page(translation_table, phys, virt, FLG_VALID | FLG_R | FLG_W | FLG_X, 1 as XousPid);
        }

        println!("Mapping kernel structures to PID{}", process.asid);
        for addr in (0..cfg.init_size - GUARD_MEMORY_BYTES + cfg.swap_offset).step_by(PAGE_SIZE) {
            cfg.map_page(
                translation_table,
                addr + krn_struct_start,
                addr + KERNEL_ARGUMENT_OFFSET,
                FLG_R | FLG_W | FLG_VALID,
                1 as XousPid,
            );
            cfg.change_owner(1 as XousPid, addr + krn_struct_start);
        }
    }
}

fn print_flags(flags: usize) {
    if flags & FLG_R != 0 {
        dprint!("R");
    }
    if flags & FLG_W != 0 {
        dprint!("W");
    }
    if flags & FLG_X != 0 {
        dprint!("X");
    }
    if flags & FLG_VALID != 0 {
        dprint!("V");
    }
    if flags & FLG_U != 0 {
        dprint!("U");
    }
}
