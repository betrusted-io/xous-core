use crate::services::Thread;
use riscv::register::{sepc, sstatus};

extern "C" {
    fn _xous_resume_context(regs: *const usize) -> !;
}

pub fn invoke(
    Thread: &mut Thread,
    supervisor: bool,
    pc: usize,
    sp: usize,
    ret_addr: usize,
    args: &[usize],
) {
    set_supervisor(supervisor);
    Thread.registers[0] = ret_addr;
    Thread.registers[1] = sp;
    assert!(args.len() <= 8, "too many arguments to invoke()");
    for (idx, arg) in args.iter().enumerate() {
        Thread.registers[9 + idx] = *arg;
    }
    Thread.sepc = pc;
}

fn set_supervisor(supervisor: bool) {
    if supervisor {
        unsafe { sstatus::set_spp(sstatus::SPP::Supervisor) };
    } else {
        unsafe { sstatus::set_spp(sstatus::SPP::User) };
    }
}

pub fn resume(supervisor: bool, Thread: &Thread) -> ! {
    sepc::write(Thread.sepc);

    // Return to the appropriate CPU mode
    set_supervisor(supervisor);

    // println!(
    //     "Switching to PID {}, SP: {:08x}, PC: {:08x}",
    //     crate::arch::current_pid(),
    //     Thread.registers[1],
    //     Thread.sepc,
    // );
    unsafe { _xous_resume_context(Thread.registers.as_ptr()) };
}
