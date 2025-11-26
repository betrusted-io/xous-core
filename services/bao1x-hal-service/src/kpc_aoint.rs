use bao1x_hal::kpc_aoint::KpcAoInt;
use utralib::utra::irqarray2;

pub fn handler(_irq_no: usize, arg: *mut usize) {
    let kpc_aoint = unsafe { &mut *(arg as *mut KpcAoInt) };
    let pending = kpc_aoint.irq.r(irqarray2::EV_PENDING);
    // clear all pending interrupts
    kpc_aoint.irq.wo(irqarray2::EV_PENDING, pending);

    // Note to self: this routine might need augmentation if the interrupt source also
    // has to be *disabled*. This would be necessary if the interrupt persists as asserted
    // instead of being a pulse, to prevent re-entrant interrupting.

    for bit in 0..16u32 {
        let mask = 1u32 << bit;
        if (pending & mask) != 0 {
            for notifier in kpc_aoint.args.iter() {
                if notifier.bit.value() as u32 == bit {
                    xous::try_send_message(
                        notifier.conn,
                        xous::Message::new_scalar(
                            notifier.opcode,
                            pending as usize,
                            notifier.args[1],
                            notifier.args[2],
                            notifier.args[3],
                        ),
                    )
                    .ok();
                }
            }
        }
    }
}
