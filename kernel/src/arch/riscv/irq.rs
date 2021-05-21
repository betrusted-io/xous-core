// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::arch::current_pid;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::Process as ArchProcess;
use crate::arch::process::{Thread, EXIT_THREAD, RETURN_FROM_ISR};
use crate::services::SystemServices;
use riscv::register::{scause, sepc, sstatus, stval, vexriscv::sim, vexriscv::sip};
use xous_kernel::{SysCall, PID, TID};

extern "Rust" {
    fn _xous_syscall_return_result(result: &xous_kernel::Result, context: &Thread) -> !;
}

// use RAM-based backing so this variable is automatically saved on suspend
static mut SIM_BACKING: usize = 0;
/// Disable external interrupts
pub fn disable_all_irqs() {
    unsafe { SIM_BACKING = sim::read() };
    sim::write(0x0);
}

/// Enable external interrupts
#[export_name = "_enable_all_irqs"]
pub extern "C" fn enable_all_irqs() {
    sim::write(unsafe { SIM_BACKING });
}

pub fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    sim::write(sim::read() | (1 << irq_no));
}

pub fn disable_irq(irq_no: usize) -> Result<(), xous_kernel::Error> {
    sim::write(sim::read() & !(1 << irq_no));
    Ok(())
}

static mut PREVIOUS_PAIR: Option<(PID, TID)> = None;

pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) {
    PREVIOUS_PAIR = Some((pid, tid));
}

// #[allow(dead_code)]
// pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> {
//     PREVIOUS_PAIR.take()
// }

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

    let pid = current_pid();

    if (sc.bits() == 9) || (sc.bits() == 8) {
        // We got here because of an `ecall` instruction.  When we return, skip
        // past this instruction.  If this is a call such as `SwitchTo`, then we
        // will want to adjust the return value of the current process prior to
        // performing the switch in order to avoid constantly executing the same
        // instruction.
        let tid = ArchProcess::with_current_mut(|p| {
            p.current_thread_mut().sepc += 4;
            p.current_tid()
        });
        let call = SysCall::from_args(a0, a1, a2, a3, a4, a5, a6, a7).unwrap_or_else(|_| {
            ArchProcess::with_current_mut(|p| unsafe {
                _xous_syscall_return_result(
                    &xous_kernel::Result::Error(xous_kernel::Error::UnhandledSyscall),
                    p.current_thread(),
                )
            })
        });

        let response = crate::syscall::handle(pid, tid, unsafe { PREVIOUS_PAIR.is_some() }, call)
            .unwrap_or_else(|e| xous_kernel::Result::Error(e));

        // println!("Syscall Result: {:?}", response);
        ArchProcess::with_current_mut(|p| {
            let thread = p.current_thread();
            // If we're resuming a process that was previously sleeping, restore the
            // context. Otherwise, keep the context the same but pass the return
            // values in 8 return registers.
            if response == xous_kernel::Result::ResumeProcess {
                crate::arch::syscall::resume(current_pid().get() == 1, thread);
            } else {
                // println!("Returning to address {:08x}", thread.sepc);
                unsafe { _xous_syscall_return_result(&response, thread) };
            }
        });
    }

    use crate::arch::exception::RiscvException;
    let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
    // println!("ex: {:?}", ex);
    if sc.is_exception() {
        // If the CPU tries to store, look for a "reserved page" and provide
        // it with one if necessary.
        match ex {
            RiscvException::StorePageFault(pc, addr) | RiscvException::LoadPageFault(pc, addr) => {
                #[cfg(all(feature = "debug-print", feature = "print-panics"))]
                print!(
                    "KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ",
                    pid, ex, pc, addr
                );
                crate::arch::mem::ensure_page_exists_inner(addr)
                    .map(|_new_page| {
                        #[cfg(all(feature = "debug-print", feature = "print-panics"))]
                        println!("Handing page {:08x} to process", _new_page);
                        ArchProcess::with_current_mut(|process| {
                            crate::arch::syscall::resume(
                                current_pid().get() == 1,
                                process.current_thread(),
                            )
                        });
                    })
                    .expect("Couldn't allocate page");

                #[cfg(all(not(feature = "debug-print"), feature = "print-panics"))]
                print!(
                    "KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ",
                    pid, ex, pc, addr
                );
                println!("Page was not allocated");
            }
            RiscvException::InstructionPageFault(EXIT_THREAD, _offset) => {
                let tid = ArchProcess::with_current(|process| process.current_tid());

                // This address indicates a thread has exited. Destroy the thread.
                // This activates another thread within this process.
                if SystemServices::with_mut(|ss| ss.destroy_thread(pid, tid)).unwrap() {
                    crate::syscall::reset_switchto_caller();
                }

                // Resume the new thread within the same process.
                ArchProcess::with_current_mut(|p| {
                    crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
                });
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
                // println!(
                //     "ISR: Resuming previous pair of ({}, {})",
                //     previous_pid, previous_context
                // );
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
        ArchProcess::with_current(|process| {
            println!("Current thread {}:", process.current_tid());
            process.print_current_thread();
        });
        MemoryMapping::current().print_map();
        loop {}
    } else {
        let irqs_pending = sip::read();
        // println!("irqs: {:x}", irqs_pending);

        // Safe to access globals since interrupts are disabled
        // when this function runs.
        unsafe {
            if PREVIOUS_PAIR.is_none() {
                let tid = crate::arch::process::current_tid();
                PREVIOUS_PAIR = Some((pid, tid));
            }
        }
        crate::irq::handle(irqs_pending).expect("Couldn't handle IRQ");
        ArchProcess::with_current_mut(|process| {
            crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
        })
    }
}
