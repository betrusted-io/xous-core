use xous_kernel::{MemoryFlags, Message, MessageEnvelope, SysCallResult, PID, SID, TID};

use crate::arch::current_pid;
use crate::arch::mem::PAGE_SIZE;
use crate::mem::MemoryManager;
use crate::server::SenderID;
use crate::services::SystemServices;

#[derive(Copy, Clone)]
pub enum BlockingSwapOp {
    /// PID of the source block, current paddr of block, original vaddr in the space of source block PID
    WriteToSwap(PID, usize, usize),
    ReadFromSwap,
    AllocateAdvisory,
    Free,
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
}
impl Swap {
    /// Calls the provided function with the current inner process state.
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&Swap) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&SWAP)
        }
        #[cfg(not(baremetal))]
        SWAP.with(|ss| f(&ss.borrow()))
    }

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
        let swapper_pid = PID::new(xous_kernel::SWAPPER_PID).unwrap();
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
    unsafe fn blocking_activate_swapper(&mut self, op: BlockingSwapOp, payload_ptr: usize) {
        // setup the argument block
        match op {
            BlockingSwapOp::WriteToSwap(pid, phys_addr, virt_addr) => {
                self.swapper_args[0] = self.swapper_state;
                self.swapper_args[1] = 0; // WriteToSwap opcode
                self.swapper_args[2] = payload_ptr;
                self.swapper_args[3] = pid.get() as usize;
                self.swapper_args[4] = phys_addr;
                self.swapper_args[5] = virt_addr;
            }
            _ => {
                todo!()
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
        });
        // the call above diverges; the return end of things is inside the IRQ handler, where we
        // conduct business as if we're returning from a syscall.
    }

    /// Cleanup after `blocking_activate_swapper()` - called on return from the divergence at the end
    /// of the previous call.
    pub fn exit_blocking_call(&mut self) -> Result<xous_kernel::Result, xous_kernel::Error> {
        match self.prev_op.take() {
            Some(BlockingSwapOp::WriteToSwap(pid, addr, _virt_addr)) => {
                // update the RPT: mark the physical memory as free
                MemoryManager::with_mut(|mm| {
                    mm.release_page_swap(addr as *mut usize, pid)
                        .expect("couldn't clear the RPT after flushing swap");
                });
            }
            Some(BlockingSwapOp::ReadFromSwap) => {
                todo!()
            }
            Some(BlockingSwapOp::Free) => (),
            Some(BlockingSwapOp::AllocateAdvisory) => (),
            None => panic!("No previous swap op was set"),
        }
        Ok(xous_kernel::Result::ResumeProcess)
    }

    pub fn evict_page(&mut self, target_pid: PID, vaddr: usize) -> SysCallResult {
        let evicted_ptr = crate::arch::mem::evict_page(target_pid, vaddr)?;

        // this is safe because evict_page() leaves us in the swapper memory context
        unsafe {
            self.blocking_activate_swapper(
                BlockingSwapOp::WriteToSwap(target_pid, evicted_ptr, vaddr),
                evicted_ptr,
            );
        }
        SysCallResult::Ok(xous_kernel::Result::ResumeProcess)
    }
}
