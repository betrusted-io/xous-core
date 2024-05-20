// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

use riscv::register::satp;
#[cfg(feature = "gdb-stub")]
use riscv::register::sstatus;
use xous_kernel::{MemoryFlags, PID};

use crate::arch::process::InitialProcess;
use crate::mem::MemoryManager;
#[cfg(feature = "swap")]
use crate::swap::Swap;

// pub const DEFAULT_STACK_TOP: usize = 0x8000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const THREAD_CONTEXT_AREA: usize = 0xff80_1000;

pub const FLG_VALID: usize = 0x1;
pub const FLG_R: usize = 0x2;
pub const FLG_W: usize = 0x4;
// pub const FLG_X: usize = 0x8;
pub const FLG_U: usize = 0x10;
pub const FLG_A: usize = 0x40;
pub const FLG_D: usize = 0x80;

extern "C" {
    pub fn flush_mmu();
}

unsafe fn zeropage(s: *mut u32) {
    let page = core::slice::from_raw_parts_mut(s, PAGE_SIZE / core::mem::size_of::<u32>());
    page.fill(0);
}

bitflags! {
    pub struct MMUFlags: usize {
        const NONE      = 0b00_0000_0000;
        const VALID     = 0b00_0000_0001;
        const R         = 0b00_0000_0010;
        const W         = 0b00_0000_0100;
        const X         = 0b00_0000_1000;
        const USER      = 0b00_0001_0000;
        const GLOBAL    = 0b00_0010_0000;
        const A         = 0b00_0100_0000;
        const D         = 0b00_1000_0000;
        const S         = 0b01_0000_0000; // Shared page
        const P         = 0b10_0000_0000; // swaP
    }
}

#[derive(Copy, Clone, Default, PartialEq)]
pub struct MemoryMapping {
    satp: usize,
}

impl core::fmt::Debug for MemoryMapping {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "(satp: 0x{:08x}, mode: {}, ASID: {}, PPN: {:08x})",
            self.satp,
            self.satp >> 31,
            self.satp >> 22 & ((1 << 9) - 1),
            (self.satp & ((1 << 22) - 1)) << 12,
        )
    }
}

fn translate_flags(req_flags: MemoryFlags) -> MMUFlags {
    let mut flags = MMUFlags::NONE;
    if req_flags & xous_kernel::MemoryFlags::R == xous_kernel::MemoryFlags::R {
        flags |= MMUFlags::R;
    }
    if req_flags & xous_kernel::MemoryFlags::W == xous_kernel::MemoryFlags::W {
        flags |= MMUFlags::W;
    }
    if req_flags & xous_kernel::MemoryFlags::X == xous_kernel::MemoryFlags::X {
        flags |= MMUFlags::X;
    }
    flags
}

fn untranslate_flags(req_flags: usize) -> MemoryFlags {
    let req_flags = MMUFlags::from_bits_truncate(req_flags);
    let mut flags = xous_kernel::MemoryFlags::FREE;
    if req_flags & MMUFlags::R == MMUFlags::R {
        flags |= xous_kernel::MemoryFlags::R;
    }
    if req_flags & MMUFlags::W == MMUFlags::W {
        flags |= xous_kernel::MemoryFlags::W;
    }
    if req_flags & MMUFlags::X == MMUFlags::X {
        flags |= xous_kernel::MemoryFlags::X;
    }
    flags
}

/// Controls MMU configurations.
impl MemoryMapping {
    /// Create a new MemoryMapping with the given SATP value.
    /// Note that the SATP contains a physical address.
    /// The specified address MUST be mapped to `PAGE_TABLE_ROOT_OFFSET`.
    // pub fn set(&mut self, root_addr: usize, pid: PID) {
    //     self.satp: 0x8000_0000 | (((pid as usize) << 22) & (((1 << 9) - 1) << 22)) | (root_addr >> 12)
    // }
    #[allow(dead_code)]
    pub unsafe fn from_raw(&mut self, satp: usize) { self.satp = satp; }

    pub unsafe fn from_init_process(&mut self, init: InitialProcess) { self.satp = init.satp; }

    /// Allocate a brand-new memory mapping. When this memory mapping is created,
    /// it will be ready to use in a new process, however it will have no actual
    /// program code. It will, however, have the following pages mapped:
    ///
    ///     1. The kernel will be mapped to superpage 1023, meaning the kernel can switch to this process and
    ///        do things.
    ///     2. A page will be allocated for superpage 1022, to contain pages for process-specific code.
    ///     3. A page will be allocated for superpage 1021, to contain pages for managing pages.
    ///     4. The root pagetable will be allocated and mapped at 0xff800000, ensuring new superpages can be
    ///        allocated.
    ///     5. A context page will be allocated at 0xff801000, ensuring the process can actually be run.
    ///     6. Individual pagetable mappings are mapped at 0xff400000
    /// At the end of this operation, the following mapping will take place. Note that
    /// names are repeated in the chart below to indicate they are the same page
    /// represented multiple times. Items in brackets are offsets (in `usize`-words)
    /// from the start of the page. For example, offset 1023 on the root pagetable
    /// (address 4092) contains an entry that points to the kernel superpage.
    ///                         +----------------+
    ///                         | Root Pagetable |
    ///                         |      root      |
    ///                         +----------------+
    ///                                  |
    ///                  +---------------+-------------------+------------------+
    ///                  |                                   |                  |
    ///               [1021]                              [1022]             [1023]
    ///                  v                                   v                  v
    ///          +--------------+                    +--------------+       +--------+
    ///          | Level 0/1021 |                    | Level 0/1022 |       | Kernel |
    ///          |   pages_l0   |                    |  process_l0  |       |        |
    ///          +--------------+                    +--------------+       +--------+
    ///                  |                                   |
    ///          +-------+---------+                     +---+-----------+
    ///          |                 |                     |               |
    ///       [1021]            [1022]                  [0]             [1]
    ///          v                 v                     v               v
    ///  +--------------+  +--------------+     +----------------+  +---------+
    ///  | Level 0/1021 |  | Level 0/1022 |     | Root Pagetable |  | Context |
    ///  +--------------+  +--------------+     +----------------+  +---------+
    pub unsafe fn allocate(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        if self.satp != 0 {
            return Err(xous_kernel::Error::MemoryInUse);
        }

        let current_pid = crate::arch::process::current_pid();

        crate::mem::MemoryManager::with_mut(|memory_manager| {
            // Address of the root pagetable
            let root_temp_virt = memory_manager.map_zeroed_page(current_pid, false)?;
            let root_phys = super::mem::virt_to_phys(root_temp_virt as usize).unwrap() as usize;
            let root_virt = PAGE_TABLE_ROOT_OFFSET;
            let root_vpn0 = (root_virt as usize >> 12) & ((1 << 10) - 1);
            let root_ppn = ((root_phys >> 12) << 10) | FLG_VALID | FLG_R | FLG_W | FLG_D | FLG_A;

            // Superpage that points to all other pagetables
            let pages_l0_temp_virt = memory_manager.map_zeroed_page(current_pid, false)?;
            let pages_l0_virt = PAGE_TABLE_OFFSET + 4096 * 1021;
            let pages_l0_phys = super::mem::virt_to_phys(pages_l0_temp_virt as usize)? as usize;
            let pages_l0_vpn0 = (pages_l0_virt as usize >> 12) & ((1 << 10) - 1);
            let pages_l0_ppn = ((pages_l0_phys >> 12) << 10) | FLG_VALID | FLG_R | FLG_W | FLG_D | FLG_A;

            // Superpage that points to process-specific pages
            let process_l0_temp_virt = memory_manager.map_zeroed_page(current_pid, false)?;
            let process_l0_virt = PAGE_TABLE_OFFSET + 4096 * 1022;
            let process_l0_phys = super::mem::virt_to_phys(process_l0_temp_virt as usize)? as usize;
            let process_l0_vpn0 = (process_l0_virt as usize >> 12) & ((1 << 10) - 1);
            let process_l0_ppn = ((process_l0_phys >> 12) << 10) | FLG_VALID | FLG_R | FLG_W | FLG_D | FLG_A;

            // Context switch information containing all thread information.
            let context_temp_virt = memory_manager.map_zeroed_page(current_pid, false)?;
            let context_virt = THREAD_CONTEXT_AREA;
            let context_phys = super::mem::virt_to_phys(context_temp_virt as usize)? as usize;
            let context_vpn0 = (context_virt as usize >> 12) & ((1 << 10) - 1);
            let context_ppn = ((context_phys >> 12) << 10) | FLG_VALID | FLG_R | FLG_W | FLG_D | FLG_A;

            // Map the kernel into the new process mapping so we can continue
            // execution when it is activated. We can copy this value from our
            // current pagetable mapping.
            let krn_pg1023_ptr = (PAGE_TABLE_ROOT_OFFSET as *const usize).add(1023).read_volatile();
            root_temp_virt.add(1023).write_volatile(krn_pg1023_ptr);

            // Map the process superpage into itself.
            root_temp_virt
                .add(PAGE_TABLE_ROOT_OFFSET >> 22)
                .write_volatile((process_l0_phys >> 12) << 10 | FLG_VALID);

            // Map the pagetable superpage into itself.
            root_temp_virt
                .add(PAGE_TABLE_OFFSET >> 22)
                .write_volatile((pages_l0_phys >> 12) << 10 | FLG_VALID);

            // Map the root pagetable and the context page into the new process
            process_l0_temp_virt.add(root_vpn0).write_volatile(root_ppn);
            process_l0_temp_virt.add(context_vpn0).write_volatile(context_ppn);

            // Add the the pagetable superpage to the l0 pagetable.
            pages_l0_temp_virt.add(process_l0_vpn0).write_volatile(process_l0_ppn);
            pages_l0_temp_virt.add(pages_l0_vpn0).write_volatile(pages_l0_ppn);

            // Mark the four pages as being owned by the new process
            memory_manager.move_page_raw(root_phys as *mut usize, pid)?;
            memory_manager.move_page_raw(pages_l0_phys as *mut usize, pid)?;
            memory_manager.move_page_raw(process_l0_phys as *mut usize, pid)?;
            memory_manager.move_page_raw(context_phys as *mut usize, pid)?;

            // Unmap our copies of the four pages
            unmap_page_inner(memory_manager, root_temp_virt as usize)?;
            unmap_page_inner(memory_manager, pages_l0_temp_virt as usize)?;
            unmap_page_inner(memory_manager, process_l0_temp_virt as usize)?;
            unmap_page_inner(memory_manager, context_temp_virt as usize)?;

            // Construct a dummy SATP that we will use to hand memory to the new process.
            self.satp = 0x8000_0000 | ((pid.get() as usize) << 22) | (root_phys as usize >> 12);
            Ok(())
        })?;

        Ok(())
    }

    /// Get the currently active memory mapping.  Note that the actual root pages
    /// may be found at virtual address `PAGE_TABLE_ROOT_OFFSET`.
    pub fn current() -> MemoryMapping { MemoryMapping { satp: satp::read().bits() } }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> Option<PID> { PID::new((self.satp >> 22 & ((1 << 9) - 1)) as _) }

    pub fn is_allocated(&self) -> bool { self.get_pid().is_some() }

    pub fn is_kernel(&self) -> bool { self.get_pid().map(|v| v.get() == 1).unwrap_or(false) }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(self) -> Result<(), xous_kernel::Error> {
        unsafe { flush_mmu() };
        satp::write(self.satp);
        unsafe { flush_mmu() };
        Ok(())
    }

    pub fn phys_to_virt(&self, phys: usize) -> Result<Option<u32>, xous_kernel::Error> {
        let mut found = None;
        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        if phys & PAGE_SIZE - 1 != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        for (i, l1_entry) in l1_pt.entries.iter().enumerate() {
            if *l1_entry == 0 {
                continue;
            }
            let _superpage_addr = i as u32 * (1 << 22);

            // Page 1023 is only available to PID1
            if i == 1023 && !self.is_kernel() {
                continue;
            }
            let l0_pt = unsafe { &mut (*((PAGE_TABLE_OFFSET + i * 4096) as *mut LeafPageTable)) };
            for (j, l0_entry) in l0_pt.entries.iter().enumerate() {
                if *l0_entry & 0x7 == 0 {
                    continue;
                }
                let _page_addr = j as u32 * (1 << 12);
                let virt_addr = _superpage_addr + _page_addr;
                let phys_addr = (*l0_entry >> 10) << 12;
                let valid = (l0_entry & MMUFlags::VALID.bits()) != 0;
                let shared = (l0_entry & MMUFlags::S.bits()) != 0;
                if phys_addr == phys && (valid || shared) {
                    if found.is_none() {
                        found = Some(virt_addr);
                    } else {
                        println!("Page is mapped twice within process {:08x}!", phys_addr);
                        return Err(xous_kernel::Error::MemoryInUse);
                    }
                }
            }
        }
        Ok(found)
    }

    pub fn print_map(&self) {
        if !self.is_allocated() {
            println!("Process isn't allocated!");
            return;
        }
        println!("Memory Maps for PID {}:", self.get_pid().map(|v| v.get()).unwrap_or(0));
        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        for (i, l1_entry) in l1_pt.entries.iter().enumerate() {
            if *l1_entry == 0 {
                continue;
            }
            let _superpage_addr = i as u32 * (1 << 22);
            #[cfg(all(feature = "swap", feature = "renode"))]
            // skip printing the mem-mapped swap for renode targets in swap debug, makes the PT dumps a lot
            // more compact
            if _superpage_addr & 0xF000_0000 == 0xA000_0000 {
                continue;
            }
            println!(
                "    {:4} Superpage for {:08x} @ {:08x} (flags: {:?})",
                i,
                _superpage_addr,
                (*l1_entry >> 10) << 12,
                MMUFlags::from_bits(l1_entry & 0x3ff).unwrap()
            );

            // Page 1023 is only available to PID1
            if i == 1023 && !self.is_kernel() {
                println!("        <unavailable>");
                continue;
            }
            // let l0_pt_addr = ((l1_entry >> 10) << 12) as *const u32;
            let l0_pt = unsafe { &mut (*((PAGE_TABLE_OFFSET + i * 4096) as *mut LeafPageTable)) };
            for (j, l0_entry) in l0_pt.entries.iter().enumerate() {
                if *l0_entry & 0x7 == 0 {
                    continue;
                }
                let _page_addr = j as u32 * (1 << 12);
                println!(
                    "        {:4} {:08x} -> {:08x} (flags: {:?})",
                    j,
                    _superpage_addr + _page_addr,
                    (*l0_entry >> 10) << 12,
                    MMUFlags::from_bits(l0_entry & 0x3ff).unwrap()
                );
            }
        }
        println!("End of map");
    }

    pub fn reserve_address(
        &mut self,
        mm: &mut MemoryManager,
        addr: usize,
        flags: MemoryFlags,
    ) -> Result<(), xous_kernel::Error> {
        let vpn1 = (addr >> 22) & ((1 << 10) - 1);
        let vpn0 = (addr >> 12) & ((1 << 10) - 1);

        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;

        // println!("Reserving memory address {:08x} with flags {:?}", addr, flags);
        // Allocate a new level 1 pagetable entry if one doesn't exist.
        if l1_pt.entries[vpn1] & MMUFlags::VALID.bits() == 0 {
            let pid = crate::arch::current_pid();
            // Allocate a fresh page
            #[cfg(not(feature = "swap"))]
            let l0pt_phys = mm.alloc_page(pid)?;
            #[cfg(feature = "swap")]
            let l0pt_phys = mm.alloc_page(pid, None)?;

            // Mark this entry as a leaf node (WRX as 0), and indicate
            // it is a valid page by setting "V".
            l1_pt.entries[vpn1] = ((l0pt_phys >> 12) << 10) | MMUFlags::VALID.bits();
            unsafe { flush_mmu() };

            // Map the new physical page to the virtual page, so we can access it.
            map_page_inner(mm, pid, l0pt_phys, l0pt_virt, MemoryFlags::W | MemoryFlags::R, false)?;

            // Zero-out the new page
            let page_addr = l0pt_virt as *mut usize;
            unsafe { zeropage(page_addr as *mut u32) };
        }

        let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };
        let current_mapping = l0_pt.entries[vpn0];
        if current_mapping & 1 == 1 {
            return Ok(());
        }
        l0_pt.entries[vpn0] = translate_flags(flags).bits();
        Ok(())
    }
}

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { satp: 0 };

/// A single RISC-V page table entry.  In order to resolve an address,
/// we need two entries: the top level, followed by the lower level.
struct RootPageTable {
    entries: [usize; 1024],
}

struct LeafPageTable {
    entries: [usize; 1024],
}

impl fmt::Display for RootPageTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, entry) in self.entries.iter().enumerate() {
            if *entry != 0 {
                writeln!(
                    f,
                    "    {:4} {:08x} -> {:08x} ({})",
                    i,
                    (entry >> 10) << 12,
                    i * (1 << 22),
                    entry & 0xff
                )?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for LeafPageTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, entry) in self.entries.iter().enumerate() {
            if *entry != 0 {
                writeln!(
                    f,
                    "    {:4} {:08x} -> {:08x} ({})",
                    i,
                    (entry >> 10) << 12,
                    i * (1 << 10),
                    entry & 0xff
                )?;
            }
        }
        Ok(())
    }
}

/// When we allocate pages, they are owned by the kernel so we can zero
/// them out.  After that is done, hand the page to the user.
pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    let virt = virt as usize;
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = virt & ((1 << 12) - 1);

    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // PAGE_TABLE_OFFSET
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address isn't valid.
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Ensure the entry hasn't already been mapped.
    if l0_pt.entries[vpn0] & 1 == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Add the USER flag to the entry
    l0_pt.entries[vpn0] |= MMUFlags::USER.bits();
    unsafe { flush_mmu() };

    Ok(())
}

#[cfg(feature = "gdb-stub")]
pub fn peek_memory<T>(addr: *mut T) -> Result<T, xous_kernel::Error> {
    let virt = addr as usize;
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = virt & ((1 << 12) - 1);

    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // PAGE_TABLE_OFFSET
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address isn't valid.
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Ensure the entry has already been mapped, and that we're allowed
    // to read it.
    if l0_pt.entries[vpn0] & (MMUFlags::R | MMUFlags::VALID).bits() != (MMUFlags::R | MMUFlags::VALID).bits()
    {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Enable supervisor access to user mode
    unsafe { sstatus::set_sum() };

    // Perform the read
    let val = unsafe { addr.read_volatile() };

    // Remove supervisor access to user mode
    unsafe { sstatus::clear_sum() };

    Ok(val)
}

#[cfg(feature = "gdb-stub")]
pub fn poke_memory<T>(addr: *mut T, val: T) -> Result<(), xous_kernel::Error> {
    let virt = addr as usize;
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = virt & ((1 << 12) - 1);

    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // PAGE_TABLE_OFFSET
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address isn't valid.
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Ensure the entry has been mapped.
    if l0_pt.entries[vpn0] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // Ensure we're allowed to read it.
    let was_writable = l0_pt.entries[vpn0] & MMUFlags::W.bits() != 0;

    // Add the WRITE bit, which allows us to patch things like
    // program code.
    if !was_writable {
        l0_pt.entries[vpn0] |= MMUFlags::W.bits();
        unsafe { flush_mmu() };
    }

    // Enable supervisor access to user mode
    unsafe { sstatus::set_sum() };

    // Perform the write
    unsafe { addr.write_volatile(val) };

    // Remove supervisor access to user mode
    unsafe { sstatus::clear_sum() };

    // Remove the WRITE bit if it wasn't previously set
    if !was_writable {
        l0_pt.entries[vpn0] &= !MMUFlags::W.bits();
        unsafe { flush_mmu() };
    }

    Ok(())
}

/// Map the given page to the specified process table.  If necessary,
/// allocate a new page.
///
/// # Errors
///
/// * OutOfMemory - Tried to allocate a new pagetable, but ran out of memory.
pub fn map_page_inner(
    mm: &mut MemoryManager,
    pid: PID,
    phys: usize,
    virt: usize,
    req_flags: MemoryFlags,
    map_user: bool,
) -> Result<(), xous_kernel::Error> {
    let ppn1 = (phys >> 22) & ((1 << 12) - 1);
    let ppn0 = (phys >> 12) & ((1 << 10) - 1);
    let ppo = phys & ((1 << 12) - 1);

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = virt & ((1 << 12) - 1);

    let flags = translate_flags(req_flags) | if map_user { MMUFlags::USER } else { MMUFlags::NONE };

    assert!(ppn1 < 4096);
    assert!(ppn0 < 1024);
    assert!(ppo < 4096);
    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);
    assert!((virt & 0xfff) == 0);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at 0xff80_0000.
    let l1_pt = PAGE_TABLE_ROOT_OFFSET as *mut usize;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0xff40_0000.
    let l0_pt = (PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE) as *mut usize;

    // Allocate a new level 1 pagetable entry if one doesn't exist.
    if unsafe { l1_pt.add(vpn1).read_volatile() } & MMUFlags::VALID.bits() == 0 {
        // Allocate a fresh page for the level 1 page table.
        #[cfg(not(feature = "swap"))]
        let l0_pt_phys = mm.alloc_page(pid)?;
        #[cfg(feature = "swap")]
        let l0_pt_phys = mm.alloc_page(pid, None)?;

        // Mark this entry as a leaf node (WRX as 0), and indicate
        // it is a valid page by setting "V".
        unsafe {
            l1_pt.add(vpn1).write_volatile(((l0_pt_phys >> 12) << 10) | MMUFlags::VALID.bits());
            flush_mmu();
        }

        // Map the new physical page to the virtual page, so we can access it.
        map_page_inner(mm, pid, l0_pt_phys, l0_pt as usize, MemoryFlags::W | MemoryFlags::R, false)?;

        // Zero-out the new page
        unsafe { zeropage(l0_pt as *mut u32) };
    }

    // Ensure the entry hasn't already been mapped.
    if unsafe { l0_pt.add(vpn0).read_volatile() } & 1 != 0 {
        panic!("Page {:08x} already allocated!", virt);
    }
    unsafe {
        l0_pt.add(vpn0).write_volatile(
            (ppn1 << 20) | (ppn0 << 10) | (flags | MMUFlags::VALID | MMUFlags::D | MMUFlags::A).bits(),
        )
    };
    unsafe { flush_mmu() };

    Ok(())
}

/// Get the pagetable entry for a given address, or `Err()` if the address is invalid
pub fn pagetable_entry(addr: usize) -> Result<*mut usize, xous_kernel::Error> {
    if addr & 3 != 0 {
        return Err(xous_kernel::Error::BadAlignment);
    }
    let vpn1 = (addr >> 22) & ((1 << 10) - 1);
    let vpn0 = (addr >> 12) & ((1 << 10) - 1);
    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);

    let l1_pt = unsafe { &(*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pte = l1_pt.entries[vpn1];
    if l1_pte & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    Ok((PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE + vpn0 * 4) as *mut usize)
}

/// Ummap the given page from the specified process table.  Never allocate a new
/// page.
///
/// # Returns
///
/// The physical address for the page that was just unmapped
///
/// # Errors
///
/// * BadAddress - Address was not already mapped.
pub fn unmap_page_inner(_mm: &mut MemoryManager, virt: usize) -> Result<usize, xous_kernel::Error> {
    let entry = pagetable_entry(virt)?;

    let phys = (unsafe { entry.read_volatile() } >> 10) << 12;
    unsafe { entry.write_volatile(0) };
    unsafe { flush_mmu() };

    Ok(phys)
}

/// Move a page from one address space to another.
pub fn move_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<(), xous_kernel::Error> {
    let entry = pagetable_entry(src_addr as usize)?;
    let previous_entry = unsafe { entry.read_volatile() };
    if previous_entry & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    // Invalidate the old entry
    unsafe { entry.write_volatile(0) };
    unsafe { flush_mmu() };

    dest_space.activate()?;
    let phys = previous_entry >> 10 << 12;
    let flags = untranslate_flags(previous_entry);

    let result = map_page_inner(mm, dest_pid, phys, dest_addr as usize, flags, dest_pid.get() != 1);

    // Switch back to the original address space and return
    src_space.activate().unwrap();
    result
}

/// Determine if a virtual page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    pagetable_entry(src_addr as usize)
        .map_or(false, |v| unsafe { v.read_volatile() } & MMUFlags::S.bits() != 0)
}

/// Mark the given virtual address as being lent.  If `writable`, clear the
/// `valid` bit so that this process can't accidentally write to this page while
/// it is lent.
///
/// This uses the `RWS` fields to keep track of the following pieces of information:
///
/// * **PTE[8]**: This is set to `1` indicating the page is lent
/// * **PTE[9]**: This is `1` if the page was previously writable
///
/// # Returns
///
/// # Errors
///
/// * **BadAlignment**: The page isn't 4096-bytes aligned
/// * **BadAddress**: The page isn't allocated
pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
    mutable: bool,
) -> Result<usize, xous_kernel::Error> {
    //klog!("***lend - src: {:08x} dest: {:08x}***", src_addr as u32, dest_addr as u32);
    let entry = pagetable_entry(src_addr as usize)?;
    let current_entry = unsafe { entry.read_volatile() };
    let phys = (current_entry >> 10) << 12;

    // If we try to share a page that's not ours, that's just wrong.
    if current_entry & MMUFlags::VALID.bits() == 0 {
        // klog!("Not valid");
        return Err(xous_kernel::Error::ShareViolation);
    }

    // If we try to share a page that's already shared, that's a sharing
    // violation.
    if current_entry & MMUFlags::S.bits() != 0 {
        // klog!("Already shared");
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the `VALID` flag, and set the `SHARED` flag.
    let new_entry = (current_entry & !MMUFlags::VALID.bits()) | MMUFlags::S.bits();
    unsafe { entry.write_volatile(new_entry) };

    // Ensure the change takes effect.
    unsafe { flush_mmu() };

    // Mark the page as Writable in new process space if it's writable here.
    let new_flags = if mutable && (new_entry & MMUFlags::W.bits()) != 0 {
        MemoryFlags::R | MemoryFlags::W
    } else {
        MemoryFlags::R
    };

    // Switch to the new address space and map the page
    dest_space.activate()?;
    let result = map_page_inner(mm, dest_pid, phys, dest_addr as usize, new_flags, dest_pid.get() != 1);
    unsafe { flush_mmu() };

    // Switch back to our proces space
    src_space.activate().unwrap();

    // Return the new address.
    result.map(|_| phys)
}

/// Return a page from `src_space` back to `dest_space`.
pub fn return_page_inner(
    _mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    _dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<usize, xous_kernel::Error> {
    //klog!("***return - src: {:08x} dest: {:08x}***", src_addr as u32, dest_addr as u32);
    let src_entry = pagetable_entry(src_addr as usize)?;
    let src_entry_value = unsafe { src_entry.read_volatile() };
    let phys = (src_entry_value >> 10) << 12;

    // If the page is not valid in this program, we can't return it.
    if src_entry_value & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Mark the page as `Free`, which unmaps it.
    unsafe { src_entry.write_volatile(0) };
    unsafe { flush_mmu() };

    // Switch to the destination address space
    dest_space.activate()?;
    let dest_entry = pagetable_entry(dest_addr as usize).expect("page wasn't lent in destination space");
    let dest_entry_value = unsafe { dest_entry.read_volatile() };

    // If the page wasn't marked as `Shared` in the destination address space,
    // treat that as an error.
    if dest_entry_value & MMUFlags::S.bits() == 0 {
        panic!("page wasn't shared in destination space");
    }

    #[cfg(feature = "swap")]
    // Clear the `SHARED` bit, and set the `VALID` bit.
    unsafe {
        dest_entry.write_volatile(dest_entry_value & !(MMUFlags::S).bits() | MMUFlags::VALID.bits())
    };
    #[cfg(not(feature = "swap"))]
    // Clear the `SHARED` and `PREVIOUSLY-WRITABLE` bits, and set the `VALID` bit.
    unsafe {
        dest_entry
            .write_volatile(dest_entry_value & !(MMUFlags::S | MMUFlags::P).bits() | MMUFlags::VALID.bits())
    };

    unsafe { flush_mmu() };

    // Swap back to our previous address space
    src_space.activate().unwrap();
    Ok(phys)
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous_kernel::Error> {
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address is invalid
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    // If the page is "Valid" but shared, issue a sharing violation
    if l0_pt.entries[vpn0] & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Ensure the entry hasn't already been mapped.
    if l0_pt.entries[vpn0] & MMUFlags::VALID.bits() == 0 {
        // The memory has been reserved, but isn't pointing anywhere yet.
        if l0_pt.entries[vpn0] != 0 {
            return Err(xous_kernel::Error::MemoryInUse);
        }

        // The address hasn't been allocated
        return Err(xous_kernel::Error::BadAddress);
    }
    Ok((l0_pt.entries[vpn0] >> 10) << 12)
}

#[allow(dead_code)]
pub fn virt_to_phys_pid(_pid: PID, _virt: usize) -> Result<usize, xous_kernel::Error> {
    todo!("virt_to_phys_pid is not yet implemented for riscv");
}

pub fn ensure_page_exists_inner(address: usize) -> Result<usize, xous_kernel::Error> {
    // Disallow mapping memory outside of user land
    if !MemoryMapping::current().is_kernel() && address >= USER_AREA_END {
        return Err(xous_kernel::Error::OutOfMemory);
    }
    let virt = address & !0xfff;
    let entry = crate::arch::mem::pagetable_entry(virt).or(Err(xous_kernel::Error::BadAddress))?;
    // let entry = crate::arch::mem::pagetable_entry(virt).or_else(|e| {
    //     // klog!("Error in mem: {:?}", e);
    //     panic!("Page doesn't exist: {:08x}", address);
    //     Err(xous_kernel::Error::BadAddress)
    // })?;
    let current_entry = unsafe { entry.read_volatile() };

    let flags = current_entry & 0x3ff;

    #[cfg(not(feature = "swap"))]
    if flags & MMUFlags::VALID.bits() != 0 {
        return Ok(address);
    }
    #[cfg(feature = "swap")]
    if (flags & MMUFlags::VALID.bits() != 0) && (flags & MMUFlags::P.bits() == 0) {
        return Ok(address);
    }

    // If the flags are nonzero, but the "Valid" bit is not 1 and
    // the page isn't shared, then this is a reserved page. Allocate
    // a real page to back it and resume execution.
    if flags == 0 || flags & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    #[cfg(not(feature = "swap"))]
    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page(crate::arch::process::current_pid()).expect("Couldn't allocate new page")
    });
    #[cfg(feature = "swap")]
    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page_oomable(crate::arch::process::current_pid(), virt).expect("Couldn't allocate new page")
    });

    let ppn1 = (new_page >> 22) & ((1 << 12) - 1);
    let ppn0 = (new_page >> 12) & ((1 << 10) - 1);
    unsafe {
        #[cfg(feature = "swap")]
        if flags & MMUFlags::P.bits() != 0 {
            // page is swapped; fill page, map and return
            Swap::with_mut(|s| {
                s.retrieve_page(
                    crate::arch::process::current_pid(),
                    crate::arch::process::current_tid(),
                    virt,
                    new_page,
                )
            })

            // the execution flow diverges from here: it returns via the interrupt context handler. -> !
        } else {
            // page is reserved: simply zero it out
            // Map the page to our process
            *entry =
                (ppn1 << 20) | (ppn0 << 10) | (flags | FLG_VALID /* valid */ | FLG_D /* D */ | FLG_A/* A */);
            flush_mmu();
            zeropage(virt as *mut u32);
        }

        #[cfg(not(feature = "swap"))]
        {
            *entry =
                (ppn1 << 20) | (ppn0 << 10) | (flags | FLG_VALID /* valid */ | FLG_D /* D */ | FLG_A/* A */);
            flush_mmu();
            // Zero-out the page
            zeropage(virt as *mut u32);
        }

        // Move the page into userspace
        *entry = (ppn1 << 20)
            | (ppn0 << 10)
            | (flags | FLG_VALID /* valid */ | FLG_U /* USER */ | FLG_D /* D */ | FLG_A/* A */);
        flush_mmu();
    };

    Ok(new_page)
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    if let Err(e) = virt_to_phys(virt) {
        // If the value is a `BadAddress`, then that means that address is not valid
        // and is therefore available
        e == xous_kernel::Error::BadAddress
    } else {
        // If the address is not an error, then it is not available and shouldn't be used.
        false
    }
}

/// Get the `MemoryFlags` for the requested virtual address. The address must
/// be valid and page-aligned, and must not be Shared.
///
/// # Returns
///
/// * **None**: The page is not valid or is shared
/// * **Some(MemoryFlags)**: The translated sharing permissions of the given flags
pub fn page_flags(virt: usize) -> Option<MemoryFlags> {
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address is invalid
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return None;
    }

    let mmu_flags = l0_pt.entries[vpn0];

    // If the page is "Valid" but shared, issue a sharing violation
    if mmu_flags & MMUFlags::S.bits() != 0 {
        return None;
    }

    let mut return_flags = MemoryFlags::empty();

    if mmu_flags & MMUFlags::R.bits() != 0 {
        return_flags = return_flags | MemoryFlags::R;
    }

    if mmu_flags & MMUFlags::W.bits() != 0 {
        return_flags = return_flags | MemoryFlags::W;
    }

    if mmu_flags & MMUFlags::X.bits() != 0 {
        return_flags = return_flags | MemoryFlags::X;
    }

    if return_flags.is_empty() { None } else { Some(return_flags) }
}

pub fn update_page_flags(virt: usize, flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
    // The resulting flags must actually be valid
    if (flags & (MemoryFlags::R | MemoryFlags::W | MemoryFlags::X)).is_empty() {
        return Err(xous_kernel::Error::MemoryInUse);
    }

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pt = &mut l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let l0_pt = &mut unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // If the level 1 pagetable doesn't exist, then this address is invalid
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::OutOfMemory);
    }

    let mut mmu_flags = l0_pt.entries[vpn0];

    // If the page is "Valid" but shared, issue a sharing violation
    if mmu_flags & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the flags as requested
    if (flags & MemoryFlags::X).is_empty() {
        if mmu_flags & MMUFlags::X.bits() != 0 {
            mmu_flags = mmu_flags & !MMUFlags::X.bits();
        }
    } else if mmu_flags & MMUFlags::X.bits() == 0 {
        // Ensure we're not adding flags back
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the flags as requested
    if (flags & MemoryFlags::R).is_empty() {
        if mmu_flags & MMUFlags::R.bits() != 0 {
            mmu_flags = mmu_flags & !MMUFlags::R.bits();
        }
    } else if mmu_flags & MMUFlags::R.bits() == 0 {
        // Ensure we're not adding flags back
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the flags as requested
    if (flags & MemoryFlags::W).is_empty() {
        if mmu_flags & MMUFlags::W.bits() != 0 {
            mmu_flags = mmu_flags & !MMUFlags::W.bits();
        }
    } else if mmu_flags & MMUFlags::W.bits() == 0 {
        // Ensure we're not adding flags back
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Update the MMU
    l0_pt.entries[vpn0] = mmu_flags;

    Ok(())
}

#[cfg(feature = "swap")]
/// Takes in the target PID and virtual address to evict. Performs the unmapping, release from
/// the target, and re-mapping into the swapper's memory space. Returns a pointer to the
/// data in the swapper's virtual memory space
pub fn evict_page_inner(target_pid: PID, vaddr: usize) -> Result<usize, xous_kernel::Error> {
    use crate::services::SystemServices;
    SystemServices::with(|system_services| {
        // swap to the target memory space
        let target_map = system_services.get_process(target_pid).unwrap().mapping;
        target_map.activate().unwrap();

        // get the PTE in the target memory space
        let entry = pagetable_entry(vaddr as usize)?;
        let target_pte = unsafe { entry.read_volatile() };
        let target_paddr = (target_pte >> 10) << 12;

        #[cfg(feature = "debug-swap")]
        println!(
            "-- evict[{}]: {:08x} -> {:08x} (flags: {:?}), count {}",
            target_pid.get(),
            vaddr,
            target_paddr,
            MMUFlags::from_bits(target_pte & 0x3ff).unwrap(),
            unsafe { MemoryManager::with(|mm| mm.get_timestamp(target_paddr)) }
        );

        // mark the page as "touched" even if the eviction checks fail: the page is definitely not LRU if
        // it's not swappable.
        MemoryManager::with(|mm| mm.touch(target_paddr));

        // sanity check
        if (target_pte & MMUFlags::VALID.bits() == 0) || (target_pte & MMUFlags::P.bits() != 0) {
            // return us to the swapper PID -- this call can only originate in the swapper
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate()?;
            return Err(xous_kernel::Error::BadAddress);
        }
        // don't allow swapping of kernel pages
        if target_pte & MMUFlags::USER.bits() == 0 {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate()?;
            return Err(xous_kernel::Error::AccessDenied);
        }
        if target_pte & MMUFlags::S.bits() != 0 {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate()?;
            return Err(xous_kernel::Error::ShareViolation);
        }

        // clear the valid bit, mark as swapped, preserve all other flags, remove physical address
        let new_pte = (target_pte & !MMUFlags::VALID.bits() & 0x3FFusize) | MMUFlags::P.bits();
        unsafe { entry.write_volatile(new_pte) };

        // switch into the swapper memory space
        let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
        let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
        swapper_map.activate()?;
        let payload_virt = MemoryManager::with_mut(|mm| {
            let payload_virt = mm
                .find_virtual_address(core::ptr::null_mut(), PAGE_SIZE, xous_kernel::MemoryType::Messages)
                .expect("couldn't find virtual address in swapper space for target page")
                as usize;
            let _result = map_page_inner(
                mm,
                swapper_pid,
                target_paddr,
                payload_virt,
                MemoryFlags::R | MemoryFlags::W, // write flag needed because encryption is in-place
                true,
            );
            payload_virt
        });
        Ok(payload_virt)
    })
}

#[cfg(feature = "swap")]
pub fn map_page_to_swapper(paddr: usize) -> Result<usize, xous_kernel::Error> {
    use crate::services::SystemServices;
    SystemServices::with(|system_services| {
        let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
        // swap to the swapper space
        let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
        swapper_map.activate()?;

        let payload_virt = MemoryManager::with_mut(|mm| {
            let payload_virt = mm
                .find_virtual_address(core::ptr::null_mut(), PAGE_SIZE, xous_kernel::MemoryType::Messages)
                .expect("couldn't find virtual address in swapper space for target page")
                as usize;
            let _result =
                map_page_inner(mm, swapper_pid, paddr, payload_virt, MemoryFlags::R | MemoryFlags::W, true);
            payload_virt
        });
        Ok(payload_virt)
    })
}
