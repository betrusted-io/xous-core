use xous::{CtxID, PID};

pub fn disable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_all_irqs() {
    // There are no IRQs in a hosted environment, so there's nothing to do.
}

pub fn enable_irq(_irq_no: usize) {
    unimplemented!();
}

pub fn disable_irq(_irq_no: usize) -> Result<(), xous::Error> {
    Err(xous::Error::UnhandledSyscall)
}

pub unsafe fn take_isr_return_pair() -> Option<(PID, CtxID)> {
    unimplemented!()
}

pub unsafe fn set_isr_return_pair(_pid: PID, _ctx: CtxID) {
    unimplemented!()
}
