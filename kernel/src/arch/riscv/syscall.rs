use crate::processtable::ProcessContext;
use vexriscv::register::{sepc, sstatus};

extern "C" {
    fn _xous_resume_context(regs: *const usize) -> !;
}

pub fn invoke(
    context: &mut ProcessContext,
    supervisor: bool,
    pc: usize,
    sp: usize,
    ret_addr: usize,
    args: &[usize],
) {
    set_supervisor(supervisor);
    context.registers[0] = ret_addr;
    context.registers[1] = sp;
    context.registers[9] = args[0];
    context.registers[10] = args[1];
    context.sepc = pc;
}

fn set_supervisor(supervisor: bool) {
    if supervisor {
        unsafe { sstatus::set_spp(sstatus::SPP::Supervisor) };
    } else {
        unsafe { sstatus::set_spp(sstatus::SPP::User) };
    }
}

pub fn resume(supervisor: bool, context: &ProcessContext) -> ! {
    sepc::write(context.sepc);

    // Return to the appropriate CPU mode
    set_supervisor(supervisor);

    println!(
        "Switching to PID {}, SP: {:08x}, PC: {:08x}",
        crate::arch::current_pid(),
        context.registers[1],
        context.sepc,
    );
    unsafe { _xous_resume_context(context.registers.as_ptr()) };
}
