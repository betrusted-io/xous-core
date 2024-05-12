use xous_kernel::SWAPPER_PID;
use xous_kernel::{AllocAdvice, MemoryRange, SysCallResult, PID, SID, TID};

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
}
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => Evict,
            2 => GetFreeMem,
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
    /*
    /// PID of the process to return to after the allocate advisory - if incurred during a page fault
    AllocateAdvisory(PID, TID),
    /// advisory issued as part of a syscall - e.g., unmap
    AllocateAdvisorySyscall(PID, TID, usize, AdviseUnmap),
    */
}

/// Tracks the entire range of the structure being unmapped. May require multiple
/// iterations of calls if this structure is large.
#[derive(Copy, Clone, Debug)]
pub struct AdviseUnmap {
    base: usize,
    size: usize,
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
    alloc_advisories: [AllocAdvice::Uninit; 8],
    missing_pages: 0,
    epoch: 0,
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
    /// track advisories to the allocator
    alloc_advisories: [AllocAdvice; 8],
    /// count missing advisories because the advisory list overflowed
    missing_pages: usize,
    /// swap epoch tracker
    epoch: u32,
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

    pub fn register_handler(
        &mut self,
        s0: u32,
        s1: u32,
        s2: u32,
        s3: u32,
        handler: usize,
        state: usize,
    ) -> Result<xous_kernel::Result, xous_kernel::Error> {
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

    /*
    /// This will insert a message into the swapper's server queue. Useful for informational messages to the
    /// swapper.
    fn nonblocking_activate_swapper(&self, swapper_msg: Message) -> SysCallResult {
        assert!(!swapper_msg.is_blocking(), "Only non-blocking messages may be sent to the the swapper");

        let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
        SystemServices::with_mut(|ss| {
            let sidx = ss.sidx_from_sid(self.sid, swapper_pid).expect("Couldn't find swapper server");
            let server = ss.server_from_sidx_mut(sidx).expect("swapper couldn't be located");
            let server_pid = server.pid;

            if let Some(server_tid) = server.take_available_thread() {
                // if the swapper can respond, send the message and switch to it
                // note: swapper_msg must be a non-blocking type of message for this code path
                let sender = SenderID::new(sidx, 0, Some(swapper_pid));
                let envelope = MessageEnvelope { sender: sender.into(), body: swapper_msg };

                // Mark the swapper's context as "Ready".
                #[cfg(baremetal)]
                ss.ready_thread(swapper_pid, server_tid)?;

                if cfg!(baremetal) {
                    ss.set_thread_result(
                        server_pid,
                        server_tid,
                        xous_kernel::Result::MessageEnvelope(envelope),
                    )
                    .map(|_| xous_kernel::Result::Ok)
                } else {
                    // "Switch to" the server PID when not running on bare metal. This ensures
                    // that it's "Running".
                    ss.switch_to_thread(server_pid, Some(server_tid))?;
                    ss.set_thread_result(
                        server_pid,
                        server_tid,
                        xous_kernel::Result::MessageEnvelope(envelope),
                    )
                    .map(|_| xous_kernel::Result::Ok)
                };
            } else {
                // else, queue it for processing later
                let tid: TID = ss.get_process(swapper_pid).unwrap().current_thread;
                // this will error-out if the swapper queue is full, leading to much badness. However,
                // I don't think there is a defined behavior if the swapper can just miss messages.
                let _queue_idx = ss.queue_server_message(sidx, swapper_pid, tid, swapper_msg, None)?;
            }
        });
    } */

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
            // AllocateAdvisory and AllocateAdvisorySyscall patterns
            _ => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 2; // AllocateAdvisory
                for (index, advisory) in self.alloc_advisories.iter_mut().enumerate() {
                    let (varg, parg) = advisory.serialize();
                    self.swapper_args[2 + index * 2] = varg;
                    self.swapper_args[3 + index * 2] = parg;
                    *advisory = AllocAdvice::Uninit;
                }
            }
        }
        if let Some(op) = self.prev_op.take() {
            if let Some(dop) = self.nested_op {
                println!("ERR: nesting depth of 2 exceeded! {:?}", dop);
                panic!("Nesting depth of 2 exceeded!");
            }
            self.nested_op = Some(op);
        }
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
            /*
            Some(BlockingSwapOp::AllocateAdvisory(pid, tid)) => {
                // Switch to the previous process' address space.
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(pid, tid).expect("unable to resume previous PID")
                });
                Ok(xous_kernel::Result::ResumeProcess)
            }
            Some(BlockingSwapOp::AllocateAdvisorySyscall(pid, tid, virt, mapping)) => {
                // We're in the swapper's address space here. We need to get back to the original process'
                // address space first.
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(pid, tid).expect("unable to resume previous PID")
                });

                println!("Exit from allocate advisory: {:x}/{:x?}", virt, mapping);
                // Free the virtual address.
                let result =
                    MemoryManager::with_mut(|mm| crate::arch::mem::unmap_page_inner(mm, virt as usize))
                        .map(|_| xous_kernel::Result::Ok);
                if result.is_ok() {
                    // We might be resuming from a previous call that went to the swapper to advise on
                    // unmapped pages, and we have to resume the loop.
                    let next_page = virt + PAGE_SIZE; // start the loop on the next page
                    let mut result = Ok(xous_kernel::Result::Ok);
                    for addr in (next_page..(mapping.base + mapping.size)).step_by(PAGE_SIZE) {
                        result = self.unmap_inner(pid, tid, addr, mapping).map(|_| xous_kernel::Result::Ok);
                        if result.is_err() {
                            break;
                        }
                    }
                    result
                } else {
                    result
                }
            } */
            None => panic!("No previous swap op was set"),
        };
        if let Some(op) = self.nested_op.take() {
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
        println!("{} k total", total_bytes / 1024);

        /*
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();
            let process = system_services.get_process(PID::new(9).unwrap()).unwrap();
            println!("PID {} {}:", process.pid, system_services.process_name(process.pid).unwrap_or(""));
            process.activate().unwrap();
            crate::arch::mem::MemoryMapping::current().print_map();
            system_services.get_process(current_pid).unwrap().activate().unwrap();
        }); */

        Ok(xous_kernel::Result::Scalar5(total_bytes, 0, 0, 0, 0))
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

        // this is safe because map_page_to_swapper() leaves us in the swapper memory context
        #[cfg(feature = "debug-swap")]
        println!("retrieve_page - swapper activate, PC: {:08x}", self.pc);
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
    /*
    /// Update the alloc scoreboard to transmit to the swapper
    pub fn tally_alloc_free(
        &mut self,
        // also the PID to return from after reporting to the swapper
        target_pid: PID,
        target_tid: TID,
        target_vaddr_in_pid: usize,
        paddr: usize,
        is_free: bool,
    ) {
        let new_advice = if is_unmap.is_some() {
            AllocAdvice::Free(target_pid, target_vaddr_in_pid, paddr)
        } else {
            AllocAdvice::Allocate(target_pid, target_vaddr_in_pid, paddr)
        };
        // First, search the advisories for existing, cached advice that matches a previous op
        // advisories are tracked on tables that are keyed off of the physical address,
        // so what we need to look for are items that match on the physical address.
        let mut stored = false;
        for (index, advisory) in self.alloc_advisories.iter_mut().enumerate() {
            match advisory {
                AllocAdvice::Allocate(_, _, pa) => {
                    if *pa == paddr {
                        *advisory = new_advice;
                        stored = true;
                        break;
                    }
                }
                AllocAdvice::Free(_, _, pa) => {
                    if *pa == paddr {
                        *advisory = new_advice;
                        stored = true;
                        break;
                    }
                }
                AllocAdvice::Uninit => {
                    self.alloc_advisories[index] = new_advice;
                    stored = true;
                    break;
                }
            }
        }
        if !stored {
            self.missing_pages += 1;
            println!("advise_alloc total missed advisories: {}", self.missing_pages);
        }
        if self.pc != 0 {
            self.flush_advisories();
        }
    }

    // This should be called between quantum if advisories exist to be flushed.
    pub fn flush_advisories(&mut self) {
        assert!(self.pc != 0);
        let mut alloc_handoff = [AllocAdvice::Uninit; 2];

        // take the first two items and send them
        for (index, advisory) in self.alloc_advisories.iter().enumerate() {}

        // LEFT OFF at:
        // I think advisories have diverged from their original design intent
        // We don't want to advise the swapper of every map/unmap -- doing so causes
        // a lot of overhead because message passing between processes leads to lots
        // of useless advisories. We only want to advise on allocations of heap, stack,
        // and text (code) pages. Everything else (memory messages, etc.) would be data
        // in active use and probably should not be unmapped?
        //
        // Also, for efficiency, we want to advise allocations in pairs, to reduce
        // expensive user-space transitions; but I think unmaps may have to be advised
        // individually because they could be syscalls that have to be returned to.
        //
        // There is also the problem of having to send multiple advisories to clear
        // out the advisory cache, which means we need to have a way to re-enter
        // the loop when the call returns.
        //
        // Questions:
        //   - can we differentiate between allocates of these types?
        //   - can we differentiate between the unmap calls?
        //   - how do we do a loop around advisories being sent to clear the advisory cache?
        //   - are unmaps special in that they need some specific return handling?
        //

        if last_index >= self.alloc_advisories.len() - 1 {
            #[cfg(feature = "debug-swap")]
            {
                // debugging
                SystemServices::with(|ss| {
                    let current = ss.get_process(current_pid()).unwrap();
                    let state = current.state();
                    println!(
                        "advise_alloc: switching to userspace from PID{}-{:?}",
                        current.pid.get(),
                        state
                    );
                });
            }

            let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
            SystemServices::with(|ss| ss.get_process(swapper_pid).unwrap().mapping.activate().unwrap());

            // this is safe because we've changed into the swapper's memory space
            if let Some(unmap_advice) = is_unmap {
                // not an allocate -> this came through the unmap syscall
                unsafe {
                    self.blocking_activate_swapper(BlockingSwapOp::AllocateAdvisorySyscall(
                        target_pid,
                        target_tid,
                        target_vaddr_in_pid,
                        unmap_advice,
                    ));
                }
            } else {
                unsafe {
                    self.blocking_activate_swapper(BlockingSwapOp::AllocateAdvisory(target_pid, target_tid));
                }
            }
            // ^^ also note this has the side effect of clearing the advisory storage table
            // call proceeds to swapper space -> we've diverged and will return via the
            // swapper return path
        }
    }
    */
}
