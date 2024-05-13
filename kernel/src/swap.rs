use loader::swap::SWAP_RPT_VADDR;
use xous_kernel::SWAPPER_PID;
use xous_kernel::{SysCallResult, PID, SID, TID};

/* for non-blocking calls
use xous_Kernel::{try_send_message, MemoryFlags, Message, MessageEnvelope, SysCallResult, TID};
use crate::server::SenderID; */
use crate::arch::current_pid;
use crate::arch::mem::MMUFlags;
use crate::arch::mem::PAGE_SIZE;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// This ABI is copy-paste synchronized with what's in the userspace handler. It's left out of
/// xous-rs so that we can change it without having to push crates to crates.io.
/// Since there is only one place the ABI could be used, we're going to stick with
/// this primitive method of synchronization because it reduces the activation barrier
/// to fix bugs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwapAbi {
    Invalid = 0,
    Evict = 1,
    GetFreeMem = 2,
    FetchAllocs = 3,
}
/// SYNC WITH `xous-swapper/src/main.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreeMem,
            3 => FetchAllocs,
            _ => Invalid,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum BlockingSwapOp {
    /// PID of source, vaddr of source, vaddr in swap space (block must already be mapped into swap space)
    WriteToSwap(PID, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block
    ReadFromSwap(PID, TID, usize, usize, usize),
    /// Argument is the original thread ID and `sepc` for return
    FetchAllocs(TID, usize),
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

    /// Safety: the current page table mapping context must be PID 2 (the swapper's PID) for this to work
    /// `op` contains the opcode data
    /// `payload_ptr` is the pointer to the virtual address of the swapped block in PID2 space
    unsafe fn blocking_activate_swapper(&mut self, op: BlockingSwapOp) -> ! {
        // setup the argument block
        match op {
            BlockingSwapOp::WriteToSwap(pid, vaddr_in_pid, vaddr_in_swap) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 0; // WriteToSwap opcode
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::ReadFromSwap(pid, _tid, vaddr_in_pid, vaddr_in_swap, _paddr) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 1; // ReadFromSwap opcode
                self.swapper_args[2] = pid.get() as usize;
                self.swapper_args[3] = vaddr_in_pid;
                self.swapper_args[4] = vaddr_in_swap;
            }
            BlockingSwapOp::FetchAllocs(_tid, _sepc) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 2; // ExecFetchAllocs opcode
                self.swapper_args[2] = self.mem_alloc_tracker_pages;
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
            Some(BlockingSwapOp::WriteToSwap(pid, addr, _virt_addr)) => {
                // update the RPT: mark the physical memory as free. The physical page is
                // in the swapper's context at this point, so free it there (it's already been
                // remapped as swapped in the target's context)
                MemoryManager::with_mut(|mm| {
                    mm.release_page_swap(addr as *mut usize, pid)
                        .expect("couldn't clear the RPT after flushing swap")
                });
                // this will resume into the swapper, because that is our memory space right now
                Ok(xous_kernel::Result::ResumeProcess)
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
                        unsafe { crate::arch::mem::flush_mmu() };
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
            None => panic!("No previous swap op was set"),
        };
        if let Some(op) = self.nested_op.take() {
            // println!("Popping nested_op into prev_op with {:?}", op);
            self.prev_op = Some(op);
        }
        result
    }

    pub fn evict_page(&mut self, target_pid: usize, vaddr: usize) -> SysCallResult {
        let evicted_ptr =
            crate::arch::mem::evict_page_inner(PID::new(target_pid as u8).expect("Invalid PID"), vaddr)
                .expect("couldn't evict page");

        // this is safe because evict_page() leaves us in the swapper memory context
        #[cfg(feature = "debug-swap")]
        println!("evict_page - swapper activate, PC: {:08x}", self.pc);
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::WriteToSwap(
                PID::new(target_pid as u8).expect("Invalid PID"),
                vaddr,
                evicted_ptr,
            ));
        }
    }

    pub fn get_free_mem(&self) -> SysCallResult {
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

        // TODO: use self.free_pages and eliminate the population count once we have confidence that the
        // scheme works!
        Ok(xous_kernel::Result::Scalar5(total_bytes, self.free_pages, 0, 0, 0))
    }

    /// The address space on entry to `retrieve_page` is `target_pid`; it must ensure
    /// that the address space is still `target_pid` on return.
    ///
    /// Also takes as argument the virtual address of the target page in the target PID,
    /// as well as the physical address of the page.
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
        println!("retrieve_page - userspace activate");
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
}
