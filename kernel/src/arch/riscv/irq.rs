use crate::arch::current_pid;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::Process as ArchProcess;
use crate::arch::process::{Thread, RETURN_FROM_ISR};
use crate::mem::{MemoryManager, PAGE_SIZE};
use crate::services::SystemServices;
use riscv::register::{scause, sepc, sie, sstatus, stval, vexriscv::sim, vexriscv::sip};
use xous::{SysCall, PID, TID};

extern "Rust" {
    fn _xous_syscall_return_result(result: &xous::Result, context: &Thread) -> !;
}

extern "C" {
    fn flush_mmu();
}

/// Disable external interrupts
pub fn disable_all_irqs() {
    unsafe { sie::clear_sext() };
}

/// Enable external interrupts
pub fn enable_all_irqs() {
    unsafe { sie::set_sext() };
}

pub fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    sim::write(sim::read() | (1 << irq_no));
}

pub fn disable_irq(irq_no: usize) -> Result<(), xous::Error> {
    sim::write(sim::read() & !(1 << irq_no));
    Ok(())
}

static mut PREVIOUS_PAIR: Option<(PID, TID)> = None;

pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) {
    PREVIOUS_PAIR = Some((pid, tid));
}

pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> {
    PREVIOUS_PAIR.take()
}

/// Trap entry point rust (_start_trap_rust)
///
/// scause is read to determine the cause of the trap. The top bit indicates if
/// it's an interrupt or an exception. The result is converted to an element of
/// the Interrupt or Exception enum and passed to handle_interrupt or
/// handle_exception.
#[export_name = "_start_trap_rust"]
pub extern "C" fn trap_handler(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> ! {
    let sc = scause::read();

    // If we were previously in Supervisor mode and we've just tried to write to
    // invalid memory, then we likely blew out the stack.
    if cfg!(target_arch = "riscv32")
        && sstatus::read().spp() == sstatus::SPP::Supervisor
        && sc.bits() == 0xf
    {
        panic!("Ran out of kernel stack");
    }

    let pid = crate::arch::current_pid();

    if (sc.bits() == 9) || (sc.bits() == 8) {
        // We got here because of an `ecall` instruction.  When we return, skip
        // past this instruction.  If this is a call such as `SwitchTo`, then we
        // will want to adjust the return value of the current process prior to
        // performing the switch in order to avoid constantly executing the same
        // instruction.
        let tid = ArchProcess::with_current_mut(|p| {
            p.current_thread().sepc += 4;
            p.current_tid()
        });
        let call = SysCall::from_args(a0, a1, a2, a3, a4, a5, a6, a7).unwrap_or_else(|_| {
            ArchProcess::with_current_mut(|p| unsafe {
                _xous_syscall_return_result(
                    &xous::Result::Error(xous::Error::UnhandledSyscall),
                    p.current_thread(),
                )
            })
        });

        let response =
            crate::syscall::handle(pid, tid, call).unwrap_or_else(|e| xous::Result::Error(e));

        // println!("Syscall Result: {:?}", response);
        ArchProcess::with_current_mut(|p| {
            let thread = p.current_thread();
            // If we're resuming a process that was previously sleeping, restore the
            // context. Otherwise, keep the context the same but pass the return
            // values in 8 return registers.
            if response == xous::Result::ResumeProcess {
                crate::arch::syscall::resume(current_pid().get() == 1, thread);
            } else {
                println!(
                    "Returning to address {:08x}",
                    ArchProcess::with_current_mut(|p| p.current_thread().sepc)
                );
                unsafe { _xous_syscall_return_result(&response, thread) };
            }
        });
    }

    use crate::arch::exception::RiscvException;
    let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
    if sc.is_exception() {
        // If the CPU tries to store, look for a "reserved page" and provide
        // it with one if necessary.
        match ex {
            RiscvException::StorePageFault(pc, addr) | RiscvException::LoadPageFault(pc, addr) => {
                println!("Fault {} @ {:08x}, addr {:08x}", ex, pc, addr);
                let entry = crate::arch::mem::pagetable_entry(addr).unwrap_or_else(|x| {
                    // MemoryManagerHandle::get().print_ownership();
                    MemoryMapping::current().print_map();
                    panic!(
                        "error {:?} at {:08x}: memory not mapped or reserved for addr {:08x}",
                        x, pc, addr
                    );
                });
                let flags = *entry & 0x1ff;

                // If the flags are nonzero, but the "Valid" bit is not 1 and
                // the page isn't shared, then this is a reserved page. Allocate
                // a real page to back it and resume execution.
                if flags & 1 == 0 && flags != 0 && flags & (1 << 8) == 0 {
                    let new_page = MemoryManager::with_mut(|mm| {
                        mm.alloc_page(pid).expect("Couldn't allocate new page")
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
                        let virt = addr & !0xfff;
                        (virt as *mut usize)
                            .write_bytes(0, PAGE_SIZE / core::mem::size_of::<usize>());

                        // Move the page into userspace
                        *entry = (ppn1 << 20)
                            | (ppn0 << 10)
                            | (flags | (1 << 0) /* valid */ | (1 << 4) /* USER */ | (1 << 6) /* D */ | (1 << 7)/* A */);
                        flush_mmu();
                    };

                    ArchProcess::with_current_mut(|process| {
                        crate::arch::syscall::resume(
                            current_pid().get() == 1,
                            process.current_thread(),
                        )
                    });
                }
            }
            RiscvException::InstructionPageFault(RETURN_FROM_ISR, _offset) => {
                // If we hit this address, then an ISR has just returned.  Since
                // we're in an interrupt context, it is safe to access this
                // global variable.
                let (previous_pid, previous_context) = unsafe {
                    PREVIOUS_PAIR
                        .take()
                        .expect("got an instruction page fault with no previous PID")
                };
                println!(
                    "ISR: Resuming previous pair of ({}, {})",
                    previous_pid, previous_context
                );
                // Switch to the previous process' address space.
                SystemServices::with_mut(|ss| {
                    ss.finish_callback_and_resume(previous_pid, previous_context)
                        .expect("unable to resume previous PID")
                });

                // Re-enable interrupts now that they're handled
                enable_all_irqs();

                ArchProcess::with_current_mut(|process| {
                    crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                });
            }
            _ => (),
        }
        println!("SYSTEM HALT: CPU Exception on PID {}: {}", pid, ex);
        MemoryMapping::current().print_map();
        loop {}
    } else {
        let irqs_pending = sip::read();
        // Safe to access globals since interrupts are disabled
        // when this function runs.
        unsafe {
            if PREVIOUS_PAIR.is_none() {
                let tid = crate::arch::process::current_tid();
                PREVIOUS_PAIR = Some((pid, tid));
                println!("ISR: Setting previous pair to ({}, {})", pid, tid);
            } else {
                println!("ISR: Previous pair is not None");
            }
        }
        crate::irq::handle(irqs_pending).expect("Couldn't handle IRQ");
        ArchProcess::with_current_mut(|process| {
            crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
        })
    }
}
