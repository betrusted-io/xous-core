// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use riscv::register::{scause, sepc, sstatus, stval};
use xous_kernel::{SysCall, PID, TID};

use crate::arch::current_pid;
use crate::arch::exception::RiscvException;
use crate::arch::mem::MemoryMapping;
#[cfg(feature = "swap")]
use crate::arch::process::RETURN_FROM_SWAPPER;
use crate::arch::process::{Process as ArchProcess, RETURN_FROM_EXCEPTION_HANDLER};
use crate::arch::process::{Thread, EXIT_THREAD, RETURN_FROM_ISR};
use crate::services::SystemServices;
#[cfg(feature = "swap")]
use crate::swap::Swap;

extern "Rust" {
    fn _xous_syscall_return_result(result: &xous_kernel::Result, context: &Thread) -> !;
}

// use RAM-based backing so this variable is automatically saved on suspend
static SIM_BACKING: AtomicUsize = AtomicUsize::new(0);

// Interrupts are enabled very early on, so just assume they're on by default
static IRQ_ENABLED: AtomicBool = AtomicBool::new(true);

// Indicate when we handle an IRQ
static HANDLING_IRQ: AtomicBool = AtomicBool::new(false);

fn sim_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0x9C0, zero", out(reg) existing) };
    existing
}

fn sim_write(new: usize) { unsafe { core::arch::asm!("csrrw zero, 0x9C0, {0}", in(reg) new) }; }

fn sip_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0xDC0, zero", out(reg) existing) };
    existing
}

/// Disable external interrupts
pub fn disable_all_irqs() {
    SIM_BACKING.store(sim_read(), Ordering::Relaxed);
    IRQ_ENABLED.store(false, Ordering::Relaxed);
    sim_write(0x0);
}

/// Enable external interrupts
#[export_name = "_enable_all_irqs"]
pub extern "C" fn enable_all_irqs() {
    IRQ_ENABLED.store(true, Ordering::Relaxed);
    sim_write(SIM_BACKING.load(Ordering::Relaxed));
}

/// Enable a given IRQ. If interrupts are currently disabled, then update the
/// SIM backing instead so that it will be enabled when interrupts are restored.
pub fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    if IRQ_ENABLED.load(Ordering::Relaxed) {
        sim_write(sim_read() | (1 << irq_no));
    } else {
        SIM_BACKING
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |existing| Some(existing | (1 << irq_no)))
            .ok();
    }
}

/// Disable a given IRQ. If interrupts are currently disabled, then update the
/// SIM backing instead so that it will be disabled when interrupts are restored.
pub fn disable_irq(irq_no: usize) {
    if IRQ_ENABLED.load(Ordering::Relaxed) {
        sim_write(sim_read() & !(1 << irq_no));
    } else {
        SIM_BACKING
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |existing| Some(existing & !(1 << irq_no)))
            .ok();
    }
}

static mut PREVIOUS_PAIR: Option<(PID, TID)> = None;

pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) { PREVIOUS_PAIR = Some((pid, tid)); }

#[cfg(feature = "gdb-stub")]
pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> { PREVIOUS_PAIR.take() }

/// Finish a pending ISR. Return `false` if there was none.
fn finish_isr() -> bool {
    if !HANDLING_IRQ.swap(false, Ordering::Relaxed) {
        return false;
    }

    // If we hit this address, then an ISR has just returned.  Since
    // we're in an interrupt context, it is safe to access this
    // global variable.
    let (previous_pid, previous_context) =
        unsafe { PREVIOUS_PAIR.take().expect("got RETURN_FROM_ISR with no previous PID") };
    // println!(
    //     "ISR: Resuming previous pair of ({}, {})",
    //     previous_pid, previous_context
    // );
    // Switch to the previous process' address space.
    SystemServices::with_mut(|ss| {
        ss.finish_callback_and_resume(previous_pid, previous_context).expect("unable to resume previous PID")
    });

    // Re-enable interrupts now that they're handled
    enable_all_irqs();

    true
}

/// Convert a RISC-V `Exception` into a Xous exception argument list.
fn generate_exception_args(ex: &RiscvException) -> Option<[usize; 3]> {
    match *ex {
        RiscvException::InstructionAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionAddressMisaligned as usize, epc, addr])
        }
        RiscvException::InstructionAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionAccessFault as usize, epc, addr])
        }
        RiscvException::IllegalInstruction(epc, instruction) => {
            Some([xous_kernel::ExceptionType::IllegalInstruction as usize, epc, instruction])
        }
        RiscvException::LoadAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadAddressMisaligned as usize, epc, addr])
        }
        RiscvException::LoadAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadAccessFault as usize, epc, addr])
        }
        RiscvException::StoreAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::StoreAddressMisaligned as usize, epc, addr])
        }
        RiscvException::StoreAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::StoreAccessFault as usize, epc, addr])
        }
        RiscvException::InstructionPageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionPageFault as usize, epc, addr])
        }
        RiscvException::LoadPageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadPageFault as usize, epc, addr])
        }
        RiscvException::StorePageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::StorePageFault as usize, epc, addr])
        }
        _ => None,
    }
}

/// Trap entry point rust (_start_trap_rust)
///
/// scause is read to determine the cause of the trap. The top bit indicates if
/// it's an interrupt or an exception. The result is converted to an element of
/// the Interrupt or Exception enum and passed to handle_interrupt or
/// handle_exception.
#[export_name = "_start_trap_rust"]
#[allow(unreachable_code)] // panic handler will terminate execution
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
    if cfg!(target_arch = "riscv32") && sstatus::read().spp() == sstatus::SPP::Supervisor && sc.bits() == 0xf
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        panic!("KERNEL({}): RISC-V fault: {} - maybe ran out of kernel stack?", pid, ex);
    }

    let pid = current_pid();
    let epc = sepc::read();

    let ex = RiscvException::from_regs(sc.bits(), epc, stval::read());
    #[cfg(feature = "debug-print")]
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        println!("IRQ -- KERNEL({}): RISC-V fault: {}", pid, ex);
    }
    match ex {
        // Syscall
        RiscvException::CallFromSMode(_epc, _) | RiscvException::CallFromUMode(_epc, _) => {
            // We got here because of an `ecall` instruction, either from User mode (sc==8)
            // or from Supervisor mode (sc==9).  When we return, skip past the `ecall`
            // instruction.
            // If this is a call such as `SwitchTo`, then we will want to adjust the return
            // value of the current process prior to performing the switch in order to
            // avoid constantly executing the same instruction.
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
                .unwrap_or_else(xous_kernel::Result::Error);

            // println!("Syscall Result: {:?}", response);
            ArchProcess::with_current_mut(|p| {
                let thread = p.current_thread();
                // If we're resuming a process that was previously sleeping, restore the
                // thread context. Otherwise, keep the thread context the same and pass
                // the return values in 8 argument registers.
                if response == xous_kernel::Result::ResumeProcess {
                    crate::arch::syscall::resume(current_pid().get() == 1, thread);
                } else {
                    // println!("Returning to address {:08x}", thread.sepc);
                    unsafe { _xous_syscall_return_result(&response, thread) };
                }
            });
        }
        // Hardware interrupt
        RiscvException::UserExternalInterrupt(_) | RiscvException::SupervisorExternalInterrupt(_) => {
            let irqs_pending = sip_read() & sim_read();

            // Safe to access globals since interrupts are disabled
            // when this function runs.
            unsafe {
                if PREVIOUS_PAIR.is_none() {
                    let tid = crate::arch::process::current_tid();
                    PREVIOUS_PAIR = Some((pid, tid));
                }
            }
            HANDLING_IRQ.store(true, Ordering::Relaxed);
            crate::irq::handle(irqs_pending).expect("Couldn't handle IRQ");
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
            })
        }

        // See if it's a known exception, such as writing to a demand-paged area
        // or returning from a handler or thread. If so, handle the exception
        // and return right away.
        RiscvException::StorePageFault(_pc, addr) | RiscvException::LoadPageFault(_pc, addr) => {
            #[cfg(all(feature = "debug-print", feature = "print-panics"))]
            println!("KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ", pid, ex, _pc, addr);
            crate::arch::mem::ensure_page_exists_inner(addr)
                .map(|_new_page| {
                    #[cfg(all(feature = "debug-print", feature = "print-panics"))]
                    println!("SPF Handing page {:08x} to process", _new_page);
                    ArchProcess::with_current_mut(|process| {
                        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                    });
                })
                .ok(); // If this fails, fall through.
        }

        RiscvException::InstructionPageFault(RETURN_FROM_EXCEPTION_HANDLER, _offset) => {
            // This address indicates the exception handler
            SystemServices::with_mut(|ss| {
                ss.finish_exception_handler_and_resume(pid).expect("unable to finish exception handler")
            });

            // TODO: Handle the case where this happens in an ISR
            // finish_isr();

            // Resume the new thread within the same process.
            ArchProcess::with_current_mut(|p| {
                // Adjust the program counter by the amount returned by the exception handler
                let pc_adjust = a0 as isize;
                if pc_adjust < 0 {
                    p.current_thread_mut().sepc -= pc_adjust.abs() as usize;
                } else {
                    p.current_thread_mut().sepc += pc_adjust.abs() as usize;
                }

                crate::arch::syscall::resume(pid.get() == 1, p.current_thread())
            });
        }

        RiscvException::InstructionPageFault(EXIT_THREAD, _offset) => {
            let tid = ArchProcess::with_current(|process| process.current_tid());

            // This address indicates a thread has exited. Destroy the thread.
            // This activates another thread within this process.
            if SystemServices::with_mut(|ss| ss.destroy_thread(pid, tid)).unwrap() {
                crate::syscall::reset_switchto_caller();
            }

            // Now that the thread is destroyed, switch to a different process if
            // we're in an interrupt handler.
            finish_isr();

            // Resume the new thread within the same process.
            ArchProcess::with_current_mut(|p| {
                crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
            });
        }

        RiscvException::InstructionPageFault(RETURN_FROM_ISR, _offset) => {
            finish_isr();
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
            });
        }
        #[cfg(feature = "swap")]
        RiscvException::InstructionPageFault(RETURN_FROM_SWAPPER, _offset) => {
            #[cfg(feature = "debug-swap")]
            {
                let pid = crate::arch::process::current_pid();
                let hardware_pid = (riscv::register::satp::read().bits() >> 22) & ((1 << 9) - 1);
                println!(
                    "RETURN_FROM_SWAPPER PROCESS_TABLE.current: {}, hw_pid: {}",
                    pid.get(),
                    hardware_pid
                );
            }
            // Cleanup after the swapper
            let response = Swap::with_mut(|s|
                // safety: this is safe because on return from swapper, we're in the swapper's memory space.
                unsafe { s.exit_blocking_call() })
            .unwrap_or_else(xous_kernel::Result::Error);

            #[cfg(feature = "debug-swap")]
            {
                let pid = crate::arch::process::current_pid();
                let hardware_pid = (riscv::register::satp::read().bits() >> 22) & ((1 << 9) - 1);
                println!(
                    "aft swapper cleanup PROCESS_TABLE.current: {}, hw_pid: {}",
                    pid.get(),
                    hardware_pid
                );

                // debugging
                SystemServices::with(|ss| {
                    let current = ss.get_process(current_pid()).unwrap();
                    let state = current.state();
                    println!("state after swapper switch: {} {:?}", current.pid.get(), state);
                });
            }
            // Re-enable interrupts now that we're out of the swap context
            enable_all_irqs();

            if response == xous_kernel::Result::ResumeProcess {
                ArchProcess::with_current_mut(|process| {
                    crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                });
            } else {
                ArchProcess::with_current_mut(|p| {
                    let thread = p.current_thread();
                    #[cfg(feature = "debug-swap")]
                    println!("Returning to address {:08x}", thread.sepc);
                    unsafe { _xous_syscall_return_result(&response, thread) };
                });
            }
        }

        // Handle faulted instruction pages, because we can now actually have instruction pages that are
        // swapped out.
        #[cfg(feature = "swap")]
        RiscvException::InstructionPageFault(_pc, addr) => {
            #[cfg(all(feature = "debug-print", feature = "print-panics"))]
            println!("IPF swap KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ", pid, ex, _pc, addr);
            crate::arch::mem::ensure_page_exists_inner(addr)
                .map(|_new_page| {
                    #[cfg(all(feature = "debug-print", feature = "print-panics"))]
                    println!("IPF Handing page {:08x} to process", _new_page);
                    ArchProcess::with_current_mut(|process| {
                        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                    });
                })
                .ok(); // If this fails, fall through.
        }

        #[cfg(feature = "gdb-stub")]
        RiscvException::Breakpoint(_address) => {
            let insn_lo = crate::arch::mem::peek_memory(epc as *mut u16).unwrap_or(0xffff);
            let insn_hi = crate::arch::mem::peek_memory((epc + 2) as *mut u16).unwrap_or(0xffff);
            if (insn_lo & 0xffff == 0x9002) || (insn_hi == 0x0010 && insn_lo == 0x0073) {
                // Report that the process has stopped
                let tid = ArchProcess::with_current_mut(|process| process.current_tid());

                // Note that we report the current `epc` here without manipulation --
                // the debugger will unpatch the opcode and re-issue the instruction.
                crate::debug::gdb::report_stop(pid, tid, epc);

                // Pause for debugging, which switches to the parent process
                SystemServices::with_mut(|ss| {
                    ss.pause_process_for_debug(pid).expect("couldn't debug current process");
                    crate::syscall::reset_switchto_caller();
                });

                // Don't lock up when debugging ISRs
                finish_isr();

                // Resume the parent process.
                ArchProcess::with_current_mut(|process| {
                    crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                })
            }
        }

        _ => {
            #[cfg(not(any(feature = "precursor", feature = "renode")))]
            println!("!!! Unrecognized exception: {:?}", ex);
            #[cfg(any(feature = "precursor", feature = "renode"))]
            panic!("!!! Unrecognized exception: {:?}", ex);
        }
    }

    // This exception is not due to something we're aware of. In this case,
    // determine if there is an exception handler in this particular program
    // and call that handler if so.
    if let Some(args) = generate_exception_args(&ex) {
        if let Some(handler) = SystemServices::with_mut(|ss| ss.begin_exception_handler(pid)) {
            klog!("Exception handler for process exists ({:x?})", handler);
            // If this is the sort of exception that may be able to be handled by
            // the userspace program, generate a list of arguments to pass to
            // the handler.
            // Invoke the handler in userspace and exit this exception handler.
            klog!(
                "At start of exception, current thread was: {}",
                SystemServices::with(|ss| ss.get_process(pid).unwrap().current_thread)
            );
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::invoke(
                    process.thread_mut(crate::arch::process::EXCEPTION_TID),
                    current_pid().get() == 1,
                    handler.pc,
                    handler.sp,
                    RETURN_FROM_EXCEPTION_HANDLER,
                    &args,
                );
                crate::arch::syscall::resume(
                    current_pid().get() == 1,
                    process.thread(crate::arch::process::EXCEPTION_TID),
                )
            });
        }
    }

    let is_kernel_failure = sstatus::read().spp() == sstatus::SPP::Supervisor;
    // The exception was not handled. We should terminate the program here.
    // For now, let's halt the whole system instead so that it becomes
    // immediately obvious that we screwed up. On hardware this will trigger
    // a watchdog reset.
    #[cfg(not(any(feature = "precursor", feature = "renode")))]
    println!(
        "{}: CPU Exception on PID {}: {}",
        if is_kernel_failure { "!!! KERNEL FAILURE !!!" } else { "PROGRAM HALT" },
        pid,
        ex
    );
    #[cfg(any(feature = "precursor", feature = "renode"))]
    println!(
        "{}: CPU Exception on PID {}: {}",
        if is_kernel_failure { "!!! KERNEL FAILURE !!!" } else { "PROGRAM HALT" },
        pid,
        ex
    );
    ArchProcess::with_current(|process| {
        println!("Current thread {}:", process.current_tid());
        process.print_current_thread();
    });

    // If this is a failure in the kernel, go into an infinite loop
    MemoryMapping::current().print_map();
    if is_kernel_failure {
        #[allow(clippy::empty_loop)]
        loop {}
    }

    finish_isr();

    // If it's not a failure in the kernel, terminate or debug the current process.
    SystemServices::with_mut(|ss| {
        #[cfg(feature = "gdb-stub")]
        {
            ss.pause_process_for_debug(pid).expect("couldn't debug current process");
            crate::debug::gdb::report_terminated(pid);
            println!("Program suspended. You may inspect it using gdb.");
        }
        #[cfg(not(feature = "gdb-stub"))]
        ss.terminate_process(pid).expect("couldn't terminate current process");
        crate::syscall::reset_switchto_caller();
    });

    // Resume the parent process.
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}
