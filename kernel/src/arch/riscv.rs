use riscv::register::{satp, sie, sstatus};
use xous::PID;

pub mod exception;
pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

pub use process::ProcessContext;

pub fn current_pid() -> PID {
    satp::read().asid() as PID
}

pub fn init() {
    unsafe {
        sstatus::set_sie();
        sie::set_ssoft();
        sie::set_sext();
    }
}

pub fn wfi() {
    unsafe { riscv::asm::wfi() };
}
