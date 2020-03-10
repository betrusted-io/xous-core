#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate vexriscv;

#[macro_use]
extern crate bitflags;

extern crate xous;

#[macro_use]
mod debug;

#[cfg(test)]
mod test;

mod arch;

#[macro_use]
mod args;
mod irq;
mod mem;
mod processtable;
mod syscall;

use mem::MemoryManagerHandle;
use processtable::SystemServicesHandle;
use xous::*;

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC in PID {}!", crate::arch::current_pid());
    println!("Details: {:?}", _arg);
    loop {}
}

#[no_mangle]
pub extern "C" fn init(arg_offset: *const u32, init_offset: *const u32, rpt_offset: *mut u32) {
    unsafe { args::KernelArguments::init(arg_offset) };
    let args = args::KernelArguments::get();
    {
        let mut memory_manager = MemoryManagerHandle::get();
        memory_manager
            .init(rpt_offset, &args)
            .expect("couldn't initialize memory manager");
    }
    // Everything needs memory, so the first thing we should do is initialize the memory manager.
    {
        let mut system_services = SystemServicesHandle::get();
        system_services.init(init_offset, &args);
    }

    // Now that the memory manager is set up, perform any arch-specific initializations.
    arch::init();
}

#[no_mangle]
pub extern "C" fn main() {
    // Either map memory using a syscall, or if we're debugging the syscall
    // handler then directly map it.
    #[cfg(feature = "debug-print")]
    {
        // xous::rsyscall(xous::SysCall::MapMemory(
        //     0xF0002000 as *mut usize,
        //     debug::SUPERVISOR_UART.base,
        //     4096,
        //     xous::MemoryFlags::R | xous::MemoryFlags::W,
        // ))
        // .unwrap();
        let mut memory_manager = MemoryManagerHandle::get();
        memory_manager
            .map_range(
                0xF0002000 as *mut usize,
                ((debug::SUPERVISOR_UART.base as u32) & !4095) as *mut usize,
                4096,
                MemoryFlags::R | MemoryFlags::W,
            )
            .expect("unable to map serial port");
        println!("KMAIN: Supervisor mode started...");
        debug::SUPERVISOR_UART.enable_rx();
        println!("Claiming IRQ 3 via syscall...");
        xous::rsyscall(xous::SysCall::ClaimInterrupt(
            3,
            debug::irq as *mut usize,
            0 as *mut usize,
        ))
        .expect("Couldn't claim interrupt 3");
        print!("}} ");
    }

    #[cfg(feature = "debug-print")]
    {
        let args = args::KernelArguments::get();
        println!("Kernel arguments:");
        for arg in args.iter() {
            println!("    {}", arg);
        }
    }

    loop {
        let mut next_pid = None;
        {
            arch::irq::disable_all_irqs();
            {
                let system_services = SystemServicesHandle::get();
                for (pid_idx, process) in system_services.processes.iter().enumerate() {
                    // If this process is owned by the kernel, and if it can be run, run it.
                    if process.ppid == 1 && process.runnable() {
                        println!("PID {} is owned by PID 1, and is runnable", pid_idx + 1);
                        next_pid = Some(pid_idx as PID + 1);
                        break;
                    }
                }
            }
            arch::irq::enable_all_irqs();
        }

        match next_pid {
            Some(pid) => {
                println!("Attempting to switch to PID {}", pid);
                xous::rsyscall(xous::SysCall::SwitchTo(pid, 0))
                    .expect("couldn't switch to pid");
                ()
            }
            None => {
                println!("No runnable tasks found.  Zzz...");
                unsafe { vexriscv::asm::wfi() };
            }
        }
    }
}
