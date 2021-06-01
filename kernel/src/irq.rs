// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::arch;
use xous_kernel::{MemoryAddress, PID};

static mut IRQ_HANDLERS: [Option<(PID, MemoryAddress, Option<MemoryAddress>)>; 32] = [None; 32];

#[cfg(baremetal)]
pub fn handle(irqs_pending: usize) -> Result<xous_kernel::Result, xous_kernel::Error> {
    use crate::services::SystemServices;
    // Unsafe is required here because we're accessing a static
    // mutable value, and it could be modified from various threads.
    // However, this is fine because this is run from an IRQ context
    // with interrupts disabled.
    // NOTE: This will become an issue when running with multiple cores,
    // so this should be protected by a mutex.
    unsafe {
        for irq_no in 0..IRQ_HANDLERS.len() {
            if irqs_pending & (1 << irq_no) != 0 {
                if let Some((pid, f, arg)) = IRQ_HANDLERS[irq_no] {
                    return SystemServices::with_mut(|ss| {
                        // Disable all other IRQs and redirect into userspace
                        arch::irq::disable_all_irqs();
                        // println!("Making a callback to PID{}: {:x?} ({:08x}, {:x?})", pid, f, irq_no as usize, arg);
                        ss.make_callback_to(
                            pid,
                            f.get() as *mut usize,
                            irq_no,
                            arg.map(|x| x.get() as *mut usize)
                                .unwrap_or(core::ptr::null_mut::<usize>()),
                        )
                        .map(|_| xous_kernel::Result::ResumeProcess)
                    });
                } else {
                    // If there is no handler, mask this interrupt
                    // to prevent an IRQ storm.  This is considered
                    // an error.
                    arch::irq::disable_irq(irq_no)?;
                }
            }
        }
    }
    Ok(xous_kernel::Result::ResumeProcess)
}

pub fn interrupt_claim(
    irq: usize,
    pid: PID,
    f: MemoryAddress,
    arg: Option<MemoryAddress>,
) -> Result<(), xous_kernel::Error> {
    // Unsafe is required since we're accessing a static mut array.
    // However, we disable interrupts to prevent contention on this array.
    unsafe {
        let result = if irq > IRQ_HANDLERS.len() {
            Err(xous_kernel::Error::InterruptNotFound)
        } else if IRQ_HANDLERS[irq].is_some() {
            Err(xous_kernel::Error::InterruptInUse)
        } else {
            IRQ_HANDLERS[irq] = Some((pid, f, arg));
            arch::irq::enable_irq(irq);
            Ok(())
        };
        result
    }
}

/// Iterate through the IRQ handlers and remove any handler that exists
/// for the given PID.
pub fn release_interrupts_for_pid(pid: PID) {
    unsafe {
        for (irq, handler) in IRQ_HANDLERS.iter_mut().enumerate() {
            if let Some(h) = handler {
                if h.0 == pid {
                    arch::irq::disable_irq(irq).unwrap();
                    *handler = None;
                }
            }
        }
    }
}
