// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
extern crate bitflags;

#[macro_use]
mod debug;

#[cfg(all(test, not(baremetal)))]
mod test;

mod arch;

#[macro_use]
mod args;
mod irq;
mod macros;
mod mem;
mod server;
mod services;
mod syscall;

use services::SystemServices;
use xous_kernel::*;

#[cfg(baremetal)]
use core::panic::PanicInfo;
#[cfg(baremetal)]
#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC in PID {}: {}", crate::arch::current_pid(), _arg);
    loop {
        arch::idle();
    }
}

use utralib::generated::*;
pub const RESUME_KICKER: ResumeKicker = ResumeKicker {
    // the manually chosen virtual address has to be in the top 4MiB as it is the only page shared among all processes
    base: 0xffcd_0000 as *mut usize, // see https://github.com/betrusted-io/xous-core/blob/master/docs/memory.md
};

pub struct ResumeKicker {
    pub base: *mut usize,
}

#[cfg(baremetal)]
#[no_mangle]
/// This function is called from baremetal startup code to initialize various kernel structures
/// based on arguments passed by the bootloader. It is unused when running under an operating system.
pub extern "C" fn init(arg_offset: *const u32, init_offset: *const u32, rpt_offset: *mut u32, resume: bool) {
    if !resume {
        unsafe { args::KernelArguments::init(arg_offset) };
        let args = args::KernelArguments::get();
        // Everything needs memory, so the first thing we should do is initialize the memory manager.
        crate::mem::MemoryManager::with_mut(|mm| {
            mm.init_from_memory(rpt_offset, &args)
                .expect("couldn't initialize memory manager")
        });
        SystemServices::with_mut(|system_services| {
            system_services.init_from_memory(init_offset, &args)
        });
    }

    // Now that the memory manager is set up, perform any arch-specific initializations.
    // each sub-call will handle things differently depending on the resume state
    arch::init(resume);

    // Either map memory using a syscall, or if we're debugging the syscall
    // handler then directly map it.
    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    {
        use utralib::generated::*;
        if !resume {
            // Map the serial port so println!() works as expected.
            mem::MemoryManager::with_mut(|memory_manager| {
                memory_manager
                    .map_range(
                        utra::uart::HW_UART_BASE as *mut u8,
                        ((debug::SUPERVISOR_UART_ADDR as u32) & !4095) as *mut u8,
                        4096,
                        PID::new(1).unwrap(),
                        MemoryFlags::R | MemoryFlags::W,
                        MemoryType::Default,
                    )
                    .expect("unable to map serial port")
            });
            debug::Uart{}.init();
            println!("KMAIN (clean boot): Supervisor mode started...");
            println!("Claiming IRQ {} via syscall...", utra::uart::UART_IRQ);
            xous_kernel::claim_interrupt(utra::uart::UART_IRQ, debug::irq, 0 as *mut usize)
                .expect("Couldn't claim debug interrupt");
            print!("}} ");
        } else {
            println!("KMAIN (resume): Loader is instructing us to resume...");
        }

        if !resume {
            // Print the processed kernel arguments
            let args = args::KernelArguments::get();
            println!("Kernel arguments:");
            for arg in args.iter() {
                println!("    {}", arg);
            }
        }
    }

    if !resume {
        // map in the resume kicker to its fixed virtual address for the next time we start with a resume
        mem::MemoryManager::with_mut(|memory_manager| {
            memory_manager
                .map_range(
                    utra::resumekicker::HW_RESUMEKICKER_BASE as *mut u8,
                    ((RESUME_KICKER.base as u32) & !4095) as *mut u8,
                    4096,
                    PID::new(1).unwrap(),
                    MemoryFlags::R | MemoryFlags::W,
                    MemoryType::Default,
                )
                .expect("unable to map resume kicker for future resume use")
        });
    }

    // rand::init() already clears the initial pipe, but pump the TRNG a little more out of no other reason than sheer paranoia
    arch::rand::get_u32();
    arch::rand::get_u32();

    if resume {
        // this will reload the SIM from its backing store in RAM
        crate::arch::irq::enable_all_irqs();
        // we should already have our page mapped from a prior boot
        let mut resumekicker = CSR::new(RESUME_KICKER.base as *mut u32);

        // this should trigger a pending interrupt immediately
        // note that the resumekicker's "kick" signal is a wire-OR into the susres SOFT_INT bit
        // and that the loader will have set EV_ENABLE on this before handing control to the kernel
        resumekicker.wfo(utra::resumekicker::KICKER_KICK, 1);
        // when the handler returns to this closing curly brace,
        // the control flow returns to the asm.S loader
        // and the very next immediate thing it does is jump to kmain() and schedule the next PID to run!
    }
}

/// Loop through the SystemServices list to determine the next PID to be run.
/// If no process is ready, return `None`.
fn next_pid_to_run(last_pid: Option<PID>) -> Option<PID> {
    // PIDs are 1-indexed but arrays are 0-indexed.  By not subtracting
    // 1 from the PID when we use it as an array index, we automatically
    // pick the next process in the list.
    let current_pid = last_pid.unwrap_or(unsafe { PID::new_unchecked(1) }).get() as usize;

    SystemServices::with(|system_services| {
        for test_idx in current_pid..system_services.processes.len() {
            if system_services.processes[test_idx].ppid.get() == 1 {
                // print!("PID {} is owned by PID1... ", test_idx + 1);
                if system_services.processes[test_idx].runnable() {
                    // println!(" and is runnable");
                    return match pid_from_usize(test_idx + 1) {
                        Ok(x) => Some(x),
                        Err(_) => None,
                    };
                }
                // println!(" and is NOT RUNNABLE");
            }
        }
        for test_idx in 0..current_pid {
            if system_services.processes[test_idx].ppid.get() == 1 {
                // print!("PID {} is owned by PID1... ", test_idx + 1);
                if system_services.processes[test_idx].runnable() {
                    // println!(" and is runnable");
                    return match pid_from_usize(test_idx + 1) {
                        Ok(x) => Some(x),
                        Err(_) => None,
                    };
                }
                // println!(" and is NOT RUNNABLE");
            }
        }
        None
    })
}

/// Common main function for baremetal and hosted environments.
#[no_mangle]
pub extern "C" fn kmain() {
    // Start performing round-robin on all child processes.
    // Note that at this point, no new direct children of INIT may be created.
    let mut pid = None;

    #[cfg(not(target_os = "none"))]
    {
        use std::panic;
        panic::set_hook(Box::new(|arg| {
            println!("Panic Details: {:?}", arg);
            debug_here::debug_here!();
        }));
    }

    loop {
        pid = next_pid_to_run(pid);

        match pid {
            Some(pid) => {
                // #[cfg(feature = "debug-print")]
                // klog!("switching to pid {}", pid);
                xous_kernel::rsyscall(xous_kernel::SysCall::SwitchTo(pid, 0))
                    .expect("couldn't switch to pid");
            }
            None => {
                #[cfg(feature = "debug-print")]
                println!("NO RUNNABLE TASKS FOUND, entering idle state");

                #[cfg(feature = "debug-print")]
                SystemServices::with(|system_services| {
                    for (test_idx, process) in system_services.processes.iter().enumerate() {
                        if ! process.free() {
                            println!("PID {}: {:?}", test_idx + 1, process);
                        }
                    }
                });

                // Special case for testing: idle can return `false` to indicate exit
                if !arch::idle() {
                    return;
                }
            }
        }
    }
}

/// The main entrypoint when run in hosted mode. When running in embedded mode,
/// this function does not exist.
#[cfg(all(not(baremetal)))]
fn main() {
    kmain();
}
