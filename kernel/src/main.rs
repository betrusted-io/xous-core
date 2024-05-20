// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[cfg_attr(not(target_arch = "arm"), macro_use)]
extern crate bitflags;

#[macro_use]
mod debug;

#[cfg(all(test, not(baremetal)))]
mod test;

mod arch;

#[macro_use]
mod args;
mod io;
mod irq;
mod macros;
mod mem;
mod platform;
mod server;
mod services;
mod syscall;
mod utils;

#[cfg(feature = "swap")]
mod swap;

use services::SystemServices;
use xous_kernel::*;

#[cfg(baremetal)]
#[no_mangle]
/// This function is called from baremetal startup code to initialize various kernel structures
/// based on arguments passed by the bootloader. It is unused when running under an operating system.
///
/// # Safety
///
/// This is safe to call only to initialize the kernel.
pub unsafe extern "C" fn init(
    arg_offset: *const u32,
    init_offset: *const u32,
    rpt_offset: usize,
    xpt_offset: usize,
) {
    args::KernelArguments::init(arg_offset);
    let args = args::KernelArguments::get();
    // Everything needs memory, so the first thing we should do is initialize the memory manager.
    crate::mem::MemoryManager::with_mut(|mm| {
        mm.init_from_memory(rpt_offset, xpt_offset, &args).expect("couldn't initialize memory manager")
    });
    SystemServices::with_mut(|system_services| system_services.init_from_memory(init_offset, &args));

    // Now that the memory manager is set up, perform any architecture and
    // platform specific initializations.
    arch::init();
    platform::init();

    println!("KMAIN (clean boot): Supervisor mode started...");

    // rand::init() already clears the initial pipe, but pump the TRNG a little more out of no other reason
    // than sheer paranoia
    platform::rand::get_u32();
    platform::rand::get_u32();
}

/// Loop through the SystemServices list to determine the next PID to be run.
/// If no process is ready, return `None`.
fn next_pid_to_run(last_pid: Option<PID>) -> Option<PID> {
    // PIDs are 1-indexed but arrays are 0-indexed.  By not subtracting
    // 1 from the PID when we use it as an array index, we automatically
    // pick the next process in the list.
    let next_pid = last_pid.map(|v| v.get() as usize).unwrap_or(1);

    SystemServices::with(|system_services| {
        for process in
            system_services.processes[next_pid..].iter().chain(system_services.processes[..next_pid].iter())
        {
            if process.runnable() {
                return Some(process.pid);
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

    #[cfg(not(any(baremetal, all(ci, test))))]
    {
        use std::panic;
        panic::set_hook(Box::new(|arg| {
            println!("Panic Details: {:?}", arg);
            // debug_here::debug_here!();
        }));
    }

    loop {
        #[cfg(feature = "debug-print")]
        let last_pid = pid;
        pid = next_pid_to_run(pid);

        match pid {
            Some(pid) => {
                #[cfg(feature = "debug-print")]
                println!("  PID{:?}->{:?}", last_pid, pid); // keep this succinct as it happens often
                xous_kernel::rsyscall(xous_kernel::SysCall::SwitchTo(pid, 0))
                    .expect("couldn't switch to pid");
            }
            None => {
                #[cfg(feature = "debug-print")]
                klog!("NO RUNNABLE TASKS FOUND, entering idle state");

                #[cfg(feature = "debug-print")]
                SystemServices::with(|system_services| {
                    for (test_idx, process) in system_services.processes.iter().enumerate() {
                        if !process.free() {
                            klog!("PID {}: {:?}", test_idx + 1, process);
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
fn main() { kmain(); }
