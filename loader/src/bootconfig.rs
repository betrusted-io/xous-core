#[cfg(not(feature = "atsama5d27"))]
use core::mem;
#[cfg(not(feature = "atsama5d27"))]
use core::num::NonZeroUsize;

#[cfg(feature = "swap")]
use crate::swap::SwapDescriptor;
use crate::*;

/// In-memory copy of the configuration page. Stage 1 sets up the gross structure,
/// and Stage 2 fills in the details.
pub struct BootConfig {
    /// `true` if the kernel and Init programs run XIP
    pub no_copy: bool,

    /// Base load address.  Defaults to the start of the args block
    pub base_addr: *const usize,

    /// `true` if we should enable the `SUM` bit, allowing the
    /// kernel to access user memory.
    pub debug: bool,

    /// Where the tagged args list starts in RAM.
    pub args: KernelArguments,

    /// Additional memory regions in this system
    pub regions: &'static [MemoryRegionExtra],

    /// The origin of usable memory.  This is where heap lives.
    pub sram_start: *mut usize,

    /// The size (in bytes) of the heap.
    pub sram_size: usize,

    /// A running total of the number of bytes consumed during
    /// initialization.  This value, divided by PAGE_SIZE,
    /// indicates the number of pages at the end of RAM that
    /// will need to be owned by the kernel.
    pub init_size: usize,

    /// Additional pages that are consumed during init.
    /// This includes pages that are allocated to other
    /// processes.
    pub extra_pages: usize,

    /// This structure keeps track of which pages are owned
    /// and which are free. A PID of `0` indicates it's free.
    pub runtime_page_tracker: &'static mut [XousPid],

    /// A list of processes that were set up.  The first element
    /// is the kernel, and any subsequent elements are init processes.
    pub processes: &'static mut [InitialProcess],

    /// The number of 'Init' tags discovered
    pub init_process_count: usize,

    /// Amount that init_size is offset by swap. We have to track this
    /// separately because init_size is used during allocations to track
    /// cfg_top(), but then re-used during page mapping with the assumption
    /// that it also points to exclusive kernel memory. swap_offset allows
    /// us to subtract out the memory we allocated and gave to swap in that
    /// phase of boot. When swap is not enabled, it is set to 0.
    pub swap_offset: usize,

    /// Swap HAL
    #[cfg(feature = "swap")]
    pub swap_hal: Option<SwapHal>,

    /// Swap descriptor
    #[cfg(feature = "swap")]
    pub swap: Option<&'static SwapDescriptor>,

    /// Offset of the current free page in swap
    #[cfg(feature = "swap")]
    pub swap_free_page: usize,

    /// root swap page table of the process
    #[cfg(feature = "swap")]
    pub swap_root: &'static mut [usize],
}

impl Default for BootConfig {
    fn default() -> BootConfig {
        BootConfig {
            no_copy: false,
            debug: false,
            base_addr: core::ptr::null::<usize>(),
            regions: Default::default(),
            sram_start: core::ptr::null_mut::<usize>(),
            sram_size: 0,
            args: KernelArguments::new(core::ptr::null::<usize>()),
            init_size: 0,
            extra_pages: 0,
            runtime_page_tracker: Default::default(),
            init_process_count: 0,
            processes: Default::default(),
            swap_offset: 0,
            #[cfg(feature = "swap")]
            swap_hal: None,
            #[cfg(feature = "swap")]
            swap: None,
            #[cfg(feature = "swap")]
            swap_free_page: 0,
            #[cfg(feature = "swap")]
            swap_root: Default::default(),
        }
    }
}

#[cfg(not(feature = "atsama5d27"))]
impl BootConfig {
    /// Used by Phase 1 to keep track of where we are in terms of physical pages of memory allocated
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

    /// The rest of the functions are used by phase 2 to help set up page tables.
    ///
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
        self.runtime_page_tracker[(self.sram_size - (extra_bytes + self.init_size)) / PAGE_SIZE] = 1;

        // Return the address
        pg as *mut usize
    }

    pub fn change_owner(&mut self, pid: XousPid, addr: usize) {
        // First, check to see if the region is in RAM,
        if addr >= self.sram_start as usize && addr < self.sram_start as usize + self.sram_size {
            // Mark this page as in-use by the kernel
            self.runtime_page_tracker[(addr - self.sram_start as usize) / PAGE_SIZE] = pid;
            return;
        }
        // The region isn't in RAM, so check the other memory regions.
        let mut rpt_offset = self.sram_size / PAGE_SIZE;

        for region in self.regions.iter() {
            let rstart = region.start as usize;
            let rlen = region.length as usize;
            if addr >= rstart && addr < rstart + rlen {
                self.runtime_page_tracker[rpt_offset + (addr - rstart) / PAGE_SIZE] = pid;
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
    pub fn map_page(&mut self, root: &mut PageTable, phys: usize, virt: usize, flags: usize, owner: XousPid) {
        if VDBG {
            println!("    map pa {:x} -> va {:x} (satp {:x})", phys, virt, root as *mut PageTable as u32);
        }
        assert!(!(phys == 0 && flags & FLG_VALID != 0), "cannot map zero page");
        if flags & FLG_VALID != 0 {
            self.change_owner(owner, phys);
        }
        match WORD_SIZE {
            4 => self.map_page_32(root, phys, virt, flags, owner),
            8 => panic!("map_page doesn't work on 64-bit devices"),
            _ => panic!("unrecognized word size: {}", WORD_SIZE),
        }
    }

    #[cfg(feature = "swap")]
    pub fn map_swap(&mut self, swap_phys: usize, virt: usize, owner: XousPid) {
        if SDBG {
            println!("    swap pa {:x} -> va {:x}", swap_phys, virt);
        }
        let ppn1 = (swap_phys >> 22) & ((1 << 12) - 1);
        let ppn0 = (swap_phys >> 12) & ((1 << 10) - 1);

        let vpn1 = (virt >> 22) & ((1 << 10) - 1);
        let vpn0 = (virt >> 12) & ((1 << 10) - 1);
        assert!(owner != 0);
        let l1_pt = unsafe {
            core::slice::from_raw_parts_mut(
                self.swap_root[owner as usize - 1] as *mut usize,
                mem::size_of::<PageTable>() / mem::size_of::<usize>(),
            )
        };

        // Allocate a new level 1 pagetable entry if one doesn't exist.
        if l1_pt[vpn1] & FLG_VALID == 0 {
            let na = self.alloc() as usize;
            if SDBG {
                println!(
                    "Swap Level 1 page table is invalid ({:08x}) @ {:08x} -- allocating a new one @ {:08x}",
                    unsafe { l1_pt.as_ptr().add(vpn1) } as usize,
                    l1_pt[vpn1],
                    na
                );
            }
            // Mark this entry as a leaf node (WRX as 0), and indicate
            // it is a valid page by setting "V".
            l1_pt[vpn1] = ((na >> 12) << 10) | FLG_VALID;
        }

        let l0_pt_idx = unsafe { &mut (*(((l1_pt[vpn1] << 2) & !((1 << 12) - 1)) as *mut PageTable)) };
        let l0_pt = &mut l0_pt_idx.entries;

        // Ensure the entry hasn't already been mapped to a different address.
        if l0_pt[vpn0] & 1 != 0 && (l0_pt[vpn0] & 0xffff_fc00) != ((ppn1 << 20) | (ppn0 << 10)) {
            panic!(
                "Swap page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
                swap_phys,
                (l0_pt[vpn0] >> 10) << 12,
                virt
            );
        }
        let previous_flags = l0_pt[vpn0] & 0xf;
        l0_pt[vpn0] = (ppn1 << 20) | (ppn0 << 10) | previous_flags | FLG_VALID;
    }

    pub fn map_page_32(
        &mut self,
        root: &mut PageTable,
        phys: usize,
        virt: usize,
        flags: usize,
        owner: XousPid,
    ) {
        let ppn1 = (phys >> 22) & ((1 << 12) - 1);
        let ppn0 = (phys >> 12) & ((1 << 10) - 1);
        let ppo = (phys) & ((1 << 12) - 1);

        let vpn1 = (virt >> 22) & ((1 << 10) - 1);
        let vpn0 = (virt >> 12) & ((1 << 10) - 1);
        let vpo = (virt) & ((1 << 12) - 1);

        assert!(ppn1 < 4096);
        assert!(ppn0 < 1024);
        assert!(ppo < 4096);
        assert!(vpn1 < 1024);
        assert!(vpn0 < 1024);
        assert!(vpo < 4096);

        let l1_pt = &mut root.entries;
        let mut new_addr = None;

        // Allocate a new level 1 pagetable entry if one doesn't exist.
        if l1_pt[vpn1] & FLG_VALID == 0 {
            let na = self.alloc() as usize;
            if VDBG {
                println!(
                    "The Level 1 page table is invalid ({:08x}) @ {:08x} -- allocating a new one @ {:08x}",
                    unsafe { l1_pt.as_ptr().add(vpn1) } as usize,
                    l1_pt[vpn1],
                    na
                );
            }
            // Mark this entry as a leaf node (WRX as 0), and indicate
            // it is a valid page by setting "V".
            l1_pt[vpn1] = ((na >> 12) << 10) | FLG_VALID;
            new_addr = Some(NonZeroUsize::new(na).unwrap());
        }

        let l0_pt_idx = unsafe { &mut (*(((l1_pt[vpn1] << 2) & !((1 << 12) - 1)) as *mut PageTable)) };
        let l0_pt = &mut l0_pt_idx.entries;

        // Ensure the entry hasn't already been mapped to a different address.
        if l0_pt[vpn0] & 1 != 0 && (l0_pt[vpn0] & 0xffff_fc00) != ((ppn1 << 20) | (ppn0 << 10)) {
            panic!(
                "Page {:08x} was already allocated to {:08x}, so cannot map to {:08x}!",
                phys,
                (l0_pt[vpn0] >> 10) << 12,
                virt
            );
        }
        let previous_flags = l0_pt[vpn0] & 0xf;
        l0_pt[vpn0] = (ppn1 << 20) | (ppn0 << 10) | flags | previous_flags | FLG_D | FLG_A;

        // If we had to allocate a level 1 pagetable entry, ensure that it's
        // mapped into our address space, owned by PID 1.
        if let Some(addr) = new_addr {
            if VDBG {
                println!(
                    ">>> Mapping new address {:08x} -> {:08x}",
                    addr.get(),
                    PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE
                );
            }
            self.map_page(
                root,
                addr.get(),
                PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE,
                FLG_R | FLG_W | FLG_VALID,
                owner,
            );
            if VDBG {
                println!("<<< Done mapping new address");
            }
        }
    }
}
