// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::arch::asm;
use core::convert::TryInto;

use xous_kernel::{arch::Arguments, PID, TID};

use crate::arch::process::{
    current_pid, EXIT_THREAD, RETURN_FROM_EXCEPTION_HANDLER, RETURN_FROM_ISR,
};
use crate::services::{ArchProcess, Thread};
use crate::SystemServices;

extern "Rust" {
    fn _xous_syscall_return_result(context: &Thread) -> !;
}

static mut PREVIOUS_PAIR: Option<(PID, TID)> = None;

pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) {
    PREVIOUS_PAIR = Some((pid, tid));
}

#[cfg(feature = "gdb-stub")]
pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> {
    PREVIOUS_PAIR.take()
}

/// Disable external interrupts
pub fn disable_all_irqs() {
    unsafe {
        asm!("mrs r0, cpsr", "orr r0, r0, #0xc0", "msr cpsr_c, r0",);
    }
}

/// Switches to the kernel memory space if called from a user's memory space
/// in order to allow operations on the AIC that's only mapped in the kernel.
fn with_kernel_aic<F>(f: F)
where
    F: FnOnce() -> (),
{
    SystemServices::with_mut(|ss| {
        let pid_idx = (ss.current_pid().get() - 1) as usize;
        let not_in_kernel_mem_space = pid_idx != 0;
        let curr_process = ss.processes[pid_idx];

        if not_in_kernel_mem_space {
            klog!("Switched to the kernel mem space to access AIC");
            let kernel_mem_space = ss.processes[0].mapping;
            kernel_mem_space
                .activate()
                .expect("can't activate kernel memory mapping");
        }

        f();

        if not_in_kernel_mem_space {
            klog!("Switched back from the kernel mem space after accessing AIC");
            let curr_process_mem_space = curr_process.mapping;
            curr_process_mem_space
                .activate()
                .expect("can't activate memory mapping");
        }

        Ok::<(), ()>(())
    })
    .expect("can't access system services");
}

pub fn enable_irq(irq_no: usize) {
    klog!("Enabling IRQ #{}", irq_no);

    let irq_no = irq_no.try_into().expect("invalid IRQ number");
    with_kernel_aic(|| {
        crate::platform::atsama5d2::aic::set_irq_enabled(irq_no, true);
    });
}

pub fn disable_irq(irq_no: usize) -> Result<(), xous_kernel::Error> {
    klog!("Disabling IRQ #{}", irq_no);

    let irq_no = irq_no.try_into().expect("invalid IRQ number");
    with_kernel_aic(|| {
        crate::platform::atsama5d2::aic::set_irq_enabled(irq_no, false);
    });

    Ok(())
}

#[no_mangle]
#[export_name = "_swi_handler_rust"]
pub extern "C" fn swi_handler(arg_addr: usize) {
    // The arguments structure pointer is passed from swi handler via `r0` register (the first fn argument)
    let args = unsafe { &mut *(arg_addr as *mut Arguments) };
    let pid = current_pid();
    let tid = ArchProcess::with_current_mut(|p| {
        let tid = p.current_tid();
        let _thread = p.current_thread();
        klog!("Current thread {}:{} context: {:x?}", pid, tid, _thread);

        tid
    });

    klog!("Handling syscall | args = ({:08x}) {:x?}", arg_addr, args,);

    let call = args.as_syscall().unwrap_or_else(|_| {
        ArchProcess::with_current_mut(|p| unsafe {
            klog!("[!] Invalid syscall");

            args.set_result(&xous_kernel::Result::Error(
                xous_kernel::Error::UnhandledSyscall,
            ));
            _xous_syscall_return_result(&p.current_thread());
        })
    });

    let response = crate::syscall::handle(pid, tid, unsafe { PREVIOUS_PAIR.is_some() }, call)
        .unwrap_or_else(xous_kernel::Result::Error);

    klog!("Syscall Result: {:x?}", response);

    ArchProcess::with_current_mut(|p| {
        // If we're resuming a process that was previously sleeping, restore the
        // thread context. Otherwise, keep the thread context the same and pass
        // the return values in 8 argument registers.
        if response == xous_kernel::Result::ResumeProcess {
            let thread = p.current_thread();
            klog!(
                "Resuming {}:{}: {:x?}",
                current_pid(),
                p.current_tid(),
                thread
            );
            crate::arch::syscall::resume(current_pid().get() == 1, thread);
        } else {
            p.set_thread_result(p.current_tid(), response);

            let thread = p.current_thread();
            klog!(
                "Resuming {}:{}: {:x?}",
                current_pid(),
                p.current_tid(),
                thread
            );
            klog!("Returning to address {:08x}", thread.resume_addr);

            unsafe { _xous_syscall_return_result(&thread) };
        }
    });
}

fn read_fault_cause() -> (usize, usize, usize, usize) {
    // Read fault status (DFSR, IFSR) and cause address (DFAR, IFAR) registers
    let mut dfar: usize;
    let mut ifar: usize;
    let mut dfsr: usize;
    let mut ifsr: usize;
    unsafe {
        asm!(
            "mrc p15, 0, {dfar}, c6, c0, 0",
            "mrc p15, 0, {ifar}, c6, c0, 2",
            "mrc p15, 0, {dfsr}, c5, c0, 0",
            "mrc p15, 0, {ifsr}, c5, c0, 1",
            dfar = out(reg) dfar,
            ifar = out(reg) ifar,
            dfsr = out(reg) dfsr,
            ifsr = out(reg) ifsr,
        );
    }

    (dfar, ifar, dfsr, ifsr)
}

fn clear_fault() {
    let zero = 0;
    unsafe {
        asm!(
            "mcr p15, 0, {dfar}, c6, c0, 0",
            "mcr p15, 0, {ifar}, c6, c0, 2",
            "mcr p15, 0, {dfsr}, c5, c0, 0",
            "mcr p15, 0, {ifsr}, c5, c0, 1",
            dfar = in(reg) zero,
            ifar = in(reg) zero,
            dfsr = in(reg) zero,
            ifsr = in(reg) zero,
        );
    }
}

/// Terminates the specified process due to a crash or violation.
fn crash_process(pid: PID) {
    SystemServices::with_mut(|ss| {
        ss.terminate_process(pid)
            .expect("couldn't terminate the process");
        crate::syscall::reset_switchto_caller();
    });

    // Resume the parent process.
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}

#[no_mangle]
#[export_name = "_abort_handler_rust"]
pub extern "C" fn abort_handler() {
    let (dfar, ifar, dfsr, ifsr) = read_fault_cause();

    // See ARM ARM Table B3-12 VMSAv7 DFSR encodings
    let dfsr_fault_cause = dfsr & 0b1111;
    let is_data_translation_page_fault = dfsr_fault_cause == 0b0111;
    let is_data_alignment_fault = dfsr_fault_cause == 0b0001;
    let is_data_permission_fault = dfsr_fault_cause == 0b0110
        || dfsr_fault_cause == 0b0011
        || dfsr_fault_cause == 0b0101
        || dfsr_fault_cause == 0b1101
        || dfsr_fault_cause == 0b1111;
    let ifsr_fault_cause = ifsr & 0b1111;
    let is_null_pointer_exception =
        dfar == 0 && (is_data_permission_fault || ifsr_fault_cause == 0b0101);
    let pid = current_pid();

    klog!(
        "KERNEL({}): ABORT | addrD {:08x} addrI: {:08x}, causeD: {:04b} causeI: {:04b}",
        pid,
        dfar,
        ifar,
        dfsr_fault_cause,
        ifsr_fault_cause,
    );

    match ifar {
        // Fault caused by returning from exception handler
        RETURN_FROM_EXCEPTION_HANDLER => {
            SystemServices::with_mut(|ss| {
                ss.finish_exception_handler_and_resume(pid)
                    .expect("unable to finish exception handler")
            });

            // Resume the new thread within the same process.
            ArchProcess::with_current_mut(|p| {
                // Adjust the program counter by the amount returned by the exception handler
                /* FIXME let pc_adjust = a0 as isize;
                if pc_adjust < 0 {
                    p.current_thread_mut().sepc -= pc_adjust.abs() as usize;
                } else {
                    p.current_thread_mut().sepc += pc_adjust.abs() as usize;
                }*/

                clear_fault();

                crate::arch::syscall::resume(pid.get() == 1, p.current_thread())
            });
        }

        EXIT_THREAD if dfar == 0 => {
            println!("[!] Thread exit requested");
            let tid = ArchProcess::with_current(|process| process.current_tid());

            // This address indicates a thread has exited. Destroy the thread.
            // This activates another thread within this process.
            if SystemServices::with_mut(|ss| ss.destroy_thread(pid, tid)).unwrap() {
                crate::syscall::reset_switchto_caller();
            }

            clear_fault();

            // Resume the new thread within the same process.
            ArchProcess::with_current_mut(|p| {
                crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
            });
        }

        RETURN_FROM_ISR if dfar == 0 => {
            // If we hit this address, then an ISR has just returned.  Since
            // we're in an interrupt context, it is safe to access this
            // global variable.
            let (previous_pid, previous_context) = unsafe {
                PREVIOUS_PAIR
                    .take()
                    .expect("got RETURN_FROM_ISR with no previous PID")
            };
            klog!(
                "ISR: Resuming previous pair of ({}, {})",
                previous_pid,
                previous_context
            );
            // Switch to the previous process' address space.
            SystemServices::with_mut(|ss| {
                ss.finish_callback_and_resume(previous_pid, previous_context)
                    .expect("unable to resume previous PID")
            });

            clear_fault();

            ArchProcess::with_current_mut(|process| {
                let mut curr_thread = process.current_thread_mut();
                curr_thread.resume_addr -= 4;
                curr_thread.ret_addr = 0;
                crate::arch::syscall::resume(current_pid().get() == 1, curr_thread)
            });
        }

        _ => {
            if is_data_translation_page_fault {
                crate::arch::mem::ensure_page_exists_inner(dfar)
                    .map(|_new_page| {
                        klog!("Handing page {:08x} to process", _new_page);

                        clear_fault();

                        ArchProcess::with_current_mut(|process| {
                            let thread = process.current_thread_mut();

                            // Clear the return address to avoid corrupting thread's LR
                            if thread.ret_addr != 0 {
                                thread.ret_addr = 0;
                            }

                            // Retry the instruction that caused abort
                            process.current_thread_mut().resume_addr -= 8;

                            crate::arch::syscall::resume(pid.get() == 1, process.current_thread())
                        });
                    })
                    .ok(); // If this fails, fall through.
            }
            if is_null_pointer_exception {
                println!(
                    "[!] Process PID {} accessed 0x00000000 address (null pointer)",
                    pid
                );
                crash_process(pid);
            } else if is_data_alignment_fault || is_data_permission_fault {
                println!("[!] Data alignment or access permissions violation");
                println!("[!] PID: {}, address: {:08x}", pid, dfar);

                crash_process(pid);
            } else {
                println!("[!] Unhandled prefetch abort fault!");
            }
        }
    }
}

#[no_mangle]
#[export_name = "_irq_handler_rust"]
pub extern "C" fn _irq_handler_rust() {
    klog!("Entered irq handler");

    let pid = current_pid();

    let mut irqs_pending = 0;
    with_kernel_aic(|| {
        irqs_pending = crate::platform::atsama5d2::aic::get_pending_irqs();
        for _ in 0..irqs_pending.count_ones() {
            crate::platform::atsama5d2::aic::acknowledge_irq();
        }
    });
    klog!("Pending irqs mask: {:032b}", irqs_pending);

    // Safe to access globals since interrupts are disabled
    unsafe {
        if PREVIOUS_PAIR.is_none() {
            let tid = crate::arch::process::current_tid();
            set_isr_return_pair(pid, tid);
        }
    }
    crate::irq::handle(irqs_pending).expect("Couldn't handle IRQ");
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}
