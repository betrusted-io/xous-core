use crate::services::Context;
use riscv::register::{sepc, sstatus};

extern "C" {
    fn _xous_resume_context(regs: *const usize) -> !;
}

pub fn invoke(
    context: &mut Context,
    supervisor: bool,
    pc: usize,
    sp: usize,
    ret_addr: usize,
    args: &[usize],
) {
    set_supervisor(supervisor);
    context.registers[0] = ret_addr;
    context.registers[1] = sp;
    assert!(args.len() <= 8, "too many arguments to invoke()");
    for (idx, arg) in args.iter().enumerate() {
        context.registers[9 + idx] = *arg;
    }
    context.sepc = pc;
}

fn set_supervisor(supervisor: bool) {
    if supervisor {
        unsafe { sstatus::set_spp(sstatus::SPP::Supervisor) };
    } else {
        unsafe { sstatus::set_spp(sstatus::SPP::User) };
    }
}

pub fn resume(supervisor: bool, context: &Context) -> ! {
    sepc::write(context.sepc);

    // Return to the appropriate CPU mode
    set_supervisor(supervisor);

    // println!(
    //     "Switching to PID {}, SP: {:08x}, PC: {:08x}",
    //     crate::arch::current_pid(),
    //     context.registers[1],
    //     context.sepc,
    // );
    unsafe { _xous_resume_context(context.registers.as_ptr()) };
}
