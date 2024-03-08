use xous_kernel::{PID, SID};

/* for non-blocking calls
use xous_Kernel::{try_send_message, MemoryFlags, Message, MessageEnvelope, SysCallResult, TID};
use crate::server::SenderID; */
use crate::arch::current_pid;
use crate::arch::mem::MMUFlags;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

#[derive(Copy, Clone)]
pub enum BlockingSwapOp {
    /// PID of source, vaddr of source, vaddr in swap space (block must already be mapped into swap space)
    WriteToSwap(PID, usize, usize),
    /// PID of the target block, current paddr of block, original vaddr in the space of target block PID,
    /// physical address of block
    ReadFromSwap(PID, usize, usize, usize),
    /// PID of the process to return to after the allocate advisory
    AllocateAdvisory(PID),
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AllocAdvice {
    /// the PID of the allocation, virtual address in PID space, physical address
    Allocate(PID, usize, usize),
    /// the PID of the page freed, virtuall address in PID space, physical address
    Free(PID, usize, usize),
    /// not yet initialized record
    Uninit,
}
impl AllocAdvice {
    pub fn serialize(&self) -> (usize, usize) {
        match self {
            AllocAdvice::Allocate(pid, vaddr, paddr) => {
                (
                    (pid.get() as usize) << 24 | (vaddr >> 12),
                    (1 << 24) | (paddr >> 12), // 1 indicates an alloc
                )
            }
            AllocAdvice::Free(pid, vaddr, paddr) => {
                (
                    (pid.get() as usize) << 24 | (vaddr >> 12),
                    (0 << 24) | (paddr >> 12), // 0 indicates a free
                )
            }
            AllocAdvice::Uninit => (0, 0),
        }
    }
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
}
impl Swap {
    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut SWAP)
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
            Ok(xous_kernel::Result::Ok)
        } else {
            // someone is trying to steal the swapper's privileges!
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
            BlockingSwapOp::AllocateAdvisory(_pid) => {
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
            Some(BlockingSwapOp::AllocateAdvisory(pid)) => {
                // return to the target PID
                SystemServices::with(|system_services| {
                    // swap back to the original allocator's space
                    system_services.get_process(pid).unwrap().mapping.activate()?;
                    Ok(xous_kernel::Result::ResumeProcess)
                })
            }
            None => panic!("No previous swap op was set"),
        }
    }

    pub fn evict_page(&mut self, target_pid: PID, vaddr: usize) -> ! {
        let evicted_ptr = crate::arch::mem::evict_page_inner(target_pid, vaddr).expect("couldn't evict page");

        // this is safe because evict_page() leaves us in the swapper memory context
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
        target_vaddr_in_pid: usize,
        paddr: usize,
        is_allocate: bool,
    ) {
        let mut placed_index: Option<usize> = None;
        for (index, advisory) in self.alloc_advisories.iter_mut().enumerate() {
            if *advisory == AllocAdvice::Uninit {
                if is_allocate {
                    *advisory = AllocAdvice::Allocate(target_pid, target_vaddr_in_pid, paddr);
                } else {
                    *advisory = AllocAdvice::Free(target_pid, target_vaddr_in_pid, paddr);
                }
                placed_index = Some(index);
                break;
            }
        }
        match placed_index {
            Some(i) => {
                if i == self.alloc_advisories.len() - 1 {
                    let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
                    SystemServices::with(|ss| {
                        ss.get_process(swapper_pid).unwrap().mapping.activate().unwrap()
                    });

                    // this is safe because we've changed into the swapper's memory space
                    unsafe {
                        self.blocking_activate_swapper(BlockingSwapOp::AllocateAdvisory(target_pid));
                        // ^^ also note this has the side effect of clearing the advisory storage table
                    }
                    // call proceeds to swapper space -> we've diverged and will return via the
                    // swapper return path
                }
            }
            None => panic!("Error: advisory record ran out of space"),
        }
    }
}
