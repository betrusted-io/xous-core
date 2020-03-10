use crate::args::KernelArguments;
use crate::processtable::SystemServicesHandle;
use core::fmt;
use core::mem;
use core::slice;
use core::str;

pub use crate::arch::mem::{MemoryMapping, PAGE_SIZE};
use xous::{MemoryAddress, MemoryFlags, PID};

#[derive(Debug)]
enum ClaimOrRelease {
    Claim,
    Release,
}

#[repr(C)]
pub struct MemoryRangeExtra {
    mem_start: u32,
    mem_size: u32,
    mem_tag: u32,
    _padding: u32,
}

impl fmt::Display for MemoryRangeExtra {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag_name_bytes = self.mem_tag.to_le_bytes();
        let s = unsafe {
            // First, we build a &[u8]...
            let slice = slice::from_raw_parts(tag_name_bytes.as_ptr(), 4);
            // ... and then convert that slice into a string slice
            str::from_utf8_unchecked(slice)
        };

        write!(
            f,
            "{} ({:08x}) {:08x} - {:08x} {} bytes",
            s,
            self.mem_tag,
            self.mem_start,
            self.mem_start + self.mem_size,
            self.mem_size
        )
    }
}

pub struct MemoryManager {
    allocations: &'static mut [PID],
    extra: &'static [MemoryRangeExtra],
    ram_start: usize,
    ram_size: usize,
    ram_name: u32,
    last_address: usize,
}

static mut MEMORY_MANAGER: MemoryManager = MemoryManager {
    allocations: &mut [],
    extra: &[],
    ram_start: 0,
    ram_size: 0,
    ram_name: 0,
    last_address: 0,
};

/// How many people have checked out the handle object.
/// This should be replaced by an AtomicUsize when we get
/// multicore support.
/// For now, we can get away with this since the memory manager
/// should only be accessed in an IRQ context.
static mut MM_HANDLE_COUNT: usize = 0;

pub struct MemoryManagerHandle<'a> {
    manager: &'a mut MemoryManager,
}

/// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
/// to the Memory Manager should only be made during interrupt contexts.
impl<'a> MemoryManagerHandle<'a> {
    /// Get the singleton memory manager.
    pub fn get() -> MemoryManagerHandle<'a> {
        let count = unsafe {
            MM_HANDLE_COUNT += 1;
            MM_HANDLE_COUNT - 1
        };
        if count != 0 {
            panic!("Multiple users of MemoryManagerHandle!");
        }
        MemoryManagerHandle {
            manager: unsafe { &mut MEMORY_MANAGER },
        }
    }
}

impl Drop for MemoryManagerHandle<'_> {
    fn drop(&mut self) {
        unsafe { MM_HANDLE_COUNT -= 1 };
    }
}

use core::ops::{Deref, DerefMut};
impl Deref for MemoryManagerHandle<'_> {
    type Target = MemoryManager;
    fn deref(&self) -> &MemoryManager {
        &*self.manager
    }
}
impl DerefMut for MemoryManagerHandle<'_> {
    fn deref_mut(&mut self) -> &mut MemoryManager {
        &mut *self.manager
    }
}

/// Initialize the memory map.
/// This will go through memory and map anything that the kernel is
/// using to process 1, then allocate a pagetable for this process
/// and place it at the usual offset.  The MMU will not be enabled yet,
/// as the process entry has not yet been created.
impl MemoryManager {
    pub fn init(&mut self, base: *mut u32, args: &KernelArguments) -> Result<(), xous::Error> {
        let mut args_iter = args.iter();
        let xarg_def = args_iter.next().expect("mm: no kernel arguments found");
        assert!(
            self.extra.len() == 0,
            "mm: self.extra.len() was {}, not 0",
            self.extra.len()
        );
        assert!(
            xarg_def.name == make_type!("XArg"),
            "mm: first tag wasn't XArg"
        );
        assert!(xarg_def.data[1] == 1, "mm: XArg had unexpected version");
        self.ram_start = xarg_def.data[2] as usize;
        self.ram_size = xarg_def.data[3] as usize;
        self.ram_name = xarg_def.data[4];

        let mut mem_size = self.ram_size / PAGE_SIZE;
        for tag in args_iter {
            if tag.name == make_type!("MREx") {
                assert!(
                    self.extra.len() == 0,
                    "mm: MREx tag appears twice!  self.extra.len() is {}, not 0",
                    self.extra.len()
                );
                let ptr = tag.data.as_ptr() as *mut MemoryRangeExtra;
                self.extra = unsafe {
                    slice::from_raw_parts_mut(
                        ptr,
                        tag.data.len() * 4 / mem::size_of::<MemoryRangeExtra>(),
                    )
                };
            }
        }

        for range in self.extra.iter() {
            mem_size += range.mem_size as usize / PAGE_SIZE;
        }

        self.allocations = unsafe { slice::from_raw_parts_mut(base as *mut PID, mem_size) };
        Ok(())
    }

    pub fn print_ownership(&self) {
        println!("Ownership ({} bytes in all):", self.allocations.len());

        let mut offset = 0;
        unsafe {
            // First, we build a &[u8]...
            let name_bytes = self.ram_name.to_le_bytes();
            // ... and then convert that slice into a string slice
            let ram_name = str::from_utf8_unchecked(&name_bytes);
            println!(
                "    Region {} ({:08x}) {:08x} - {:08x} {} bytes:",
                ram_name,
                self.ram_name,
                self.ram_start,
                self.ram_start + self.ram_size,
                self.ram_size
            );
        };
        for o in 0..self.ram_size / PAGE_SIZE {
            if self.allocations[offset + o] != 0 {
                println!(
                    "        {:08x} => {}",
                    self.ram_size + o * PAGE_SIZE,
                    self.allocations[o]
                );
            }
        }

        offset += self.ram_size / PAGE_SIZE;

        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        for region in self.extra {
            println!("    Region {}:", region);
            for o in 0..(region.mem_size as usize) / PAGE_SIZE {
                if self.allocations[offset + o] != 0 {
                    println!(
                        "        {:08x} => {}",
                        (region.mem_start as usize) + o * PAGE_SIZE,
                        self.allocations[offset + o]
                    )
                }
            }
            offset += region.mem_size as usize / PAGE_SIZE;
        }
    }

    /// Allocate a single page to the given process.
    /// Ensures the page is zeroed out prior to handing it over to
    /// the specified process.
    pub fn alloc_page(&mut self, pid: PID) -> Result<usize, xous::Error> {
        // Go through all RAM pages looking for a free page.
        // Optimization: start from the previous address.
        // println!("Allocating page for PID {}", pid);
        for index in self.last_address..((self.ram_size as usize) / PAGE_SIZE) {
            // println!("    Checking {:08x}...", index * PAGE_SIZE + self.ram_start as usize);
            if self.allocations[index] == 0 {
                self.allocations[index] = pid;
                self.last_address = index + 1;
                return Ok(index * PAGE_SIZE + self.ram_start);
            }
        }
        for index in 0..self.last_address {
            // println!("    Checking {:08x}...", index * PAGE_SIZE + self.ram_start as usize);
            if self.allocations[index] == 0 {
                self.last_address = index;
                self.allocations[index] = pid;
                return Ok(index * PAGE_SIZE + self.ram_start);
            }
        }
        Err(xous::Error::OutOfMemory)
    }

    /// Reserve the given range without actually allocating memory.
    /// That way we can overpromise on stack size and heap size without
    /// needing to actually have pages to back it.
    pub fn reserve_range(
        &mut self,
        virt_ptr: *mut usize,
        size: usize,
        flags: MemoryFlags,
    ) -> Result<xous::Result, xous::Error> {
        let virt = virt_ptr as usize;

        if virt & 0xfff != 0 {
            return Err(xous::Error::BadAlignment);
        }

        if size & 0xfff != 0 {
            return Err(xous::Error::BadAlignment);
        }

        let mut mm = MemoryMapping::current();
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            mm.reserve_address(self, virt, flags)?;
        }
        Ok(xous::Result::MemoryRange(virt_ptr as *mut u8, size))
    }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn map_range(
        &mut self,
        phys_ptr: *mut usize,
        virt_ptr: *mut usize,
        size: usize,
        flags: MemoryFlags,
    ) -> Result<MemoryAddress, xous::Error> {
        let ss = SystemServicesHandle::get();
        let pid = ss.current_pid();
        let phys = phys_ptr as usize;
        let virt = virt_ptr as usize;

        if phys == 0 || virt == 0 {
            println!("Attempted to map a range without specifying phys or virt");
            return Err(xous::Error::BadAddress);
        }

        let mut error = None;
        for phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
            if let Err(err) = self.claim_page(phys as *mut usize, pid) {
                error = Some(err);
                break;
            }
        }
        if let Some(err) = error {
            for phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
                self.release_page(phys as *mut usize, pid).ok();
            }
            return Err(err);
        }

        for phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
            if let Err(e) =
                crate::arch::mem::map_page_inner(self, pid, phys as usize, virt as usize, flags)
            {
                error = Some(e);
                break;
            }
        }

        if let Some(err) = error {
            for phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
                self.release_page(phys as *mut usize, pid).ok();
            }
            return Err(err);
        }

        Ok(MemoryAddress::new(virt).unwrap())
    }

    // /// Map a range of physical addresses into the current memory space.
    // pub fn map_range(&mut self, phys: *mut usize, virt: *mut usize, size: usize, flags: MemoryFlags) -> xous::Result {
    //     // If a physical address range was requested, verify that it is valid
    //     if phys as usize != 0 {

    //     }
    //     let virt = if virt as usize == 0 {
    //         // No virt address was
    //     }
    //     for offset in (0..size).step_by(4096) {
    //         if let xous::Result::Error(e) = crate::arch::mem::
    //             map_page_inner(
    //                 ((phys as usize) + offset) as usize,
    //                 ((virt as usize) + offset) as usize,
    //                 req_flags,
    //             )
    //             .map(|x| xous::Result::MemoryAddress(x.get() as *mut usize))
    //             .unwrap_or_else(|e| xous::Result::Error(e))
    //         {
    //             result = xous::Result::Error(e);
    //             break;
    //         }
    //         last_mapped = offset;
    //     }
    //     if result != xous::Result::Ok {
    //         for offset in (0..last_mapped).step_by(4096) {
    //             mm.unmap_page(
    //                 ((phys as usize) + offset) as *mut usize,
    //                 ((virt as usize) + offset) as *mut usize,
    //                 req_flags,
    //             )
    //             .expect("couldn't unmap page");
    //         }
    //     }
    //     result
    // }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn unmap_page(&mut self, virt: *mut usize) -> Result<(), xous::Error> {
        let pid = crate::arch::current_pid();
        let phys = crate::arch::mem::virt_to_phys(virt as usize)?;
        self.release_page(phys as *mut usize, pid)?;
        crate::arch::mem::unmap_page_inner(self, virt as usize)
    }

    fn claim_or_release(
        &mut self,
        addr: *mut usize,
        pid: PID,
        action: ClaimOrRelease,
    ) -> Result<(), xous::Error> {
        fn action_inner(
            addr: &mut PID,
            pid: PID,
            action: ClaimOrRelease,
        ) -> Result<(), xous::Error> {
            if *addr != 0 && *addr != pid {
                return Err(xous::Error::MemoryInUse);
            }
            match action {
                ClaimOrRelease::Claim => {
                    *addr = pid;
                }
                ClaimOrRelease::Release => {
                    *addr = 0;
                }
            }
            Ok(())
        }
        let addr = addr as usize;

        // Ensure the address lies on a page boundary
        if addr & 0xfff != 0 {
            return Err(xous::Error::BadAlignment);
        }

        let mut offset = 0;
        // Happy path: The address is in main RAM
        if addr >= self.ram_start && addr < self.ram_start + self.ram_size {
            offset += (addr - self.ram_start) / PAGE_SIZE;
            return action_inner(&mut self.allocations[offset], pid, action);
        }

        offset += self.ram_size / PAGE_SIZE;
        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        for region in self.extra {
            if addr >= (region.mem_start as usize)
                && addr < (region.mem_start + region.mem_size) as usize
            {
                offset += (addr - (region.mem_start as usize)) / PAGE_SIZE;
                return action_inner(&mut self.allocations[offset], pid, action);
            }
            offset += region.mem_size as usize / PAGE_SIZE;
        }
        println!(
            "mem: unable to claim or release physical address {:08x}",
            addr
        );
        Err(xous::Error::BadAddress)
    }

    /// Mark a given address as being owned by the specified process ID
    fn claim_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous::Error> {
        self.claim_or_release(addr, pid, ClaimOrRelease::Claim)
    }

    /// Mark a given address as no longer being owned by the specified process ID
    fn release_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous::Error> {
        self.claim_or_release(addr, pid, ClaimOrRelease::Release)
    }
}
