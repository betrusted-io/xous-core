#[cfg(feature = "swap")]
use crate::swap::*;
use crate::*;

/// Phase 2 bootloader
///
/// Set up all the page tables, allocating new root page tables for SATPs and corresponding
/// sub-pages starting from the base of previously copied process data.
pub fn phase_2(cfg: &mut BootConfig, fs_prehash: &[u8; 64]) {
    let args = cfg.args;

    // This is the offset in RAM where programs are loaded from.
    let mut process_offset = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    println!("\n\nPhase2: Processess start out @ {:08x}", process_offset);

    // Construct an environment block. This will be used by processes
    // to access environment variables and system parameters. This
    // is hardcoded for now, and can be expanded later.
    #[rustfmt::skip]
    let mut env = [
        0x41, 0x70, 0x70, 0x50, // 'AppP' indicating application parameters
        0x08, 0x00, 0x00, 0x00, // Size of AppP tag contents
        0xb2, 0x00, 0x00, 0x00, // Size of entire AppP block including all tags
        0x02, 0x00, 0x00, 0x00, // Number of tags present
        0x45, 0x6e, 0x76, 0x42, // 'EnvB' indicating an environment block
        0x9a, 0x00, 0x00, 0x00, // Number of bytes that follows for the environment block
        0x01, 0x00, // Number of environment variables
        0x14, 0x00, // Length of name of first variable
        // Name of first variable 'ROOT_FILESYSTEM_HASH':
        0x52, 0x4f, 0x4f, 0x54, 0x5f, 0x46, 0x49, 0x4c, 0x45, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4d,
        0x5f, 0x48, 0x41, 0x53, 0x48,
        // Length of the contents of the first variable
        0x80, 0x00,
        // Root filesystem hash contents begin here:
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
    ];
    // Convert the fs_prehash into a hex ascii string, suitable for environment variables
    // The initial loader environment is hard-coded, so we use a hard-coded offset for the string
    // destination. It was a deliberate decision to not include a generic environment variable
    // handler in the loader because we want to keep the loader compact and with a small attack
    // surface; a generic environment handling routine would add significant size without much
    // benefit.
    //
    // Note that the main purpose for environment variables is for test & debug tooling, where
    // single programs are run in hosted or emulated environments with arguments passed for fast
    // debugging. The sole purpose for the environment variable in the loader's context is to
    // pass this computed hash onto the userspace environments.
    const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
    for (i, &byte) in fs_prehash.iter().enumerate() {
        env[env.len() - 128 + i * 2] = HEX_DIGITS[(byte >> 4) as usize];
        env[env.len() - 128 + i * 2 + 1] = HEX_DIGITS[(byte & 0xF) as usize];
    }

    // Go through all Init processes and the kernel, setting up their
    // page tables and mapping memory to them.
    let mut pid = 2;
    #[cfg(feature = "atsama5d27")]
    let mut ktext_offset = 0;
    #[cfg(feature = "atsama5d27")]
    let mut ktext_virt_offset = 0;
    #[cfg(feature = "atsama5d27")]
    let mut kdata_offset = 0;
    #[cfg(feature = "atsama5d27")]
    let mut kdata_virt_offset = 0;
    #[cfg(feature = "atsama5d27")]
    let mut ktext_size = 0;
    #[cfg(feature = "atsama5d27")]
    let mut kdata_size = 0;
    #[cfg(feature = "atsama5d27")]
    let mut kernel_exception_sp = 0;
    #[cfg(feature = "atsama5d27")]
    let mut kernel_irq_sp = 0;

    for tag in args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            let inie = MiniElf::new(&tag);
            println!("\n\nCopying IniE program into memory");
            let allocated = inie.load(cfg, process_offset, pid, &env, IniType::IniE);
            println!("IniE Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniF") {
            let inif = MiniElf::new(&tag);
            println!("\n\nMapping IniF program into memory");
            let allocated = inif.load(cfg, process_offset, pid, &env, IniType::IniF);
            println!("IniF Allocated {:x}", allocated);
            process_offset -= allocated;
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"IniS") {
            #[cfg(feature = "swap")]
            {
                let inis = MiniElf::new(&tag);
                println!("\n\nMapping IniS program into memory");
                let allocated = inis.load(cfg, process_offset, pid, &env, IniType::IniS);
                println!("IniS Allocated {:x}", allocated);
                process_offset -= allocated;
                pid += 1;
            }
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            let xkrn = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };
            println!("\n\nCopying kernel into memory");
            let load_size_rounded = ((xkrn.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
                + (((xkrn.data_size + xkrn.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
            #[cfg(not(feature = "atsama5d27"))]
            {
                xkrn.load(cfg, process_offset - load_size_rounded, 1);
            }
            #[cfg(feature = "atsama5d27")]
            {
                (ktext_offset, kdata_offset, kernel_exception_sp, kernel_irq_sp) =
                    xkrn.load(cfg, process_offset - load_size_rounded, 1);
                (ktext_size, kdata_size, ktext_virt_offset, kdata_virt_offset) = (
                    (xkrn.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1),
                    (((xkrn.data_size + xkrn.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)),
                    xkrn.text_offset as usize,
                    xkrn.data_offset as usize,
                );
            }
            process_offset -= load_size_rounded;
        }
    }

    println!("Done loading.");

    assert!(cfg.init_size & 0xFFF == 0); // check that we didn't mess this assumption up somewhere in the process
    let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size + cfg.swap_offset;
    #[cfg(feature = "atsama5d27")]
    let krn_l1_pt_addr = cfg.processes[0].ttbr0;
    #[cfg(not(feature = "atsama5d27"))]
    let krn_l1_pt_addr = cfg.processes[0].satp << 12;

    println!("krn_l1_pt_addr: {:08x}", krn_l1_pt_addr);

    #[cfg(not(feature = "atsama5d27"))]
    {
        assert!(krn_struct_start & (PAGE_SIZE - 1) == 0);
        let krn_pg1023_ptr = unsafe { (krn_l1_pt_addr as *const usize).add(1023).read() };

        // Map boot-generated kernel structures into the kernel
        let satp = unsafe { &mut *(krn_l1_pt_addr as *mut PageTable) };
        let kernel_arg_extents = cfg.init_size - (GUARD_MEMORY_BYTES + cfg.swap_offset);
        assert!(kernel_arg_extents <= 0xA000, "Kernel init structures exceeded allocated region");
        for addr in (0..kernel_arg_extents).step_by(PAGE_SIZE as usize) {
            cfg.map_page(
                satp,
                addr + krn_struct_start,
                addr + KERNEL_ARGUMENT_OFFSET,
                FLG_R | FLG_W | FLG_VALID,
                1 as XousPid,
            );
        }
        #[cfg(feature = "swap")]
        if SDBG && VDBG {
            // dumps the page with kernel struct data, so we can correlate offsets to data.
            for i in (0..4096).step_by(32) {
                println!("{:08x}: {:02x?}", krn_struct_start + i, unsafe {
                    core::slice::from_raw_parts((krn_struct_start + i) as *const u8, 32)
                });
            }
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
    }
    #[cfg(feature = "atsama5d27")]
    {
        // Map boot-generated kernel structures into the kernel
        crate::platform::atsama5d27::boot::map_structs_to_kernel(cfg, krn_l1_pt_addr, krn_struct_start);
        crate::platform::atsama5d27::boot::map_kernel_to_processes(
            cfg,
            ktext_offset,
            ktext_size,
            ktext_virt_offset,
            kdata_offset,
            kdata_size,
            kdata_virt_offset,
            kernel_exception_sp,
            kernel_irq_sp,
            krn_struct_start,
        );
    }
    #[cfg(feature = "swap")]
    {
        // map the swap page table into PID space 2
        let tt_address = cfg.processes[SWAPPER_PID as usize - 1].satp << 12;
        let root = unsafe { &mut *(tt_address as *mut PageTable) };
        let mut swap_pt_vaddr_offset = 0;
        // map page table roots
        for p in 0..cfg.processes.len() {
            // loop is "decomposed" because iterating over processes causes a borrow conflic
            let swap_root = cfg.swap_root[p];
            println!(
                "Mapping root swap PT to PID 2 @paddr {:x} -> vaddr {:x}",
                swap_root,
                SWAP_PT_VADDR + swap_pt_vaddr_offset
            );
            cfg.map_page(
                root,
                swap_root,
                SWAP_PT_VADDR + swap_pt_vaddr_offset,
                FLG_R | FLG_W | FLG_U | FLG_VALID,
                SWAPPER_PID,
            );
            swap_pt_vaddr_offset += PAGE_SIZE;
        }
        // now chase down any entries in the roots, and map valid pages
        for p in 0..cfg.processes.len() {
            let root_pt = unsafe { &mut *(cfg.swap_root[p] as *mut PageTable) };
            for entry in root_pt.entries.iter_mut() {
                if *entry & FLG_VALID != 0 {
                    let paddr = (*entry & !0x3FF) << 2;
                    let vaddr = SWAP_PT_VADDR + swap_pt_vaddr_offset;
                    cfg.map_page(root, paddr, vaddr, FLG_R | FLG_W | FLG_U | FLG_VALID, SWAPPER_PID);
                    // patch the entry to point at the virtual address
                    *entry &= 0x3FF;
                    *entry |= (vaddr & !0xFFF) >> 2;
                    println!("Remapping L2 PT @paddr {:x} -> vaddr {:x}", paddr, vaddr);
                    swap_pt_vaddr_offset += PAGE_SIZE;
                }
            }
        }
        // map the arguments into PID 2
        let swap_spec_ptr = cfg.alloc();
        cfg.map_page(
            root,
            swap_spec_ptr as usize,
            SWAP_CFG_VADDR,
            FLG_R | FLG_W | FLG_U | FLG_VALID,
            SWAPPER_PID,
        );
        // this is safe because:
        //   - swap_spec_ptr is aligned (it's page-aligned even)
        //   - alloc() zeroes the contents
        //   - SwapSpec is a Repr(C), and every element of the struct is valid with a 0's initialization.
        let swap_spec = unsafe { (swap_spec_ptr as *mut SwapSpec).as_mut().unwrap() };
        if let Some(desc) = cfg.swap {
            swap_spec.key.copy_from_slice(&cfg.swap_hal.as_ref().unwrap().get_swap_key());
            swap_spec.pid_count = cfg.init_process_count as u32 + 1;
            swap_spec.rpt_len_bytes = cfg.runtime_page_tracker.len() as u32;
            swap_spec.swap_base = desc.ram_offset;
            swap_spec.swap_len = desc.ram_size;
            (swap_spec.mac_base, swap_spec.mac_len) = cfg.swap_hal.as_ref().unwrap().mac_base_bounds();
            swap_spec.sram_start = cfg.sram_start as u32;
            swap_spec.sram_size = cfg.sram_size as u32;
        }

        // copy the RPT into PID 2
        {
            // safety: This is safe because we know that:
            //   - RPT is fully initialized at this point
            //   - rpt_alias is scoped in a block so it can't be abused later by accident
            //   - We are only going to read data from the alias pointer
            //   - There is no borrow conflict in the upcoming for loop
            // This is done to avoid adding a RefCell around the runtime page tracker to work around
            // interior mutability issues in a very performance-sensitive region, and in a manner that
            // only ever affects the `swap` configuration (i.e., if we did this as a RefCell we'd have
            // to pay the performance penalty for all configurations, and/or we'd have to add `cfg`
            // directives in lots of places). Mea culpa, this is not how you're supposed to write Rust.
            let terrible_rpt_alias = unsafe {
                core::slice::from_raw_parts(cfg.runtime_page_tracker.as_ptr(), cfg.runtime_page_tracker.len())
            };
            for (i, rpt_page) in terrible_rpt_alias.chunks(4096).enumerate() {
                let swap_rpt = cfg.alloc() as usize;
                cfg.map_page(
                    root,
                    swap_rpt,
                    SWAP_RPT_VADDR + i * 4096,
                    FLG_R | FLG_W | FLG_U | FLG_VALID,
                    SWAPPER_PID,
                );
                // safety: this is safe because the allocator aligns it and initializes it, and all underlying
                // data has defined behavior as initialized
                let dest = unsafe { core::slice::from_raw_parts_mut(swap_rpt as *mut u8, 4096) };
                dest[..rpt_page.len()].copy_from_slice(rpt_page);
            }
        }

        // map any hardware-specific pages into the userspace swapper
        crate::platform::userspace_maps(cfg);
    }

    if VVDBG || SDBG {
        println!("PID1 pagetables:");
        #[cfg(feature = "atsama5d27")]
        debug::print_pagetable(cfg.processes[0].ttbr0);
        #[cfg(not(feature = "atsama5d27"))]
        debug::print_pagetable(cfg.processes[0].satp);
        println!();
        println!();
        for (_pid, process) in cfg.processes[1..].iter().enumerate() {
            println!("PID{} pagetables:", _pid + 2);
            #[cfg(feature = "atsama5d27")]
            debug::print_pagetable(process.ttbr0);
            #[cfg(not(feature = "atsama5d27"))]
            debug::print_pagetable(process.satp);
            println!();
            println!();
        }
    }
    println!("Runtime Page Tracker: {} bytes", cfg.runtime_page_tracker.len());
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
    #[cfg(not(feature = "atsama5d27"))]
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid) {
        assert!(pid != 0);
        println!("Mapping PID {} into offset {:08x}", pid, load_offset);
        let pid_idx = (pid - 1) as usize;
        let is_kernel = pid == 1;
        let flag_defaults = FLG_R | FLG_W | FLG_VALID | if is_kernel { 0 } else { FLG_U };
        let stack_addr = if is_kernel { KERNEL_STACK_TOP } else { USER_STACK_TOP } - 16;
        if is_kernel {
            assert!(self.text_offset as usize == KERNEL_LOAD_OFFSET);
            assert!(((self.text_offset + self.text_size) as usize) < EXCEPTION_STACK_TOP);
            assert!(
                ((self.data_offset + self.data_size + self.bss_size) as usize) < EXCEPTION_STACK_TOP - 16
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
        if SDBG {
            println!("Kernel root PT address: {:x}", satp_address);
        }
        allocator.map_page(
            satp,
            satp_address,
            PAGE_TABLE_ROOT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
            pid as XousPid,
        );

        // Allocate context for this process
        let thread_address = allocator.alloc() as usize;
        allocator.map_page(satp, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID, pid as XousPid);

        // Allocate stack pages.
        for i in 0..if is_kernel { KERNEL_STACK_PAGE_COUNT } else { STACK_PAGE_COUNT } {
            if i == 0 {
                // Pre-allocate the first stack offset, since it
                // will definitely be used
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults,
                    pid as XousPid,
                );
            } else {
                // Reserve every page other than the 1st stack page
                allocator.map_page(
                    satp,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    flag_defaults & !FLG_VALID,
                    pid as XousPid,
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
                    pid as XousPid,
                );
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
        let rounded_data_bss = ((self.data_size + self.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // let load_size_rounded = (self.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for offset in (0..self.text_size as usize).step_by(PAGE_SIZE) {
            if VDBG {
                println!(
                    "   TEXT: Mapping {:08x} -> {:08x}",
                    load_offset + offset + rounded_data_bss,
                    self.text_offset as usize + offset
                );
            }
            allocator.map_page(
                satp,
                load_offset + offset + rounded_data_bss,
                self.text_offset as usize + offset,
                flag_defaults | FLG_X | FLG_VALID,
                pid as XousPid,
            );
        }

        // Map the process data section into RAM.
        for offset in (0..(self.data_size + self.bss_size) as usize).step_by(PAGE_SIZE as usize) {
            // let page_addr = allocator.alloc();
            if VDBG {
                println!(
                    "   DATA: Mapping {:08x} -> {:08x}",
                    load_offset + offset,
                    self.data_offset as usize + offset
                );
            }
            allocator.map_page(
                satp,
                load_offset + offset,
                self.data_offset as usize + offset,
                flag_defaults,
                pid as XousPid,
            );
        }

        // Allocate pages for .bss, if necessary

        // Our "earlyprintk" equivalent
        if cfg!(feature = "earlyprintk") && is_kernel {
            allocator.map_page(satp, 0xF000_2000, 0xffcf_0000, FLG_R | FLG_W | FLG_VALID, pid as XousPid);
        }

        let process = &mut allocator.processes[pid_idx];
        process.entrypoint = self.entrypoint as usize;
        process.sp = stack_addr;
        process.env = 0;
        process.satp = 0x8000_0000 | ((pid as usize) << 22) | (satp_address >> 12);
    }
}
