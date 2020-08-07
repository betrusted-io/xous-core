#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
extern crate bitflags;

#[cfg(baremetal)]
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
use xous::*;

#[cfg(baremetal)]
use core::panic::PanicInfo;
#[cfg(baremetal)]
#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC in PID {}!", crate::arch::current_pid());
    println!("Details: {:?}", _arg);
    loop {
        arch::idle();
    }
}

#[cfg(baremetal)]
#[no_mangle]
/// This function is called from baremetal startup code to initialize various kernel structures
/// based on arguments passed by the bootloader. It is unused when running under an operating system.
pub extern "C" fn init(arg_offset: *const u32, init_offset: *const u32, rpt_offset: *mut u32) {
    unsafe { args::KernelArguments::init(arg_offset) };
    let args = args::KernelArguments::get();
    // Everything needs memory, so the first thing we should do is initialize the memory manager.
    {
        use mem::MemoryManagerHandle;
        let mut memory_manager = MemoryManagerHandle::get();
        memory_manager
            .init_from_memory(rpt_offset, &args)
            .expect("couldn't initialize memory manager");
    }
    SystemServices::with_mut(|system_services| system_services.init_from_memory(init_offset, &args));

    // Now that the memory manager is set up, perform any arch-specific initializations.
    arch::init();

    // Either map memory using a syscall, or if we're debugging the syscall
    // handler then directly map it.
    #[cfg(feature = "debug-print")]
    {
        // Map the serial port so println!() works as expected.
        use mem::MemoryManagerHandle;
        let mut memory_manager = MemoryManagerHandle::get();
        memory_manager
            .map_range(
                0xF0002000 as *mut usize,
                ((debug::SUPERVISOR_UART.base as u32) & !4095) as *mut usize,
                4096,
                1,
                MemoryFlags::R | MemoryFlags::W,
                MemoryType::Default,
            )
            .expect("unable to map serial port");
        println!("KMAIN: Supervisor mode started...");
        debug::SUPERVISOR_UART.enable_rx();
        println!("Claiming IRQ 3 via syscall...");
        xous::claim_interrupt(3, debug::irq, 0 as *mut usize).expect("Couldn't claim interrupt 3");
        print!("}} ");

        // Print the processed kernel arguments
        let args = args::KernelArguments::get();
        println!("Kernel arguments:");
        for arg in args.iter() {
            println!("    {}", arg);
        }
    }}

/// Loop through the SystemServices list to determine the next PID to be run.
/// If no process is ready, return `None`.
fn next_pid_to_run(last_pid: Option<PID>) -> Option<PID> {
    // PIDs are 1-indexed but arrays are 0-indexed.  By not subtracting
    // 1 from the PID when we use it as an array index, we automatically
    // pick the next process in the list.
    let current_pid = last_pid.unwrap_or(unsafe { PID::new_unchecked(1)}).get() as usize;

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
    loop {
        arch::irq::disable_all_irqs();
        pid = next_pid_to_run(pid);
        arch::irq::enable_all_irqs();

        match pid {
            Some(pid) => {
                // println!("Attempting to switch to PID {}", pid);
                xous::rsyscall(xous::SysCall::SwitchTo(pid, 0)).expect("couldn't switch to pid");
            }
            None => {
                // println!("No runnable tasks found.  Entering idle state...");
                // Special case for testing: idle can return `false` to indicate exit
                if ! arch::idle() {
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
