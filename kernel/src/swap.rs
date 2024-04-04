use xous_kernel::{AllocAdvice, MemoryRange, SysCallResult, PID, SID, TID};

/* for non-blocking calls
use xous_Kernel::{try_send_message, MemoryFlags, Message, MessageEnvelope, SysCallResult, TID};
use crate::server::SenderID; */
use crate::arch::current_pid;
use crate::arch::mem::MMUFlags;
use crate::arch::mem::PAGE_SIZE;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

#[derive(Copy, Clone)]
pub enum BlockingSwapOp {
    /// PID of source, vaddr of source, vaddr in swap space (block must already be mapped into swap space)
    WriteToSwap(PID, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block
    ReadFromSwap(PID, usize, usize, usize),
    /// PID of the process to return to after the allocate advisory - if incurred during a page fault
    AllocateAdvisory(PID, TID),
    /// advisory issued as part of a syscall - e.g., unmap
    AllocateAdvisorySyscall(PID, TID, usize, AdviseUnmap),
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
    spt_ptr: 0,
    smt_base: 0,
    smt_bounds: 0,
    rpt_ptr: 0,
    sid: SID::from_u32(0, 0, 0, 0),
    pc: 0,
    prev_op: None,
    swapper_state: 0,
    swapper_args: [0usize; 8],
    alloc_advisories: [AllocAdvice::Uninit, AllocAdvice::Uninit, AllocAdvice::Uninit],
    missing_pages: 0,
};

pub struct Swap {
    /// Pointer to the swap page table base
    spt_ptr: usize,
    /// SMT base and bounds: address meanings can vary depending on the target system,
    /// if swap is memory-mapped, or if behind a SPI register interface.
    smt_base: usize,
    smt_bounds: usize,
    /// Pointer to runtime page tracker
    rpt_ptr: usize,
    /// SID for the swapper
    sid: SID,
    /// PC for blocking handler
    pc: usize,
    /// previous op
    prev_op: Option<BlockingSwapOp>,
    /// state for the swapper. this is a PID-2 local virtual address, passed from the swapper on registration
    swapper_state: usize,
    /// storage for args
    swapper_args: [usize; 8],
    /// track advisories to the allocator
    alloc_advisories: [AllocAdvice; 3],
    /// count missing advisories because the swapper has yet to register
    missing_pages: usize,
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

    pub fn init_from_args(
        &mut self,
        args: &crate::args::KernelArguments,
    ) -> Result<xous_kernel::Result, xous_kernel::Error> {
        for tag in args.iter() {
            if tag.name == u32::from_le_bytes(*b"Swap") {
                self.spt_ptr = tag.data[0] as usize;
                self.smt_base = tag.data[1] as usize;
                self.smt_bounds = tag.data[2] as usize;
                self.rpt_ptr = tag.data[3] as usize;
                return Ok(xous_kernel::Result::Ok);
            }
        }
        Err(xous_kernel::Error::UseBeforeInit)
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
            Ok(xous_kernel::Result::Scalar5(self.spt_ptr, self.smt_base, self.smt_bounds, self.rpt_ptr, 0))
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
            BlockingSwapOp::ReadFromSwap(pid, vaddr_in_pid, vaddr_in_swap, _paddr) => {
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
        match self.prev_op.take() {
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
            Some(BlockingSwapOp::ReadFromSwap(pid, vaddr_in_pid, vaddr_in_swap, paddr)) => {
                MemoryManager::with_mut(|mm| {
                    // we are in the swapper's memory space a this point
                    // unmap the page from the swapper
                    crate::arch::mem::unmap_page_inner(mm, vaddr_in_swap)?;

                    // return to the target PID
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
                    let flags = current_entry & 0x1ff & !MMUFlags::P.bits();
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
                    // the current memory space is the target PID, so we will resume into the target PID
                    Ok(xous_kernel::Result::ResumeProcess)
                })
            }
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
            }
            None => panic!("No previous swap op was set"),
        }
    }

    pub fn evict_page(&mut self, target_pid: PID, vaddr: usize) -> ! {
        let evicted_ptr = crate::arch::mem::evict_page_inner(target_pid, vaddr).expect("couldn't evict page");

        // this is safe because evict_page() leaves us in the swapper memory context
        #[cfg(feature = "debug-swap")]
        println!("evict_page - swapper activate, PC: {:08x}", self.pc);
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::WriteToSwap(target_pid, vaddr, evicted_ptr));
        }
    }

    /// The address space on entry to `retrieve_page` is `target_pid`; it must ensure
    /// that the address space is still `target_pid` on return.
    ///
    /// Also takes as argument the virtual address of the target page in the target PID,
    /// as well as the physical address of the page.
    pub fn retrieve_page(&mut self, target_pid: PID, target_vaddr_in_pid: usize, paddr: usize) -> ! {
        let block_vaddr_in_swap =
            crate::arch::mem::map_page_to_swapper(paddr).expect("couldn't map target page to swapper");
        // we are now in the swapper's memory space

        // this is safe because map_page_to_swapper() leaves us in the swapper memory context
        #[cfg(feature = "debug-swap")]
        println!("retrieve_page - swapper activate, PC: {:08x}", self.pc);
        unsafe {
            self.blocking_activate_swapper(BlockingSwapOp::ReadFromSwap(
                target_pid,
                target_vaddr_in_pid,
                block_vaddr_in_swap,
                paddr,
            ));
        }
    }

    /// Accumulate allocations and advise the swapper en-bulk of allocations. This will diverge
    /// only when it's determined that we need to advise the swapper.
    pub fn advise_alloc(
        &mut self,
        // also the PID to return from after reporting to the swapper
        target_pid: PID,
        target_tid: TID,
        target_vaddr_in_pid: usize,
        paddr: usize,
        maybe_unmap: Option<AdviseUnmap>,
    ) {
        let mut last_index: usize = 0;
        let mut overflow = false;
        for (index, advisory) in self.alloc_advisories.iter_mut().enumerate() {
            if *advisory == AllocAdvice::Uninit {
                if maybe_unmap.is_none() {
                    *advisory = AllocAdvice::Allocate(target_pid, target_vaddr_in_pid, paddr);
                } else {
                    *advisory = AllocAdvice::Free(target_pid, target_vaddr_in_pid, paddr);
                }
                last_index = index;
                overflow = false;
                break;
            } else {
                overflow = true;
                last_index = index;
            }
        }
        if self.pc != 0 {
            if last_index >= self.alloc_advisories.len() - 1 {
                {
                    // debugging
                    SystemServices::with(|ss| {
                        let current = ss.get_process(current_pid()).unwrap();
                        let state = current.state();
                        println!("state before swapper switch: {} {:?}", current.pid.get(), state);
                    });
                }

                let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
                SystemServices::with(|ss| ss.get_process(swapper_pid).unwrap().mapping.activate().unwrap());

                // this is safe because we've changed into the swapper's memory space
                #[cfg(feature = "debug-swap")]
                println!("advise_alloc - swapper activate, PC: {:08x}", self.pc);
                if let Some(unmap_advice) = maybe_unmap {
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
                        self.blocking_activate_swapper(BlockingSwapOp::AllocateAdvisory(
                            target_pid, target_tid,
                        ));
                    }
                }
                // ^^ also note this has the side effect of clearing the advisory storage table
                // call proceeds to swapper space -> we've diverged and will return via the
                // swapper return path
            }
        } else {
            // If the swapper hasn't registered yet, don't advise it of anything - the call
            // would crash. That being said, this means we effectively get some extra pages of
            // memory that are "wired" (not swappable) since they were never marked as allocated.
            // If this is a small number, I think it's not worth the complexity to track and
            // register? For now, just print to the kernel log so we know how bad this is;
            // if the swapper registers *immediately* (without doing any debug prints or sycalls),
            // we actually miss no pages.
            if overflow {
                self.missing_pages += 1;
                println!("missed advisory: {}", self.missing_pages);
            }
        }
    }

    pub fn unmap(&mut self, range: MemoryRange) -> SysCallResult {
        let virt = range.as_ptr() as usize;
        let size = range.len();
        if cfg!(baremetal) && virt & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        let pid = crate::arch::process::current_pid();
        let tid = crate::arch::process::current_tid();

        for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
            self.unmap_inner(pid, tid, addr, AdviseUnmap { base: virt, size })?;
        }
        Ok(xous_kernel::Result::Ok)
    }

    /// Note: this call may diverge into the swapper if the advice buffer fills up. We have to be
    /// prepared to resume the loop where it left off!
    ///
    /// ASSUME: we are in the process space of the unmap caller, *not* the swapper, on entry.
    fn unmap_inner(
        &mut self,
        pid: PID,
        tid: TID,
        virt: usize,
        mapping: AdviseUnmap,
    ) -> Result<usize, xous_kernel::Error> {
        // If the virtual address has an assigned physical address, release that
        // address from this process.
        if let Ok(phys) = crate::arch::mem::virt_to_phys(virt as usize) {
            MemoryManager::with_mut(|mm| mm.release_page_swap(phys as *mut usize, pid).ok());
            self.advise_alloc(pid, tid, virt as usize, phys, Some(mapping));
        } else {
            // return a null physical pointer if only virtual memory is being freed
            self.advise_alloc(pid, tid, virt as usize, 0, Some(mapping));
        };

        // Free the virtual address.
        MemoryManager::with_mut(|mm| crate::arch::mem::unmap_page_inner(mm, virt as usize))
    }
}