use crate::arch::current_pid;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::ProcessHandle;
use crate::mem::{MemoryManagerHandle, PAGE_SIZE};
use crate::services::{ProcessContext, ProcessState, SystemServicesHandle, RETURN_FROM_ISR};
use riscv::register::{scause, sepc, sie, sstatus, stval, vexriscv::sim, vexriscv::sip};
use xous::{SysCall, PID};

extern "Rust" {
    fn _xous_syscall_return_result(result: &xous::Result, context: &ProcessContext) -> !;
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

pub fn disable_irq(irq_no: usize) {
    sim::write(sim::read() & !(1 << irq_no));
}

static mut PREVIOUS_PID: Option<PID> = None;

// fn map_page_and_return(pc: usize, addr: usize, pid: PID, flags: MemoryFlags) {
//     assert!(
//         pid > 1,
//         "kernel store page fault (pc: {:08x}  target: {:08x})",
//         pc,
//         addr
//     );

//     {
//         let mut mm = MemoryManagerHandle::get();
//         let new_page = mm.alloc_page(pid).expect("Couldn't allocate new page");
//         println!(
//             "Allocating new physical page {:08x} @ {:08x}",
//             new_page,
//             (addr & !4095)
//         );
//         mm.map_range(
//             new_page as *mut usize,
//             (addr & !4095) as *mut usize,
//             4096,
//             flags,
//         )
//         .expect("Couldn't map new stack");
//     }
//     crate::arch::syscall::resume(current_pid() == 1, ProcessContext::current());
// }

/// Trap entry point rust (_start_trap_rust)
///
/// scause is read to determine the cause of the trap. The top bit indicates
/// if it's an interrupt or an exception. The result is converted to an element
/// of the Interrupt or Exception enum and passed to handle_interrupt or
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

    // If we were previously in Supervisor mode and we've just tried to write
    // to invalid memory, then we likely blew out the stack.
    if cfg!(target_arch = "riscv32")
        && sstatus::read().spp() == sstatus::SPP::Supervisor
        && sc.bits() == 0xf
    {
        panic!("Ran out of kernel stack");
    }

    if (sc.bits() == 9) || (sc.bits() == 8) {
        // We got here because of an `ecall` instruction.  When we return, skip
        // past this instruction.  If this is a call such as `SwitchTo`, then we
        // will want to adjust the return value of the current process prior to
        // performing the switch in order to avoid constantly executing the same
        // instruction.
        let mut process = ProcessHandle::get();
        let ctx = process.current_context();
        ctx.sepc += 4;
        let call = SysCall::from_args(a0, a1, a2, a3, a4, a5, a6, a7).unwrap_or_else(|_| unsafe {
            _xous_syscall_return_result(&xous::Result::Error(xous::Error::UnhandledSyscall), ctx)
        });

        let response = crate::syscall::handle(call);

        println!("Result: {:?}", response);
        let mut process = ProcessHandle::get();

        // If we're resuming a process that was previously sleeping, restore the context.
        // Otherwise, keep the context the same but pass the return values in 8 return
        // registers.
        if response == xous::Result::ResumeProcess {
            crate::arch::syscall::resume(current_pid() == 1, ctx);
        } else {
            println!("Returning to address {:08x}", ctx.sepc);
            // crate::arch::syscall::resume(current_pid() == 1, ctx);
            unsafe { _xous_syscall_return_result(&response, ctx) };
        }
    }

    let pid = crate::arch::current_pid();
    use crate::arch::exception::RiscvException;
    let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
    if sc.is_exception() {
        // If the CPU tries to store, look for a "reserved page" and provide
        // it with one if necessary.
        match ex {
            RiscvException::StorePageFault(pc, addr) | RiscvException::LoadPageFault(pc, addr) => {
                println!("Fault {} @ {:08x}, addr {:08x}", ex, pc, addr);
                let mapping = MemoryMapping::current();
                let entry = mapping.pagetable_entry(addr);
                if entry as usize == 0 {
                    // MemoryManagerHandle::get().print_ownership();
                    MemoryMapping::current().print_map();
                    panic!(
                        "error at {:08x}: memory not mapped or reserved for addr {:08x}",
                        pc, addr
                    );
                }
                let flags = unsafe { entry.read_volatile() } & 0xf;

                // If the flags are nonzero, but the "Valid" bit is not 1, then this is
                // a reserved page.  Allocate a real page to back it and resume execution.
                if flags & 1 == 0 && flags != 0 {
                    let new_page = {
                        let mut mm = MemoryManagerHandle::get();
                        mm.alloc_page(pid).expect("Couldn't allocate new page")
                    };
                    let ppn1 = (new_page >> 22) & ((1 << 12) - 1);
                    let ppn0 = (new_page >> 12) & ((1 << 10) - 1);
                    unsafe {
                        entry.write_volatile((ppn1 << 20) | (ppn0 << 10) |
                        (flags | (1 << 0) /* valid */ | (1 << 4) /* USER */ | (1 << 6) /* D */ | (1 << 7) /* A */))
                    };
                    unsafe { flush_mmu() };

                    // Zero-out the page
                    // unsafe { (new_page as *mut usize).write_bytes(0, PAGE_SIZE / core::mem::size_of::<usize>()) };

                    let mut process = ProcessHandle::get();
                    crate::arch::syscall::resume(current_pid() == 1, process.current_context());
                }
            }
            RiscvException::InstructionPageFault(RETURN_FROM_ISR, _offset) => {
                unsafe {
                    // Restore the process context from the current process from
                    // before the time that the interrupt was executed.
                    let mut process = ProcessHandle::get();
                    process.unbank();

                    if let Some(previous_pid) = PREVIOUS_PID.take() {
                        // Switch to the previous process' address space.
                        println!("<<< Resuming previous pid {}", previous_pid);
                        SystemServicesHandle::get()
                            .resume_pid(previous_pid, ProcessState::Ready)
                            .expect("unable to resume previous PID");

                        // Re-enable interrupts now that they're handled
                        enable_all_irqs();

                        let mut process = ProcessHandle::get();
                        crate::arch::syscall::resume(current_pid() == 1, process.current_context());
                    }
                }
                println!("Got an instruction page fault with no previous PID");
            }
            _ => (),
        }
        println!("CPU Exception on PID {}: {}", pid, ex);
        loop {}
    } else {
        let irqs_pending = sip::read();
        // Safe to access globals since interrupts are disabled
        // when this function runs.
        unsafe {
            if PREVIOUS_PID.is_none() {
                PREVIOUS_PID = Some(pid);
            }
        }
        println!(">>> HANDLING IRQ {:08x} (current PID: {})", irqs_pending, pid);
        crate::irq::handle(irqs_pending).expect("Couldn't handle IRQ");
        let mut process = ProcessHandle::get();
        println!(">>> returning to pid {}", current_pid());
        crate::arch::syscall::resume(current_pid() == 1, process.trap_context());
    }
}
