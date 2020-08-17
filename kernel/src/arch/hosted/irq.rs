use xous_kernel::{TID, PID};

pub fn disable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_irq(_irq_no: usize) {
    unimplemented!();
}

pub fn disable_irq(_irq_no: usize) -> Result<(), xous_kernel::Error> {
    Err(xous_kernel::Error::UnhandledSyscall)
}

pub unsafe fn take_isr_return_pair() -> Option<(PID, TID)> {
    unimplemented!()
}

pub unsafe fn set_isr_return_pair(_pid: PID, _ctx: TID) {
    unimplemented!()
}
