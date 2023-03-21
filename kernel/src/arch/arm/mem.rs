// SPDX-FileCopyrightText: 2022 Foundation Devices <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::arch::arm::process::InitialProcess;
use crate::mem::MemoryManager;
use core::num::NonZeroU8;

pub use armv7::structures::paging::{
    InMemoryRegister, PageTable as L2PageTable, PageTableDescriptor, PageTableMemory,
    PageTableType, Readable, TranslationTable, TranslationTableDescriptor, TranslationTableMemory,
    TranslationTableType, Writeable, PAGE_TABLE_FLAGS, PAGE_TABLE_SIZE, SMALL_PAGE_FLAGS,
};
pub use armv7::{PhysicalAddress, VirtualAddress};

use xous_kernel::{MemoryFlags, PID};

pub const DEFAULT_HEAP_BASE: usize = 0x3000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_TABLE_OFFSET: usize = 0xfefe_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const THREAD_CONTEXT_AREA: usize = 0xff80_4000;
pub const USER_AREA_END: usize = 0xfe00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping {
    ttbr0: 0,
    pid: unsafe { PID::new_unchecked(1) },
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MemoryMapping {
    ttbr0: usize,
    pid: PID,
}

impl Default for MemoryMapping {
    fn default() -> Self {
        DEFAULT_MEMORY_MAPPING
    }
}

impl MemoryMapping {
    #[allow(dead_code)]
    pub unsafe fn from_raw(&mut self, _: usize) {
        unimplemented!("Use from_init_process instead");
    }

    pub unsafe fn from_init_process(&mut self, init: InitialProcess) {
        self.ttbr0 = init.ttbr0;
        self.pid = init.pid();
    }

    pub unsafe fn allocate(&mut self, _pid: PID) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    /// Get the currently active memory mapping.  Note that the actual root pages
    /// may be found at virtual address `PAGE_TABLE_ROOT_OFFSET`.
    pub fn current() -> MemoryMapping {
        let mut ttbr0;
        let mut pid: usize;

        unsafe {
            core::arch::asm!(
                "mrc p15, 0, {ttbr0}, c2, c0, 0",
                "mrc p15, 0, {pid}, c13, c0, 1",
                ttbr0 = out(reg) ttbr0,
                pid = out(reg) pid,
            )
        }

        assert_ne!(pid, 0, "Hardware PID is zero");

        MemoryMapping {
            ttbr0,
            pid: unsafe { NonZeroU8::new_unchecked((pid & 0xff) as u8) },
        }
    }

    pub fn get_pid(&self) -> PID {
        self.pid
    }

    pub fn is_kernel(&self) -> bool {
        self.pid.get() == 1
    }

    pub fn activate(self) -> Result<(), xous_kernel::Error> {
        klog!(
            "Activating current memory mapping. ttbr0: {:08x}, pid: {}",
            self.ttbr0,
            self.pid.get(),
        );
        let contextidr = ((self.pid.get() as usize) << 8) | self.pid.get() as usize;

        unsafe {
            // Set TTBR0 and CONTEXTIDR
            core::arch::asm!(
              "mcr p15, 0, {ttbr0}, c2, c0, 0",
              "mcr p15, 0, {contextidr}, c13, c0, 1",
              "isb",
              "dsb",
              ttbr0 = in(reg) self.ttbr0,
              contextidr = in(reg) contextidr,
            );

            flush_mmu();
        }

        Ok(())
    }

    fn _print_l2_pagetable(vpn1: usize, table_addr: usize) {
        let ptr: *mut PageTableMemory = table_addr as _;
        let mut l2_pt = unsafe { L2PageTable::new_from_ptr(ptr) };
        let l2_pt = unsafe { l2_pt.table_mut() };

        let mut no_valid_items = true;
        for (i, pt_desc) in l2_pt.iter().enumerate() {
            let virt_addr = (vpn1 << 20) | (i << 12);

            if let PageTableType::Invalid = pt_desc.get_type() {
                continue;
            }

            no_valid_items = false;

            let phys_addr = pt_desc.get_addr().expect("addr");

            match pt_desc.get_type() {
                PageTableType::LargePage => println!(
                    "        - {:02x} (64K) Large Page {:08x} -> {:08x}",
                    i, virt_addr, phys_addr
                ),
                PageTableType::SmallPage => println!(
                    "        - {:02x} (4K)  Small Page {:08x} -> {:08x}",
                    i, virt_addr, phys_addr
                ),
                _ => (),
            }
        }

        if no_valid_items {
            println!("        - <no valid items>");
        }
    }

    pub fn print_map(&self) {
        let tt_ptr = PAGE_TABLE_ROOT_OFFSET as *mut TranslationTableMemory;
        let tt = TranslationTable::new(tt_ptr);

        for (i, tt_desc) in tt.table().iter().enumerate() {
            if let TranslationTableType::Invalid = tt_desc.get_type() {
                continue;
            }

            let phys_addr = tt_desc.get_addr().expect("addr");
            match tt_desc.get_type() {
                TranslationTableType::Page => {
                    let virt_addr = i << 20;
                    let table_virt_addr = PAGE_TABLE_OFFSET + i * PAGE_SIZE;
                    println!(
                        "    - {:03x} (1MB) {:08x} L2 page table @ {:08x} (v. {:08x})",
                        i, virt_addr, phys_addr, table_virt_addr,
                    );
                    //Self::print_l2_pagetable(i, table_virt_addr);
                }
                TranslationTableType::Section => {
                    let virt_addr = i * (1024 * 1); // 1 MB
                    println!(
                        "    - {:03x} (1MB)  section {:08x} -> {:08x}",
                        i, virt_addr, phys_addr
                    );
                }
                TranslationTableType::Supersection => {
                    let virt_addr = i * (1024 * 16); // 16 MB
                    println!(
                        "    - {:03x} (16MB) supersection {:08x} -> {:08x}",
                        i, virt_addr, phys_addr
                    );
                }

                _ => (),
            }
        }
    }

    pub fn reserve_address(
        &mut self,
        mm: &mut MemoryManager,
        addr: usize,
        flags: MemoryFlags,
    ) -> Result<(), xous_kernel::Error> {
        let pid = crate::arch::current_pid();
        map_page_inner(
            mm,
            pid,
            0, // 0 means reserved
            addr,
            flags | MemoryFlags::RESERVE,
            false,
        )
    }
}

pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    let virt = virt as usize;
    let entry = crate::arch::mem::pagetable_entry(virt).or(Err(xous_kernel::Error::BadAddress))?;
    let current_entry: PageTableDescriptor = unsafe { entry.read_volatile() };

    let flags_u32 = current_entry.get_flags().expect("flags");
    let phys = (current_entry.as_u32() & !0xfff) as usize;

    klog!(
        "hand_page_to_user: phys={:08x}, entry: {:08x}",
        phys,
        current_entry.as_u32()
    );

    unsafe {
        // Move the page into userspace
        let mut small_page_flags = flags_u32;
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP::FullAccess);
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::NG::Enable); // Mark as non-global
        let new_entry = phys as u32 | small_page_flags;

        let new_entry = PageTableDescriptor::from_u32(new_entry);
        entry.write_volatile(new_entry);
        flush_mmu();
    };

    Ok(())
}

#[cfg(feature = "gdb-stub")]
pub fn peek_memory<T>(addr: *mut T) -> Result<T, xous_kernel::Error> {
    todo!();
}

#[cfg(feature = "gdb-stub")]
pub fn poke_memory<T>(addr: *mut T, val: T) -> Result<(), xous_kernel::Error> {
    todo!();
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
    klog!(
        "map_page_inner(): pid={} phys={:08x} virt={:08x}, flags: {:04x}",
        pid.get(),
        phys,
        virt,
        req_flags.bits()
    );

    let v = VirtualAddress::new(virt as u32);
    let vpn1 = v.translation_table_index();
    let vpn2 = v.page_table_index();

    let p = phys & !(0xfff);
    let ppn2 = (p >> 12) & 0xff;

    assert!(vpn1 < 4096);
    assert!(vpn2 < 256);
    assert!(ppn2 < 256);

    // The root (l1) pagetable is defined to be mapped into our virtual
    // address space at 0xff80_0000.
    let l1_pt_addr = PAGE_TABLE_ROOT_OFFSET;

    // Subsequent pagetables are defined as being mapped starting at
    // offset 0xfefe_0000.
    let l2_pt_addr = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;

    let existing_l1_entry = unsafe {
        ((l1_pt_addr as *mut u32).add(vpn1) as *mut TranslationTableDescriptor).read_volatile()
    };
    if existing_l1_entry.get_type() == TranslationTableType::Invalid {
        let l2_pt_phys = mm.alloc_page(pid)?;
        let phys = PhysicalAddress::from_ptr(l2_pt_phys as *const usize);
        let attributes = u32::from(PAGE_TABLE_FLAGS::VALID::Enable)
            | u32::from(PAGE_TABLE_FLAGS::DOMAIN.val(0xf));
        let descriptor =
            TranslationTableDescriptor::new(TranslationTableType::Page, phys, attributes)
                .expect("tt descriptor");

        unsafe {
            ((l1_pt_addr as *mut u32).add(vpn1)).write_volatile(descriptor.as_u32());
            flush_mmu();
        }

        // Map the new physical page to the virtual page, so we can access it.
        map_page_inner(
            mm,
            pid,
            l2_pt_phys,
            l2_pt_addr as usize,
            MemoryFlags::W | MemoryFlags::R,
            false,
        )?;

        // Zero-out the new page
        unsafe { memset(l2_pt_addr as *mut u8, 0, PAGE_SIZE) };
    }

    let existing_l2_entry =
        unsafe { ((l2_pt_addr as *mut u32).add(vpn2) as *mut PageTableDescriptor).read_volatile() };
    if existing_l2_entry.get_type() == PageTableType::SmallPage {
        let attrs = existing_l2_entry.as_u32();

        // Ensure the entry hasn't already been mapped to a different address.
        if attrs & u32::from(SMALL_PAGE_FLAGS::VALID::Enable) != 0 {
            // Return if we're reserving a page
            if phys == 0 {
                return Ok(());
            }

            panic!(
                "Page {:08x} already mapped to {:08x}!",
                virt,
                existing_l2_entry.as_u32() & !0xfff
            );
        }
    }

    // Map the L2 entry
    let mut small_page_flags = 0;

    // Mark entry as valid if it's not reserved or free
    let should_be_valid =
        (req_flags & MemoryFlags::RESERVE).is_empty() && (req_flags & MemoryFlags::FREE).is_empty();
    if should_be_valid {
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::VALID::Enable);
    }
    if !(req_flags & MemoryFlags::DEV).is_empty() {
        // small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP::FullAccess); // FIXME: do we need this?
    } else {
        // Device pages are always executable, otherwise, disable the execution if requested
        if (req_flags & MemoryFlags::X).is_empty() {
            // Avoid rising XN flag for invalid entries otherwise
            // it turns them into supersection entries which is not what we want
            if should_be_valid {
                small_page_flags |= u32::from(SMALL_PAGE_FLAGS::XN::Enable);
            }
        }
    }
    if map_user {
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP::FullAccess);
    }
    if (req_flags & MemoryFlags::W).is_empty() {
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP2::Enable); // Mark page read-only
    }

    let new_entry = PageTableDescriptor::new(
        PageTableType::SmallPage,
        PhysicalAddress::new(p as u32),
        small_page_flags,
    )
    .expect("new l2 entry");

    unsafe {
        ((l2_pt_addr as *mut u32).add(vpn2)).write_volatile(new_entry.as_u32());
        flush_mmu();
    }

    Ok(())
}

/// Get the pagetable entry for a given address, or `Err()` if the address is invalid
pub fn pagetable_entry(addr: usize) -> Result<*mut PageTableDescriptor, xous_kernel::Error> {
    if addr & 3 != 0 {
        return Err(xous_kernel::Error::BadAlignment);
    }

    let v = VirtualAddress::new(addr as u32);
    let vpn1 = v.translation_table_index();
    let vpn2 = v.page_table_index();
    assert!(vpn1 < 4096);
    assert!(vpn2 < 256);

    let l1_pt_addr = PAGE_TABLE_ROOT_OFFSET;
    let l2_pt_addr = PAGE_TABLE_OFFSET + vpn1 * PAGE_SIZE;

    let existing_l1_entry = unsafe {
        ((l1_pt_addr as *mut u32).add(vpn1) as *mut TranslationTableDescriptor).read_volatile()
    };
    if existing_l1_entry.get_type() == TranslationTableType::Invalid {
        return Err(xous_kernel::Error::BadAddress);
    }

    let existing_l2_entry_addr =
        unsafe { (l2_pt_addr as *mut u32).add(vpn2) as *mut PageTableDescriptor };
    return Ok(existing_l2_entry_addr);
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
pub fn unmap_page_inner(
    _mm: &mut MemoryManager,
    _virt: usize,
) -> Result<usize, xous_kernel::Error> {
    todo!();
}

/// Move a page from one address space to another.
pub fn move_page_inner(
    _mm: &mut MemoryManager,
    _src_space: &MemoryMapping,
    _src_addr: *mut u8,
    _dest_pid: PID,
    _dest_space: &MemoryMapping,
    _dest_addr: *mut u8,
) -> Result<(), xous_kernel::Error> {
    todo!();
}

/// Determine if a page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    pagetable_entry(src_addr as usize).map_or(false, |v| {
        let entry = unsafe { v.read_volatile() };
        let flags_u32 = entry.get_flags().expect("flags");
        let flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> =
            InMemoryRegister::new(flags_u32);
        let tex_bits = flags.read(SMALL_PAGE_FLAGS::TEX);
        get_s_flag_from_tex_bits(tex_bits)
    })
}

pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
    mutable: bool,
) -> Result<usize, xous_kernel::Error> {
    klog!(
        "***lend - src: {:08x} dest: {:08x}***",
        src_addr as u32,
        dest_addr as u32
    );
    let entry = pagetable_entry(src_addr as usize)?;
    let current_entry = unsafe { entry.read_volatile() };
    let flags_u32 = current_entry.get_flags().expect("flags");
    let flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> = InMemoryRegister::new(flags_u32);
    let is_valid = flags.read(SMALL_PAGE_FLAGS::VALID) != 0;
    let is_writable = flags.read(SMALL_PAGE_FLAGS::AP2) == 0;
    let phys = (current_entry.as_u32() & !0xfff) as usize;

    // If we try to share a page that's not ours, that's just wrong.
    if !is_valid {
        klog!("Not valid");
        return Err(xous_kernel::Error::ShareViolation);
    }

    let tex_bits = flags.read(SMALL_PAGE_FLAGS::TEX);
    let is_shared = get_s_flag_from_tex_bits(tex_bits);

    // If we try to share a page that's already shared, that's a sharing
    // violation.
    if is_shared {
        klog!("Already shared");
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the `VALID` flag, and set the `SHARED` flag.
    let mut small_page_flags = flags_u32;
    small_page_flags &= !u32::from(SMALL_PAGE_FLAGS::VALID::Enable);
    small_page_flags =
        (small_page_flags & !(0b111 << 6)) | (apply_s_flag_to_tex_bits(tex_bits, true) << 6);

    let new_entry = phys as u32 | small_page_flags;
    let new_entry = PageTableDescriptor::from_u32(new_entry);
    unsafe { entry.write_volatile(new_entry) };

    // Ensure the change takes effect.
    unsafe { flush_mmu() };

    // Mark the page as Writable in new process space if it's writable here.
    let new_flags = if mutable && is_writable {
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
    src_space.activate()?;

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
    klog!(
        "***return - src: {:08x} dest: {:08x}***",
        src_addr as u32,
        dest_addr as u32
    );
    let src_entry_ptr = pagetable_entry(src_addr as usize)?;
    let src_entry = unsafe { src_entry_ptr.read_volatile() };
    let src_flags_u32 = src_entry.get_flags().expect("flags");
    let src_flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> =
        InMemoryRegister::new(src_flags_u32);
    let is_src_valid = src_flags.read(SMALL_PAGE_FLAGS::VALID) != 0;
    let src_phys = (src_entry.as_u32() & !0xfff) as usize;

    // If the page is not valid in this address space, we can't return it.
    if !is_src_valid {
        klog!("Not valid");
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Mark the page as `Free`, which unmaps it.
    unsafe {
        (src_entry_ptr as *mut usize).write_volatile(0);
        flush_mmu();
    }

    // Switch to the destination address space
    dest_space.activate()?;

    let dest_entry_ptr = pagetable_entry(dest_addr as usize)?;
    let dest_entry = unsafe { dest_entry_ptr.read_volatile() };
    let dest_flags_u32 = dest_entry.get_flags().expect("flags");
    let dest_flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> =
        InMemoryRegister::new(dest_flags_u32);
    let dest_phys = (dest_entry.as_u32() & !0xfff) as usize;

    let tex_bits = dest_flags.read(SMALL_PAGE_FLAGS::TEX);
    let is_shared = get_s_flag_from_tex_bits(tex_bits);

    // If the page wasn't marked as `Shared` in the destination address space,
    // treat that as an error.
    if !is_shared {
        panic!("page wasn't shared in destination space");
    }

    // Set the `VALID` bit.
    let mut small_page_flags = dest_flags_u32;
    small_page_flags |= u32::from(SMALL_PAGE_FLAGS::VALID::Enable);

    // Clear the `SHARED` and `PREVIOUSLY-WRITABLE` bits
    let tex_bits = apply_s_flag_to_tex_bits(tex_bits, false);
    let tex_bits = apply_p_flag_to_tex_bits(tex_bits, false);
    small_page_flags = (small_page_flags & !(0b111 << 6)) | (tex_bits << 6);

    let new_entry = dest_phys as u32 | small_page_flags;
    let new_entry = PageTableDescriptor::from_u32(new_entry);
    unsafe {
        dest_entry_ptr.write_volatile(new_entry);
        flush_mmu();
    };

    // Swap back to our previous address space
    src_space.activate()?;
    Ok(src_phys)
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous_kernel::Error> {
    let virt = virt & !0xfff;
    let entry = crate::arch::mem::pagetable_entry(virt).or(Err(xous_kernel::Error::BadAddress))?;
    let current_entry: PageTableDescriptor = unsafe { entry.read_volatile() };

    let flags_u32 = current_entry.get_flags().expect("flags");
    let flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> = InMemoryRegister::new(flags_u32);
    let is_valid = flags.read(SMALL_PAGE_FLAGS::VALID) != 0;
    let phys = (current_entry.as_u32() & !0xfff) as usize;
    if is_valid {
        return Ok(phys);
    }

    let is_shared = get_s_flag_from_tex_bits(flags.read(SMALL_PAGE_FLAGS::TEX));

    // If the flags are nonzero, but the "Valid" bit is not 1 and
    // the page isn't shared, then this is a reserved page. Allocate
    // a real page to back it and resume execution.
    if is_shared {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // The memory has been reserved, but isn't pointing anywhere yet.
    if current_entry.as_u32() != 0 {
        return Err(xous_kernel::Error::MemoryInUse);
    }

    Err(xous_kernel::Error::BadAddress)
}

fn get_s_flag_from_tex_bits(tex_bits: u32) -> bool {
    // Reuse TEX[1] bit as an S flag since TRE is enabled and TEX[2:1] bits are customizable
    tex_bits & 0b010 != 0
}

fn apply_s_flag_to_tex_bits(tex_bits: u32, s: bool) -> u32 {
    // Reuse TEX[1] bit as an S flag since TRE is enabled and TEX[2:1] bits are customizable
    if s {
        tex_bits | 0b010
    } else {
        tex_bits & !0b010
    }
}

fn apply_p_flag_to_tex_bits(tex_bits: u32, p: bool) -> u32 {
    // Reuse TEX[2] bit as a P flag since TRE is enabled and TEX[2:1] bits are customizable
    if p {
        tex_bits | 0b100
    } else {
        tex_bits & !0b100
    }
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
    let current_entry: PageTableDescriptor = unsafe { entry.read_volatile() };

    let flags_u32 = current_entry.get_flags().expect("flags");
    let flags: InMemoryRegister<u32, SMALL_PAGE_FLAGS::Register> = InMemoryRegister::new(flags_u32);
    let is_valid = flags.read(SMALL_PAGE_FLAGS::VALID) != 0;
    if is_valid {
        return Ok(address);
    }

    let is_shared = get_s_flag_from_tex_bits(flags.read(SMALL_PAGE_FLAGS::TEX));

    // If the flags are nonzero, but the "Valid" bit is not 1 and
    // the page isn't shared, then this is a reserved page. Allocate
    // a real page to back it and resume execution.
    if is_shared {
        return Err(xous_kernel::Error::BadAddress);
    }

    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page(crate::arch::process::current_pid())
            .expect("Couldn't allocate new page")
    });

    let mut small_page_flags = flags_u32;
    small_page_flags |= u32::from(SMALL_PAGE_FLAGS::VALID::Enable);

    let new_entry = new_page as u32 | small_page_flags;
    let new_entry = PageTableDescriptor::from_u32(new_entry);

    unsafe {
        // Map the page to our process
        entry.write_volatile(new_entry);
        flush_mmu();

        // Zero-out the page
        memset(virt as *mut u8, 0, PAGE_SIZE);

        // Move the page into userspace
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::AP::FullAccess);
        small_page_flags |= u32::from(SMALL_PAGE_FLAGS::NG::Enable); // Mark as non-global
        let new_entry = new_page as u32 | small_page_flags;
        let new_entry = PageTableDescriptor::from_u32(new_entry);

        entry.write_volatile(new_entry);
        flush_mmu();
    };

    Ok(new_page)
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    if let Err(e) = virt_to_phys(virt) {
        // If the value is a `BadAddress`, then that means that address is not valid
        // and is therefore available
        let is_available = e == xous_kernel::Error::BadAddress;
        is_available
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
pub fn page_flags(_virt: usize) -> Option<MemoryFlags> {
    todo!();
}

pub fn update_page_flags(_virt: usize, _flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
    todo!();
}

extern "C" {
    fn flush_mmu();
}

pub unsafe fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *s.add(i) = c as u8;
        i += 1;
    }
    s
}
