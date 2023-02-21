use crate::*;

/// Phase 2 bootloader
///
/// Set up all the page tables, allocating new root page tables for SATPs and corresponding
/// sub-pages starting from the base of previously copied process data.
pub fn phase_2(cfg: &mut BootConfig) {
    let args = cfg.args;

    // This is the offset in RAM where programs are loaded from.
    let mut process_offset = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    println!("\n\nPhase2: Processess start out @ {:08x}", process_offset);

    // Go through all Init processes and the kernel, setting up their
    // page tables and mapping memory to them.
    let mut pid = 2;
    for tag in args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            let inie = MiniElf::new(&tag);
            println!("\n\nCopying IniE program into memory");
            let allocated = inie.load(cfg, process_offset, pid, false);
            println!("IniE Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            let inif = MiniElf::new(&tag);
            println!("\n\nMapping IniF program into memory");
            let allocated = inif.load(cfg, process_offset, pid, true);
            println!("IniF Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            println!("\n\nCopying kernel into memory");
            let xkrn = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };
            let load_size_rounded = ((xkrn.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
                + (((xkrn.data_size + xkrn.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
            xkrn.load(cfg, process_offset - load_size_rounded, 1);
            process_offset -= load_size_rounded;
        }
    }

    println!("Done loading.");
    let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    let krn_l1_pt_addr = cfg.processes[0].satp << 12;
    assert!(krn_struct_start & (PAGE_SIZE - 1) == 0);
    let krn_pg1023_ptr = unsafe { (krn_l1_pt_addr as *const usize).add(1023).read() };

    // Map boot-generated kernel structures into the kernel
    let satp = unsafe { &mut *(krn_l1_pt_addr as *mut PageTable) };
    for addr in (0..cfg.init_size-GUARD_MEMORY_BYTES).step_by(PAGE_SIZE as usize) {
        cfg.map_page(
            satp,
            addr + krn_struct_start,
            addr + KERNEL_ARGUMENT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
        );
        cfg.change_owner(1 as XousPid, (addr + krn_struct_start) as usize);
    }

    // Copy the kernel's "MMU Page 1023" into every process.
    // This ensures a context switch into the kernel can
    // always be made, and that the `stvec` is always valid.
    // Since it's a megapage, all we need to do is write
    // the one address to get all 4MB mapped.
    println!("Mapping MMU page 1023 to all processes");
    for process in cfg.processes[1..].iter() {
        let l1_pt_addr = process.satp << 12;
        unsafe { (l1_pt_addr as *mut usize).add(1023).write(krn_pg1023_ptr) };
    }

    if VVDBG {
        println!("PID1 pagetables:");
        debug::print_pagetable(cfg.processes[0].satp);
        println!();
        println!();
        for (_pid, process) in cfg.processes[1..].iter().enumerate() {
            println!("PID{} pagetables:", _pid + 2);
            debug::print_pagetable(process.satp);
            println!();
            println!();
        }
    }
    println!(
        "Runtime Page Tracker: {} bytes",
        cfg.runtime_page_tracker.len()
    );
    // mark pages used by suspend/resume according to their needs
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 1] = 1; // claim the loader stack -- do not allow tampering, as it contains backup kernel args
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 2] = 1; // 8k in total (to allow for digital signatures to be computed)
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 3] = 0; // allow clean suspend page to be mapped in Xous
}

/// This describes the kernel as well as initially-loaded processes
#[repr(C)]
pub struct ProgramDescription {
    /// Physical source address of this program in RAM (i.e. SPI flash).
    /// The image is assumed to contain a text section followed immediately
    /// by a data section.
    pub load_offset: u32,

    /// Start of the virtual address where the .text section will go.
    /// This section will be marked non-writable, executable.
    pub text_offset: u32,

    /// How many bytes of data to load from the source to the target
    pub text_size: u32,

    /// Start of the virtual address of .data and .bss section in RAM.
    /// This will simply allocate this memory and mark it "read-write"
    /// without actually copying any data.
    pub data_offset: u32,

    /// Size of the .data section, in bytes..  This many bytes will
    /// be allocated for the data section.
    pub data_size: u32,

    /// Size of the .bss section, in bytes.
    pub bss_size: u32,

    /// Virtual address entry point.
    pub entrypoint: u32,
}

impl ProgramDescription {
    /// Map this ProgramDescription into RAM.
    /// The program may already have been relocated, and so may be
    /// either on SPI flash or in RAM.  The `load_offset` argument
    /// that is passed in should be used instead of `self.load_offset`
    /// for this reason.
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid) {
        assert!(pid != 0);
        println!("Mapping PID {} into offset {:08x}", pid, load_offset);
        let pid_idx = (pid - 1) as usize;
        let is_kernel = pid == 1;
        let flag_defaults = FLG_R | FLG_W | FLG_VALID | if is_kernel { 0 } else { FLG_U };
        let stack_addr = USER_STACK_TOP - 16;
        if is_kernel {
            assert!(self.text_offset as usize == KERNEL_LOAD_OFFSET);
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

        // SATP must be nonzero
        if allocator.processes[pid_idx].satp != 0 {
            panic!("tried to re-use a process id");
        }

        // Allocate a page to handle the top-level memory translation
        let satp_address = allocator.alloc() as usize;
        allocator.change_owner(pid as XousPid, satp_address);

        // Turn the satp address into a pointer
        let satp = unsafe { &mut *(satp_address as *mut PageTable) };
        allocator.map_page(satp, satp_address, PAGE_TABLE_ROOT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, satp_address as usize);

        // Allocate context for this process
        let thread_address = allocator.alloc() as usize;
        allocator.map_page(satp, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, thread_address as usize);

        // Allocate stack pages.
        for i in 0..if is_kernel {
            KERNEL_STACK_PAGE_COUNT
        } else {
            STACK_PAGE_COUNT
        } {
            if i == 0 {
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            } else {
                // Reserve every page other than the 1st stack page
                allocator.map_page(
                    satp,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults & !FLG_VALID,
                );
            }

            // If it's the kernel, also allocate an exception page
            if is_kernel {
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (EXCEPTION_STACK_TOP - 16 - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            }
        }

        assert!((self.text_offset as usize & (PAGE_SIZE - 1)) == 0);
        assert!((self.data_offset as usize & (PAGE_SIZE - 1)) == 0);
        if allocator.no_copy {
            assert!((self.load_offset as usize & (PAGE_SIZE - 1)) == 0);
        }

        // Map the process text section into RAM.
        // Either this is on SPI flash at an aligned address, or it
        // has been copied into RAM already.  This is why we ignore `self.load_offset`
        // and use the `load_offset` parameter instead.
        let rounded_data_bss =
            ((self.data_size + self.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // let load_size_rounded = (self.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for offset in (0..self.text_size as usize).step_by(PAGE_SIZE) {
            if VDBG {println!(
                "   TEXT: Mapping {:08x} -> {:08x}",
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset
            );}
            allocator.map_page(
                satp,
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset,
                flag_defaults | FLG_X | FLG_VALID,
            );
            allocator.change_owner(pid as XousPid, load_offset + offset + rounded_data_bss);
        }

        // Map the process data section into RAM.
        for offset in (0..(self.data_size + self.bss_size) as usize).step_by(PAGE_SIZE as usize) {
            // let page_addr = allocator.alloc();
            if VDBG {println!(
                "   DATA: Mapping {:08x} -> {:08x}",
                load_offset + offset,
                self.data_offset as usize + offset
            );}
            allocator.map_page(
                satp,
                load_offset + offset,
                self.data_offset as usize + offset,
                flag_defaults,
            );
            allocator.change_owner(pid as XousPid, load_offset as usize + offset);
        }

        // Allocate pages for .bss, if necessary

        // Our "earlyprintk" equivalent
        if cfg!(feature = "earlyprintk") && is_kernel {
            allocator.map_page(satp, 0xF000_2000, 0xffcf_0000, FLG_R | FLG_W | FLG_VALID);
            allocator.change_owner(pid as XousPid, 0xF000_2000);
        }

        let mut process = &mut allocator.processes[pid_idx];
        process.entrypoint = self.entrypoint as usize;
        process.sp = stack_addr;
        process.satp = 0x8000_0000 | ((pid as usize) << 22) | (satp_address >> 12);
    }
}
