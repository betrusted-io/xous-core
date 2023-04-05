// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::consts::{CONTEXT_OFFSET, EXCEPTION_STACK_TOP, FLG_R, FLG_U, FLG_VALID, FLG_W, FLG_X, KERNEL_LOAD_OFFSET, PAGE_TABLE_ROOT_OFFSET, USER_AREA_END, USER_STACK_TOP, KERNEL_STACK_PAGE_COUNT, KERNEL_STACK_TOP, IRQ_STACK_PAGE_COUNT, IRQ_STACK_TOP};
use crate::{
    println, BootConfig, ProgramDescription,
    XousPid, PAGE_SIZE, STACK_PAGE_COUNT, VDBG,
};
use armv7::structures::paging::TranslationTableMemory;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
/// **Note**: this struct must be in sync with the loader version.
pub struct InitialProcess {
    /// Level-1 translation table base address of the process
    pub ttbr0: usize,

    /// Address Space ID (PID) of the process.
    pub asid: u8,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,
}

impl ProgramDescription {
    /// Map this ProgramDescription into RAM.
    /// The program may already have been relocated, and so may be
    /// either on SPI flash or in RAM.  The `load_offset` argument
    /// that is passed in should be used instead of `self.load_offset`
    /// for this reason.
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid)
                -> (usize, usize, usize, usize) {
        assert_ne!(pid, 0, "PID must not be 0");

        println!("Mapping PID {} into offset {:08x}", pid, load_offset);
        let pid_idx = (pid - 1) as usize;
        let is_kernel = pid == 1;
        let flag_defaults = FLG_R | FLG_W | FLG_VALID | if is_kernel { 0 } else { FLG_U };
        let stack_addr = if is_kernel { KERNEL_STACK_TOP } else { USER_STACK_TOP } - 16;
        if is_kernel {
            println!(
                "self.text_offset: {:08x}, KERNEL_LOAD_OFFSET: {:08x}",
                self.text_offset, KERNEL_LOAD_OFFSET
            );
            assert_eq!(self.text_offset as usize, KERNEL_LOAD_OFFSET);
            assert!(((self.text_offset + self.text_size) as usize) < EXCEPTION_STACK_TOP);
            assert!(
                ((self.data_offset + self.data_size + self.bss_size) as usize)
                    < EXCEPTION_STACK_TOP - 16
            );
            assert!(self.data_offset as usize >= KERNEL_LOAD_OFFSET);
        } else {
            assert!(((self.text_offset + self.text_size) as usize) < USER_AREA_END);
            assert!(((self.data_offset + self.data_size) as usize) < USER_AREA_END);
        }

        // Translation table address must be zero
        if allocator.processes[pid_idx].ttbr0 != 0 {
            panic!("tried to re-use a process id {}", pid);
        }

        // Allocate physical pages for L1 translation table
        let tt_address = allocator.alloc_l1_page_table(pid) as usize;
        if VDBG {
            println!("Setting {:08x} as translation table address for PID {}", tt_address, pid);
        }

        allocator.processes[pid_idx].ttbr0 = tt_address;

        let translation_table = tt_address as *mut TranslationTableMemory;
        // Map all four pages of the translation table to the kernel address space
        for offset in 0..4 {
            let offset = offset * PAGE_SIZE;
            println!("Map L1 pages: {:08x} -> {:08x}", tt_address + offset, PAGE_TABLE_ROOT_OFFSET + offset);
            allocator.map_page(
                translation_table,
                tt_address + offset,
                PAGE_TABLE_ROOT_OFFSET + offset,
                FLG_R | FLG_W | FLG_VALID,
            );
            allocator.change_owner(pid as XousPid, tt_address);
        }

        // Allocate context for this process
        let thread_address = allocator.alloc() as usize;
        if VDBG {
            println!("PID {} thread: 0x{:08x}", pid, thread_address);
        }

        allocator.map_page(
            translation_table,
            thread_address,
            CONTEXT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
        );
        allocator.change_owner(pid as XousPid, thread_address);

        // Allocate stack pages.
        let total_stack_pages = if is_kernel {
            KERNEL_STACK_PAGE_COUNT
        } else {
            STACK_PAGE_COUNT
        };

        if VDBG {
            println!("Mapping {} stack pages for PID {}", total_stack_pages, pid);
        }

        let mut exception_sp = 0;
        for i in 0..total_stack_pages {
            if i == 0 {
                let sp_page = allocator.alloc() as usize;
                println!("Allocated stack page: {:08x}", sp_page);

                allocator.map_page(
                    translation_table,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            } else {
                // Reserve every page other than the 1st stack page
                allocator.map_page(
                    translation_table,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults & !FLG_VALID,
                );
            }

            // If it's the kernel, also allocate an exception page
            if is_kernel {
                let sp_page = allocator.alloc() as usize;
                // Remember only the first (top) exception stack page
                if exception_sp == 0 {
                    exception_sp = sp_page;
                }
                println!("Allocated exception stack page: {:08x}", sp_page);
                allocator.map_page(
                    translation_table,
                    sp_page,
                    (EXCEPTION_STACK_TOP - 16 - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            }
        }

        // Allocate IRQ stack pages
        let mut irq_sp = 0;
        for i in 0..IRQ_STACK_PAGE_COUNT {
            // If it's the kernel, also allocate an exception page
            if is_kernel {
                let sp_page = allocator.alloc() as usize;
                // Remember only the first (top) exception stack page
                if irq_sp == 0 {
                    irq_sp = sp_page;
                }
                println!("Allocated IRQ stack page: {:08x}", sp_page);
                allocator.map_page(
                    translation_table,
                    sp_page,
                    (IRQ_STACK_TOP - 16 - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            }
        }

        assert_eq!((self.text_offset as usize & (PAGE_SIZE - 1)), 0);
        assert_eq!((self.data_offset as usize & (PAGE_SIZE - 1)), 0);
        if allocator.no_copy {
            assert_eq!((self.load_offset as usize & (PAGE_SIZE - 1)), 0);
        }

        // Map the process text section into RAM.
        // Either this is on SPI flash at an aligned address, or it
        // has been copied into RAM already.  This is why we ignore `self.load_offset`
        // and use the `load_offset` parameter instead.
        let rounded_data_bss =
            ((self.data_size + self.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // let load_size_rounded = (self.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let text_phys_offset = load_offset + rounded_data_bss;
        for offset in (0..self.text_size as usize).step_by(PAGE_SIZE) {
            if VDBG {
                println!(
                    "   TEXT: Mapping {:08x} -> {:08x}",
                    load_offset + offset + rounded_data_bss,
                    self.text_offset as usize + offset
                );
            }
            allocator.map_page(
                translation_table,
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset,
                flag_defaults | FLG_X | FLG_VALID,
            );
            allocator.change_owner(pid as XousPid, load_offset + offset + rounded_data_bss);
        }

        // Map the process data section into RAM.
        let data_phys_offset = load_offset;
        for offset in (0..(self.data_size + self.bss_size) as usize).step_by(PAGE_SIZE) {
            // let page_addr = allocator.alloc();
            if VDBG {
                println!(
                    "   DATA: Mapping {:08x} -> {:08x}",
                    load_offset + offset,
                    self.data_offset as usize + offset
                );
            }
            allocator.map_page(
                translation_table,
                load_offset + offset,
                self.data_offset as usize + offset,
                flag_defaults,
            );
            allocator.change_owner(pid as XousPid, load_offset + offset);
        }

        allocator.processes[pid_idx].entrypoint = self.entrypoint as usize;
        allocator.processes[pid_idx].sp = stack_addr;
        allocator.processes[pid_idx].asid = pid;

        (text_phys_offset, data_phys_offset, exception_sp, irq_sp)
    }
}
