use xous::{CtxID, PID};

pub fn disable_all_irqs() {
    // unimplemented!()
}

pub fn enable_all_irqs() {
    // unimplemented!()
}

pub fn enable_irq(_irq_no: usize) {
    unimplemented!();
}

pub fn disable_irq(_irq_no: usize) {
    unimplemented!();
}

pub unsafe fn take_isr_return_pair() -> Option<(PID, CtxID)> {
    unimplemented!()
}

pub unsafe fn set_isr_return_pair(pid: PID, ctx: CtxID) {
    unimplemented!()
}
