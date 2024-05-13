// SPDX-FileCopyrightText: 2024 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: Apache-2.0

use loader::swap::SWAP_FLG_WIRED;
use loader::swap::SWAP_RPT_VADDR;
use xous_kernel::SWAPPER_PID;
use xous_kernel::{SysCallResult, PID, SID, TID};

use crate::arch::current_pid;
use crate::arch::mem::MMUFlags;
use crate::arch::mem::PAGE_SIZE;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// Initial number of pages to free up in case of an OOM is tripped inside the kernel. This is different
/// from the number used by the userspace OOM trigger -- this one is hard-coded into the kernel.
const KERNEL_OOM_DOOM_PAGES_TO_FREE: usize = 22;

/// Initial threshold for triggering the OOM doom handler
const KERNEL_OOM_DOOM_THRESH_PAGES: usize = 10;

/// userspace swapper -> kernel ABI (see kernel/src/syscall.rs)
/// This ABI is copy-paste synchronized with what's in the userspace handler. It's left out of
/// xous-rs so that we can change it without having to push crates to crates.io.
/// Since there is only one place the ABI could be used, we're going to stick with
/// this primitive method of synchronization because it reduces the activation barrier
/// to fix bugs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwapAbi {
    Invalid = 0,
    Evict = 1,
    GetFreePages = 2,
    FetchAllocs = 3,
    SetOomThresh = 4,
}
/// SYNC WITH `xous-swapper/src/main.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreePages,
            3 => FetchAllocs,
            4 => SetOomThresh,
            _ => Invalid,
        }
    }
}

/// kernel -> swapper handler ABI
/// This is the response ABI from the kernel into the swapper. It also tracks intermediate state
/// necessary for proper return from these calls.
#[derive(Debug, Copy, Clone)]
pub enum BlockingSwapOp {
    /// TID, `sepc` of swapper; PID of source, vaddr of source, vaddr in swap space (block must already be
    /// mapped into swap space)
    WriteToSwap(TID, usize, PID, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block
    ReadFromSwap(PID, TID, usize, usize, usize),
    /// Argument is the original thread ID of swapper and `sepc` for return
    FetchAllocs(TID, usize),
    /// OOM is nigh warning to swapper. Argument is TID, PID of the thing we were running that triggered
    /// OomDoom.
    OomDoom(TID, PID),
}

/// This structure is a copy of what's defined in the loader's swap module. The reason
/// we can't condense the two APIs is because PID is actually defined differently in the
/// kernel than in the loader: a PID for the loader is a `u8`. A PID for the kernel is a
/// NonZeroU8. Thus the APIs are not compatible, and we have to maintain two sets of
/// calls.
///
/// Also, critically, `SwapAlloc` in the kernel needs an `update` and `reparent` call,
/// which increments `next_epoch` -- these can't be migrated into the swapper, and Rust
/// does not allow extensions to types that aren't native to your crate. So, we're left
/// with redundant copies of the structure definition.
#[derive(Debug)]
pub struct SwapAlloc {
    timestamp: u32,
    /// virtual_page_number[19:0] | flags[3:0] | pid[7:0]
    vpn: u32,
}

impl SwapAlloc {
    pub fn is_pid(&self, pid: PID) -> bool { self.vpn as u8 == pid.get() }

    #[allow(dead_code)]
    pub fn is_some(&self) -> bool { self.vpn as u8 != 0 }

    pub fn is_none(&self) -> bool { self.vpn as u8 == 0 }

    pub fn get_pid(&self) -> Option<PID> {
        if self.vpn as u8 == 0 { None } else { Some(PID::new(self.vpn as u8).unwrap()) }
    }

    pub unsafe fn update(&mut self, pid: Option<PID>, vaddr: Option<usize>) {
        crate::swap::Swap::with_mut(|s| {
            self.timestamp = s.next_epoch();
            if let Some(pid) = pid {
                s.track_alloc(true);
                if let Some(va) = vaddr {
                    self.vpn = (va as u32) & !0xFFF | pid.get() as u32;
                } else {
                    self.vpn = SWAP_FLG_WIRED | pid.get() as u32;
                }
            } else {
                s.track_alloc(false);
                self.vpn = 0;
            }
        });
    }

    pub unsafe fn reparent(&mut self, pid: PID) {
        self.timestamp = crate::swap::Swap::with_mut(|s| s.next_epoch());
        self.vpn = self.vpn & !&0xFFu32 | pid.get() as u32;
    }
}

#[cfg(baremetal)]
#[no_mangle]
static mut SWAP: Swap = Swap {
    sid: SID::from_u32(0, 0, 0, 0),
    pc: 0,
    prev_op: None,
    nested_op: None,
    swapper_state: 0,
    swapper_args: [0usize; 8],
    epoch: 0,
    free_pages: 0,
    mem_alloc_tracker_paddr: 0,
    mem_alloc_tracker_pages: 0,
    oom_doom_thresh_pages: KERNEL_OOM_DOOM_THRESH_PAGES,
    oom_pages_to_free: KERNEL_OOM_DOOM_PAGES_TO_FREE,
};

pub struct Swap {
    /// SID for the swapper
    sid: SID,
    /// PC for blocking handler
    pc: usize,
    /// previous op
    prev_op: Option<BlockingSwapOp>,
    /// previous previous op - I think that structurally, we can only get a nesting of depth = 2, and this
    /// is due to things like an alloc advisory pulling in a new superpage while handling the advisory...?
    /// Anyways, there is an assert that checks for the overflow condition of nesting.
    nested_op: Option<BlockingSwapOp>,
    /// Userspace state pointer for the swapper. This is a PID-2 local virtual address, passed from the
    /// swapper on registration
    swapper_state: usize,
    /// storage for args
    swapper_args: [usize; 8],
    /// swap epoch tracker
    epoch: u32,
    /// approximate free memory, in pages
    free_pages: usize,
    /// physical base of memory allocation tracker
    mem_alloc_tracker_paddr: usize,
    /// number of pages to map when handling the tracker
    mem_alloc_tracker_pages: usize,
    /// Imminent OOM threshold, in pages
    oom_doom_thresh_pages: usize,
    /// Number of pages to free on an OOM doom handler invocation
    oom_pages_to_free: usize,
}
impl Swap {
    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut *core::ptr::addr_of_mut!(SWAP))
        }

        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&mut ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&*core::ptr::addr_of!(SWAP))
        }

        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&ss.borrow_mut()))
    }

    pub fn next_epoch(&mut self) -> u32 {
        if self.epoch < u32::MAX {
            self.epoch += 1;
            self.epoch
        } else {
            // Epoch rollover handling -- two proposals:
            //   - Fast epoch rollover, but long-lasting performance impact: just reset all counters to 0, and
            //     let the system re-discover LRU order based on usage patterns again
            //   - Slow epoch rollover, with no performance impact: go through all pages and "compact" the
            //     count down to the lowest level, resetting the epoch counter to the next available epoch.
            //     LRU patterns are maintained, but the search could take a long time.
            todo!("Handle swap epoch rollover");
        }
    }

    pub fn track_alloc(&mut self, is_alloc: bool) {
        if is_alloc {
            self.free_pages = self.free_pages.saturating_add(1);
        } else {
            self.free_pages = self.free_pages.saturating_sub(1);
        }
    }

    pub fn set_oom_thresh(&mut self, thresh_pages: usize, pages_to_free: usize) -> SysCallResult {
        self.oom_doom_thresh_pages = thresh_pages;
        self.oom_pages_to_free = pages_to_free;
        Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
    }

    pub fn register_handler(
        &mut self,
        s0: u32,
        s1: u32,
        s2: u32,
        s3: u32,
        handler: usize,
        state: usize,
    ) -> Result<xous_kernel::Result, xous_kernel::Error> {
        // initialize free_pages with a comprehensive count of RAM usage
        let mut total_bytes = 0;
        crate::services::SystemServices::with(|system_services| {
            crate::mem::MemoryManager::with(|mm| {
                for process in &system_services.processes {
                    if !process.free() {
                        let bytes_used = mm.ram_used_by(process.pid);
                        total_bytes += bytes_used;
                    }
                }
            });
        });
        self.free_pages = total_bytes / PAGE_SIZE;

        // now register the swapper
        if self.sid == SID::from_u32(0, 0, 0, 0) {
            self.sid = SID::from_u32(s0, s1, s2, s3);
            self.pc = handler;
            self.swapper_state = state;
            #[cfg(feature = "debug-swap")]
            println!(
                "handler registered: sid {:?} pc {:?} state {:?}",
                self.sid, self.pc, self.swapper_state
            );
            Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
        } else {
            // someone is trying to steal the swapper's privileges!
            #[cfg(feature = "debug-swap")]
            println!("Handler double-register detected!");
            Err(xous_kernel::Error::AccessDenied)
        }
    }

    pub fn init_rpt(&mut self, base: usize, pages: usize) {
        self.mem_alloc_tracker_paddr = crate::arch::mem::virt_to_phys(base).unwrap() as usize;
        self.mem_alloc_tracker_pages = pages;
    }

    /// This is a non-divergent syscall (handled entirely within the kernel)
    pub fn get_free_mem(&self) -> SysCallResult {
        // TODO: remove this block once we have confidence that self.free_pages aligns with total_bytes
        // reported
        {
            println!("RAM usage:");
            let mut total_bytes = 0;
            crate::services::SystemServices::with(|system_services| {
                crate::mem::MemoryManager::with(|mm| {
                    for process in &system_services.processes {
                        if !process.free() {
                            let bytes_used = mm.ram_used_by(process.pid);
                            total_bytes += bytes_used;
                            println!(
                                "    PID {:>3}: {:>4} k {}",
                                process.pid,
                                bytes_used / 1024,
                                system_services.process_name(process.pid).unwrap_or("")
                            );
                        }
                    }
                });
            });
            println!("Via count: {} k total", total_bytes / 1024);
            println!("Via tracked alloc: {} k total", self.free_pages * PAGE_SIZE / 1024);
        }

        Ok(xous_kernel::Result::Scalar5(self.free_pages, 0, 0, 0, 0))
    }

    /// This call diverges into the swapper to inform it of imminent OOM DOOM. Otherwise it returns normally.
    ///
    /// Figuring out where to insert this call is a bit tricky, because it diverges and after the call
    /// we'd have to resume execution. Normally, the swapper should poll memory levels and prevent this
    /// from ever being called, but we need this fallback in the case that we have a single process
    /// that just suddenly decides to allocate all of free memory in a single go.
    ///
    /// TODO: figure out where to insert this.
    pub fn oom_doom(&mut self) -> ! {
        let original_pid = crate::arch::process::current_pid();
        let original_tid = crate::arch::process::current_tid();

        SystemServices::with(|system_services| {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            // swap to the swapper space
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate().expect("couldn't activate swapper memory space");
        });

        #[cfg(feature = "debug-swap")]
        println!("oom_doom - userspace activate");
        // this is safe because we're now in the swapper memory context, thanks to the previous call
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::OomDoom(original_tid, original_pid));
        }
    }

    /// This call diverges into the userspace swapper.
    pub fn fetch_allocs(&mut self) -> ! {
        let (tid, sepc) = crate::arch::process::Process::with_current(|p| {
            let thread = p.current_thread();
            let tid = p.current_tid();
            (tid, thread.sepc)
        });

        SystemServices::with(|system_services| {
            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            // swap to the swapper space
            let swapper_map = system_services.get_process(swapper_pid).unwrap().mapping;
            swapper_map.activate().unwrap();

            // map the RPT into userspace
            // note that this technically makes the RPT shared with kernel and userspace, but,
            // the userspace version is read-only, and the next section runs in an interrupt context,
            // so I think it's safe for it to be shared for that duration.
            MemoryManager::with_mut(|mm| {
                for page in 0..self.mem_alloc_tracker_pages {
                    crate::arch::mem::map_page_inner(
                        mm,
                        swapper_pid,
                        self.mem_alloc_tracker_paddr + page * PAGE_SIZE,
                        SWAP_RPT_VADDR + page * PAGE_SIZE,
                        xous_kernel::MemoryFlags::R,
                        true,
                    )
                    .ok();
                    unsafe { crate::arch::mem::flush_mmu() };
                }
            });
        });

        #[cfg(feature = "debug-swap")]
        println!("fetch_allocs - userspace activate");
        // this is safe because we entered the swapper memory context above
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::FetchAllocs(tid, sepc));
        }
    }

    /// This call normally diverges into the userspace swapper. It will return a SysCallResult
    /// if the requested pages fail sanity checks.
    pub fn evict_page(&mut self, target_pid: usize, vaddr: usize) -> SysCallResult {
        let (tid, sepc) = crate::arch::process::Process::with_current(|p| {
            let thread = p.current_thread();
            let tid = p.current_tid();
            (tid, thread.sepc)
        });
        // This call can fail for legitimate reasons: for example, the address given was already swapped, or
        // invalid.
        let evicted_ptr =
            match crate::arch::mem::evict_page_inner(PID::new(target_pid as u8).expect("Invalid PID"), vaddr)
            {
                Ok(ptr) => ptr,
                Err(e) => return Err(e),
            };

        #[cfg(feature = "debug-swap")]
        println!("evict_page - swapper activate");
        // this is safe because evict_page() leaves us in the swapper memory context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::WriteToSwap(
                tid,
                sepc,
                PID::new(target_pid as u8).expect("Invalid PID"),
                vaddr,
                evicted_ptr,
            ));
        }
    }

    /// The address space on entry to `retrieve_page` is `target_pid`; it must ensure
    /// that the address space is still `target_pid` on return.
    ///
    /// Also takes as argument the virtual address of the target page in the target PID,
    /// as well as the physical address of the page.
    ///
    /// This call diverges into the userspace swapper.
    pub fn retrieve_page(
        &mut self,
        target_pid: PID,
        target_tid: TID,
        target_vaddr_in_pid: usize,
        paddr: usize,
    ) -> ! {
        let block_vaddr_in_swap =
            crate::arch::mem::map_page_to_swapper(paddr).expect("couldn't map target page to swapper");
        // we are now in the swapper's memory space

        #[cfg(feature = "debug-swap")]
        println!(
            "retrieve_page - userspace activate from pid{:?} for vaddr {:x?}",
            target_pid, target_vaddr_in_pid
        );
        // this is safe because map_page_to_swapper() leaves us in the swapper memory context
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::ReadFromSwap(
                target_pid,
                target_tid,
                target_vaddr_in_pid,
                block_vaddr_in_swap,
                paddr,
            ));
        }
    }

    /// Safety: the current page table mapping context must be PID 2 (the swapper's PID) for this to work
    /// `op` contains the opcode data
    /// `payload_ptr` is the pointer to the virtual address of the swapped block in PID2 space
    unsafe fn blocking_activate_swapper(&mut self, op: BlockingSwapOp) -> ! {
        // setup the argument block
        match op {
            // Note to self: get opcode number from `KernelOp` structure in services/xous-swapper/main.rs
            BlockingSwapOp::WriteToSwap(_tid, _sepc, pid, vaddr_in_pid, vaddr_in_swap) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 0; // WriteToSwap
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::ReadFromSwap(pid, _tid, vaddr_in_pid, vaddr_in_swap, _paddr) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 1; // ReadFromSwap
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::FetchAllocs(_tid, _sepc) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 2; // ExecFetchAllocs
                self.swapper_args[2] = self.mem_alloc_tracker_pages;
            }
            BlockingSwapOp::OomDoom(_tid, _pid) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 3; // OomDoom
                self.swapper_args[2] = KERNEL_OOM_DOOM_PAGES_TO_FREE;
            }
        }
        if let Some(op) = self.prev_op.take() {
            if let Some(dop) = self.nested_op {
                println!("ERR: nesting depth of 2 exceeded! {:?}", dop);
                panic!("Nesting depth of 2 exceeded!");
            }
            // println!("Nesting {:?}", op);
            self.nested_op = Some(op);
        }
        // println!("Setting prev_op to {:?}", op);
        self.prev_op = Some(op);
        let swapper_pid: PID = PID::new(xous_kernel::SWAPPER_PID).unwrap();

        SystemServices::with_mut(|ss| {
            // Disable all other IRQs and redirect into userspace
            crate::arch::irq::disable_all_irqs();
            ss.make_callback_to(
                swapper_pid,
                self.pc as *const usize,
                crate::services::CallbackType::Swap(self.swapper_args),
            )
        })
        .expect("couldn't switch to handler");
        // unmap args and payload

        crate::services::ArchProcess::with_current_mut(|process| {
            crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
        })
        // the call above diverges; the return end of things is inside the IRQ handler, where we
        // conduct business as if we're returning from a syscall.
    }

    /// Cleanup after `blocking_activate_swapper()` - called on return from the divergence at the end
    /// of the previous call.
    ///
    /// Safety: this call must only be invoked in the swapper's memory context
    pub unsafe fn exit_blocking_call(&mut self) -> Result<xous_kernel::Result, xous_kernel::Error> {
        let result = match self.prev_op.take() {
            Some(BlockingSwapOp::WriteToSwap(tid, sepc, pid, _addr, swapper_virt_page)) => {
                // Update the RPT: mark the physical memory as free. The virtual address is
                // in the swapper's context at this point, so free it there (it's already been
                // remapped as swapped in the target's context). Note that the swapped page now contains
                // encrypted data, as the encryption happens in-place.
                MemoryManager::with_mut(|mm| {
                    let paddr = crate::arch::mem::virt_to_phys(swapper_virt_page).unwrap() as usize;
                    println!("WTS releasing vaddr {:x}, paddr {:x}", swapper_virt_page, paddr);
                    // this call unmaps the virtual page from the page table
                    crate::arch::mem::unmap_page_inner(mm, swapper_virt_page).expect("couldn't unmap page");
                    // This call releases the physical page from the RPT - the pid has to match that of the
                    // original owner. This is the "pointy end" of the stick; after this call, the memory is
                    // now back into the free pool.
                    mm.release_page_swap(paddr as *mut usize, pid)
                        .expect("couldn't free page that was swapped out");
                });

                // Switch back to the Swapper's userland thread
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(PID::new(SWAPPER_PID).unwrap(), tid)
                        .expect("Unable to resume to swapper");
                });
                // restore the `sepc` for that thread
                crate::arch::process::Process::with_current_mut(|p| {
                    let thread = p.current_thread_mut();
                    thread.sepc = sepc;
                });
                // return as a SysCall
                Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
            }
            Some(BlockingSwapOp::ReadFromSwap(pid, tid, vaddr_in_pid, vaddr_in_swap, paddr)) => {
                MemoryManager::with_mut(|mm| {
                    // we are in the swapper's memory space a this point
                    // unmap the page from the swapper
                    crate::arch::mem::unmap_page_inner(mm, vaddr_in_swap)?;

                    // Access target PID page tables
                    SystemServices::with(|system_services| {
                        // swap to the swapper space
                        let target_map = system_services.get_process(pid).unwrap().mapping;
                        crate::arch::process::set_current_pid(pid);
                        target_map.activate()
                    })?;

                    let entry = crate::arch::mem::pagetable_entry(vaddr_in_pid)
                        .or(Err(xous_kernel::Error::BadAddress))?;
                    let current_entry = entry.read_volatile();
                    // clear the swapped flag
                    let flags = current_entry & 0x3ff & !MMUFlags::P.bits();
                    let ppn1 = (paddr >> 22) & ((1 << 12) - 1);
                    let ppn0 = (paddr >> 12) & ((1 << 10) - 1);
                    // Map the retrieved page to the target memory space, and set valid. I don't think `A`/`D`
                    // has any meaning, but we set it because the regular path would set
                    // that.
                    *entry = (ppn1 << 20)
                        | (ppn0 << 10)
                        | (flags | crate::arch::mem::FLG_VALID /* valid */
                        | crate::arch::mem::FLG_A/* A */
                        | crate::arch::mem::FLG_D/* D */
                        | crate::arch::mem::FLG_U/* USER */);
                    crate::arch::mem::flush_mmu();

                    // Return to swapper PID's address space, because that's what the
                    // finish_callback_and_resume assumes...slightly inefficient but
                    // better code re-use.
                    SystemServices::with(|system_services| {
                        // swap to the swapper space
                        let target_map =
                            system_services.get_process(PID::new(SWAPPER_PID).unwrap()).unwrap().mapping;
                        crate::arch::process::set_current_pid(PID::new(SWAPPER_PID).unwrap());
                        target_map.activate()
                    })?;
                    // Switch to the previous process' address space.
                    SystemServices::with_mut(|ss| {
                        ss.finish_callback_and_resume(pid, tid).expect("unable to resume previous PID")
                    });
                    // the current memory space is the target PID, so we will resume into the target PID
                    Ok(xous_kernel::Result::ResumeProcess)
                })
            }
            Some(BlockingSwapOp::FetchAllocs(tid, sepc)) => {
                // unmap RPT from userspace
                MemoryManager::with_mut(|mm| {
                    for page in 0..self.mem_alloc_tracker_pages {
                        crate::arch::mem::unmap_page_inner(mm, SWAP_RPT_VADDR + page * PAGE_SIZE).ok();
                    }
                });
                // Switch back to the Swapper's userland thread
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(PID::new(SWAPPER_PID).unwrap(), tid)
                        .expect("Unable to resume to swapper");
                });
                // restore the `sepc` for that thread
                crate::arch::process::Process::with_current_mut(|p| {
                    let thread = p.current_thread_mut();
                    thread.sepc = sepc;
                });
                // return as a SysCall
                Ok(xous_kernel::Result::Scalar5(0, 0, 0, 0, 0))
            }
            Some(BlockingSwapOp::OomDoom(tid, pid)) => {
                // Switch to the previous process' address space.
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(pid, tid).expect("unable to resume previous PID")
                });
                // the current memory space is the target PID, so we will resume into the target PID
                Ok(xous_kernel::Result::ResumeProcess)
            }
            None => panic!("No previous swap op was set"),
        };
        if let Some(op) = self.nested_op.take() {
            // println!("Popping nested_op into prev_op with {:?}", op);
            self.prev_op = Some(op);
        }
        result
    }
}
