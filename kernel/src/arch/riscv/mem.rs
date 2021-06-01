// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::mem::MemoryManager;
use core::fmt;
use riscv::register::satp;
use xous_kernel::{MemoryFlags, PID};

// pub const DEFAULT_STACK_TOP: usize = 0x8000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const PAGE_SIZE: usize = 4096;
const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;

extern "C" {
    fn flush_mmu();
}

pub unsafe fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *s.offset(i as isize) = c as u8;
        i += 1;
    }
    s
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
        const P         = 0b10_0000_0000; // Previously writable
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
        PID::new((self.satp >> 22 & ((1 << 9) - 1)) as _).unwrap()
    }

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

    pub fn print_map(&self) {
        println!("Memory Maps for PID {}:", self.get_pid());
        let l1_pt = unsafe { &mut (*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
        for (i, l1_entry) in l1_pt.entries.iter().enumerate() {
            if *l1_entry == 0 {
                continue;
            }
            let _superpage_addr = i as u32 * (1 << 22);
            println!(
                "    {:4} Superpage for {:08x} @ {:08x} (flags: {:?})",
                i,
                _superpage_addr,
                (*l1_entry >> 10) << 12,
                MMUFlags::from_bits(l1_entry & 0xff).unwrap()
            );

            // Page 1023 is only available to PID1
            if i == 1023 {
                if self.get_pid().get() != 1 {
                    println!("        <unavailable>");
                    continue;
                }
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
                    MMUFlags::from_bits(l0_entry & 0xff).unwrap()
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
                false,
            )?;

            // Zero-out the new page
            let page_addr = l0pt_virt as *mut usize;
            unsafe { memset(page_addr as *mut u8, 0, PAGE_SIZE) };
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

/// When we allocate pages, they are owned by the kernel so we can zero
/// them out.  After that is done, hand the page to the user.
pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    let virt = virt as usize;
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
    // PAGE_TABLE_OFFSET
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

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
    let ppo = (phys >> 0) & ((1 << 12) - 1);

    let vpn1 = (virt >> 22) & ((1 << 10) - 1);
    let vpn0 = (virt >> 12) & ((1 << 10) - 1);
    let vpo = (virt >> 0) & ((1 << 12) - 1);

    let flags = translate_flags(req_flags)
        | if map_user {
            MMUFlags::USER
        } else {
            MMUFlags::NONE
        };

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
        let l0_pt_phys = mm.alloc_page(pid)?;

        // Mark this entry as a leaf node (WRX as 0), and indicate
        // it is a valid page by setting "V".
        unsafe {
            l1_pt
                .add(vpn1)
                .write_volatile(((l0_pt_phys >> 12) << 10) | MMUFlags::VALID.bits());
            flush_mmu();
        }

        // Map the new physical page to the virtual page, so we can access it.
        map_page_inner(
            mm,
            pid,
            l0_pt_phys,
            l0_pt as usize,
            MemoryFlags::W | MemoryFlags::R,
            false,
        )?;

        // Zero-out the new page
        unsafe { memset(l0_pt as *mut u8, 0, PAGE_SIZE) };
    }

    // Ensure the entry hasn't already been mapped.
    if unsafe { l0_pt.add(vpn0).read_volatile() } & 1 != 0 {
        panic!("Page {:08x} already allocated!", virt);
    }
    unsafe {
        l0_pt.add(vpn0).write_volatile(
            (ppn1 << 20)
                | (ppn0 << 10)
                | (flags | MMUFlags::VALID | MMUFlags::D | MMUFlags::A).bits(),
        )
    };
    unsafe { flush_mmu() };

    Ok(())
}

/// Get the pagetable entry for a given address, or `Err()` if the address is invalid
pub fn pagetable_entry(addr: usize) -> Result<&'static mut usize, xous_kernel::Error> {
    if addr & 3 != 0 {
        return Err(xous_kernel::Error::BadAlignment);
    }
    let vpn1 = (addr >> 22) & ((1 << 10) - 1);
    let vpn0 = (addr >> 12) & ((1 << 10) - 1);
    assert!(vpn1 < 1024);
    assert!(vpn0 < 1024);

    let l1_pt = unsafe { &(*(PAGE_TABLE_ROOT_OFFSET as *mut RootPageTable)) };
    let l1_pte = l1_pt.entries[vpn1];
    if l1_pte & 1 == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    let l0_pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let entry = unsafe { &mut (*((l0_pt_virt + vpn0 * 4) as *mut usize)) };
    Ok(entry)
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

    // Ensure the entry hasn't already been mapped.
    if *entry & 1 == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    let phys = (*entry >> 10) << 12;
    *entry = 0;
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
    if *entry & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    let previous_entry = *entry;
    // Invalidate the old entry
    *entry = 0;
    unsafe { flush_mmu() };

    dest_space.activate()?;
    let phys = previous_entry >> 10 << 12;
    let flags = untranslate_flags(previous_entry);

    let result = map_page_inner(
        mm,
        dest_pid,
        phys,
        dest_addr as usize,
        flags,
        dest_pid.get() != 1,
    );

    // Switch back to the original address space and return
    src_space.activate().unwrap();
    result
}

/// Determine if a page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    let entry = if let Ok(val) = pagetable_entry(src_addr as usize) {
        val
    } else {
        return false;
    };
    *entry & MMUFlags::S.bits() != 0
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
    let phys = (*entry >> 10) << 12;

    // If we try to share a page that's not ours, that's just wrong.
    if *entry & MMUFlags::VALID.bits() == 0 {
        // klog!("Not valid");
        Err(xous_kernel::Error::ShareViolation)?;
    }

    // If we try to share a page that's already shared, that's a sharing
    // violation.
    if *entry & MMUFlags::S.bits() != 0 {
        // klog!("Already shared");
        Err(xous_kernel::Error::ShareViolation)?;
    }

    // Strip the `VALID` flag, and set the `SHARED` flag.
    *entry = (*entry & !MMUFlags::VALID.bits()) | MMUFlags::S.bits();

    // Ensure the change takes effect.
    unsafe { flush_mmu() };

    // Mark the page as Writable in new process space if it's writable here.
    let new_flags = if mutable && (*entry & MMUFlags::W.bits()) != 0 {
        MemoryFlags::R | MemoryFlags::W
    } else {
        MemoryFlags::R
    };

    // Switch to the new address space and map the page
    dest_space.activate()?;
    let result = map_page_inner(
        mm,
        dest_pid,
        phys,
        dest_addr as usize,
        new_flags,
        dest_pid.get() != 1,
    );
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
    let phys = (*src_entry >> 10) << 12;

    // If the page is not valid in this program, we can't return it.
    if *src_entry & MMUFlags::VALID.bits() == 0 {
        Err(xous_kernel::Error::ShareViolation)?;
    }

    // Mark the page as `Free`, which unmaps it.
    *src_entry = 0;
    unsafe { flush_mmu() };

    // Switch to the destination address space
    dest_space.activate()?;
    let dest_entry =
        pagetable_entry(dest_addr as usize).expect("page wasn't lent in destination space");

    // If the page wasn't marked as `Shared` in the destination address space,
    // treat that as an error.
    if *dest_entry & MMUFlags::S.bits() == 0 {
        panic!("page wasn't shared in destination space");
    }

    // Clear the `SHARED` and `PREVIOUSLY-WRITABLE` bits, and set the `VALID` bit.
    *dest_entry = *dest_entry & !(MMUFlags::S | MMUFlags::P).bits() | MMUFlags::VALID.bits();
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
    let ref mut l1_pt = l1_pt.entries;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0x0020_0004, so 4 must be added to the ppn1 value.
    let l0pt_virt = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;
    let ref mut l0_pt = unsafe { &mut (*(l0pt_virt as *mut LeafPageTable)) };

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

pub fn ensure_page_exists_inner(address: usize) -> Result<usize, xous_kernel::Error> {
    let virt = address & !0xfff;
    let entry = crate::arch::mem::pagetable_entry(virt).or(Err(xous_kernel::Error::BadAddress))?;
    // let entry = crate::arch::mem::pagetable_entry(virt).or_else(|e| {
    //     // klog!("Error in mem: {:?}", e);
    //     panic!("Page doesn't exist: {:08x}", address);
    //     Err(xous_kernel::Error::BadAddress)
    // })?;

    let flags = *entry & 0x1ff;

    if flags & MMUFlags::VALID.bits() != 0 {
        return Ok(address);
    }

    // If the flags are nonzero, but the "Valid" bit is not 1 and
    // the page isn't shared, then this is a reserved page. Allocate
    // a real page to back it and resume execution.
    if flags == 0 || flags & MMUFlags::S.bits() != 0 {
        Err(xous_kernel::Error::BadAddress)?;
    }

    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page(crate::arch::process::current_pid())
            .expect("Couldn't allocate new page")
    });
    let ppn1 = (new_page >> 22) & ((1 << 12) - 1);
    let ppn0 = (new_page >> 12) & ((1 << 10) - 1);
    unsafe {
        // Map the page to our process
        *entry = (ppn1 << 20)
            | (ppn0 << 10)
            | (flags | (1 << 0) /* valid */ | (1 << 6) /* D */ | (1 << 7)/* A */);
        flush_mmu();

        // Zero-out the page
        memset(virt as *mut u8, 0, PAGE_SIZE);

        // Move the page into userspace
        *entry = (ppn1 << 20)
            | (ppn0 << 10)
            | (flags | (1 << 0) /* valid */ | (1 << 4) /* USER */ | (1 << 6) /* D */ | (1 << 7)/* A */);
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
