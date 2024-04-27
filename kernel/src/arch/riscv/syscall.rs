// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{sepc, sstatus};

use crate::services::Thread;

extern "C" {
    fn _xous_resume_context(regs: *const usize) -> !;
}

pub fn invoke(thread: &mut Thread, supervisor: bool, pc: usize, sp: usize, ret_addr: usize, args: &[usize]) {
    set_supervisor(supervisor);
    thread.registers[0] = ret_addr;
    thread.registers[1] = sp;
    assert!(args.len() <= 8, "too many arguments to invoke()");
    for (idx, arg) in args.iter().enumerate() {
        thread.registers[9 + idx] = *arg;
    }
    thread.sepc = pc;
}

fn set_supervisor(supervisor: bool) {
    if supervisor {
        unsafe { sstatus::set_spp(sstatus::SPP::Supervisor) };
    } else {
        unsafe { sstatus::set_spp(sstatus::SPP::User) };
    }
}

pub fn resume(supervisor: bool, thread: &Thread) -> ! {
    sepc::write(thread.sepc);

    // Return to the appropriate CPU mode
    set_supervisor(supervisor);
    #[cfg(feature = "debug-print")]
    println!(
        "Switching to PID {}, SP: {:08x}, PC: {:08x}",
        crate::arch::current_pid(),
        thread.registers[1],
        thread.sepc,
    );
    unsafe { _xous_resume_context(thread.registers.as_ptr()) };
}
