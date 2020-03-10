use crate::mem::MemoryManager;
use core::fmt;
use vexriscv::register::satp;
use xous::{MemoryFlags, PID};

pub const DEFAULT_STACK_TOP: usize = 0x8000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
pub const PAGE_SIZE: usize = 4096;
const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;

extern "C" {
    fn flush_mmu();
}

bitflags! {
    pub struct MMUFlags: usize {
        const NONE      = 0b00000000;
        const VALID     = 0b00000001;
        const R         = 0b00000010;
        const W         = 0b00000100;
        const X         = 0b00001000;
        const USER      = 0b00010000;
        const GLOBAL    = 0b00100000;
        const A         = 0b01000000;
        const D         = 0b10000000;
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
            (self.satp >> 0 & ((1 << 22) - 1)) << 12,
        )
    }
}

fn translate_flags(req_flags: MemoryFlags) -> MMUFlags {
    let mut flags = MMUFlags::NONE;
    if req_flags & xous::MemoryFlags::R == xous::MemoryFlags::R {
        flags |= MMUFlags::R;
    }
    if req_flags & xous::MemoryFlags::W == xous::MemoryFlags::W {
        flags |= MMUFlags::W;
    }
    if req_flags & xous::MemoryFlags::X == xous::MemoryFlags::X {
        flags |= MMUFlags::X;
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
    pub unsafe fn from_raw(&mut self, satp: usize) {
        self.satp = satp;
    }

    /// Get the currently active memory mapping.  Note that the actual root pages
    /// may be found at virtual address `PAGE_TABLE_ROOT_OFFSET`.
    pub fn current() -> MemoryMapping {
        MemoryMapping {
            satp: satp::read().bits(),
        }
    }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> PID {
        (self.satp >> 22 & ((1 << 9) - 1)) as PID
    }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(&self) {
        satp::write(self.satp);
    }

    // /// Get the flags for a given address, or `0` if none is set.
    // pub fn flags_for_address(&self, addr: usize) -> usize {
    //     let vpn1 = (addr >> 22) & ((1 << 10) - 1);
    //     let vpn0 = (addr >> 12) & ((1 << 10) - 1);

    //     let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    //     let l0_pt = l1_pt.entries[vpn1];
    //     if l0_pt & 1 == 0 {
    //         return 0;
    //     }
    //     let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    //     let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };
    //     l0_pt.entries[vpn0]
    // }

    pub fn print_map(&self) {
        println!("Memory Maps:");
        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        for (i, l1_entry) in l1_pt.entries.iter().enumerate() {
            if *l1_entry == 0 {
                continue;
            }
            let superpage_addr = i as u32 * (1 << 22);
            println!(
                "    {:4} Superpage for {:08x} @ {:08x} (flags: {:?})",
                i,
                superpage_addr,
                (*l1_entry >> 10) << 12,
                MMUFlags::from_bits(l1_entry & 0xff).unwrap()
            );

            // Page 1023 is only available to PID1
            if i == 1023 {
                if self.get_pid() != 1 {
                    println!("        <unavailable>");
                    continue;
                }
            }
            // let l0_pt_addr = ((l1_entry >> 10) << 12) as *const u32;
            let l0_pt = unsafe { &mut (*((PAGE_TABLE_OFFSET + i * 4096) as *mut LeafPageTable)) };
            for (j, l0_entry) in l0_pt.entries.iter().enumerate() {
                if *l0_entry & 1 == 0 {
                    continue;
                }
                let page_addr = j as u32 * (1 << 12);
                println!(
                    "        {:4} {:08x} -> {:08x} (flags: {:?})",
                    j,
                    superpage_addr + page_addr,
                    (*l0_entry >> 10) << 12,
                    MMUFlags::from_bits(l0_entry & 0xff).unwrap()
                );
            }
        }
        println!("End of map");
    }

    /// Get the pagetable entry for a given address, or `0` if none is set.
    pub fn pagetable_entry(&self, addr: usize) -> *mut usize {
        let vpn1 = (addr >> 22) & ((1 << 10) - 1);
        let vpn0 = (addr >> 12) & ((1 << 10) - 1);

        let l1_pt = unsafe { &(*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        let l1_pte = l1_pt.entries[vpn1];
        if l1_pte & 1 == 0 {
            return 0 as *mut usize;
        }
        let l0_pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
        let entry = (l0_pt_virt + vpn0*4) as *mut usize;
        entry
    }

    pub fn reserve_address(
        &mut self,
        mm: &mut MemoryManager,
        addr: usize,
        flags: MemoryFlags,
    ) -> Result<(), xous::Error> {
        let vpn1 = (addr >> 22) & ((1 << 10) - 1);
        let vpn0 = (addr >> 12) & ((1 << 10) - 1);

        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;

        // Allocate a new level 1 pagetable entry if one doesn't exist.
        if l1_pt.entries[vpn1] & MMUFlags::VALID.bits() == 0 {
            let pid = crate::arch::current_pid();
            // Allocate a fresh page
            let l0pt_phys = mm.alloc_page(pid)?;

            // Mark this entry as a leaf node (WRX as 0), and indicate
            // it is a valid page by setting "V".
            l1_pt.entries[vpn1] = ((l0pt_phys >> 12) << 10) | MMUFlags::VALID.bits();
            unsafe { flush_mmu() };

            // Map the new physical page to the virtual page, so we can access it.
            map_page_inner(
                mm,
                pid,
                l0pt_phys,
                l0pt_virt,
                MemoryFlags::W | MemoryFlags::R,
            )?;

            // Zero-out the new page
            let page_addr = l0pt_virt as *mut usize;
            unsafe {
                for i in 0..PAGE_SIZE / core::mem::size_of::<usize>() {
                    *page_addr.add(i) = 0;
                }
            }
        }

        let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };
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
) -> Result<(), xous::Error> {
    let ppn1 = (phys >> 22) & ((1 << 12) - 1);
    let ppn0 = (phys >> 12) & ((1 << 10) - 1);
    let ppo = (phys >> 0) & ((1 << 12) - 1);

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = (virt >> 0) & ((1 << 12) - 1);

    let mut flags = translate_flags(req_flags);
    // The kernel runs in Supervisor mode, and therefore always needs
    // exclusive access to this memory.
    // Additionally, any address below the user area must be accessible
    // by the kernel.
    if pid != 1 && virt < USER_AREA_END {
        flags |= MMUFlags::USER;
    }

    assert!(ppn1 < 4096);
    assert!(ppn0 < 1024);
    assert!(ppo < 4096);
    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let ref mut l1_pt = l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // Allocate a new level 1 pagetable entry if one doesn't exist.
    if l1_pt[vpn1 as usize] & MMUFlags::VALID.bits() == 0 {
        // Allocate a fresh page
        let l0pt_phys = mm.alloc_page(pid)?;

        // Mark this entry as a leaf node (WRX as 0), and indicate
        // it is a valid page by setting "V".
        l1_pt[vpn1 as usize] = ((l0pt_phys >> 12) << 10) | MMUFlags::VALID.bits();
        unsafe { flush_mmu() };

        // Map the new physical page to the virtual page, so we can access it.
        map_page_inner(
            mm,
            pid,
            l0pt_phys,
            l0pt_virt,
            MemoryFlags::W | MemoryFlags::R,
        )?;

        // Zero-out the new page
        let page_addr = l0pt_virt as *mut usize;
        unsafe {
            for i in 0..PAGE_SIZE / core::mem::size_of::<usize>() {
                *page_addr.add(i) = 0;
            }
        }
    }

    // Ensure the entry hasn't already been mapped.
    if l0_pt.entries[vpn0 as usize] & 1 != 0 {
        // println!("Page {:08x} already allocated!", virt);
    }
    l0_pt.entries[vpn0 as usize] =
        (ppn1 << 20) | (ppn0 << 10) | (flags | MMUFlags::VALID | MMUFlags::D | MMUFlags::A).bits();
    unsafe { flush_mmu() };

    Ok(())
}

/// Ummap the given page from the specified process table.  Never
/// allocate a new page.
///
/// # Errors
///
/// * BadAddress - Address was not already mapped.
pub fn unmap_page_inner(
    _mm: &mut MemoryManager,
    virt: usize,
) -> Result<(), xous::Error> {
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = (virt >> 0) & ((1 << 12) - 1);

    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);
    assert!(vpo < 4096);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let ref mut l1_pt = l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // Allocate a new level 1 pagetable entry if one doesn't exist.
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous::Error::BadAddress);
    }

    // Ensure the entry hasn't already been mapped.
    if l0_pt.entries[vpn0] & 1 == 0 {
        return Err(xous::Error::BadAddress);
    }
    l0_pt.entries[vpn0] = 0;
    unsafe { flush_mmu() };

    Ok(())
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous::Error> {
    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at this address.
    let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let ref mut l1_pt = l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

    // Allocate a new level 1 pagetable entry if one doesn't exist.
    if l1_pt[vpn1] & MMUFlags::VALID.bits() == 0 {
        return Err(xous::Error::BadAddress);
    }

    // Ensure the entry hasn't already been mapped.
    if l0_pt.entries[vpn0] & 1 == 0 {
        return Err(xous::Error::BadAddress);
    }
    Ok(l0_pt.entries[vpn0])
}
