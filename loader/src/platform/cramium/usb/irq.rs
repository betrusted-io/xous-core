use cramium_hal::usb::utra::*;
use riscv::register::{mcause, mie, mstatus, vexriscv::mim, vexriscv::mip};

use super::*;

pub(crate) fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    mim::write(mim::read() | (1 << irq_no));
}

pub(crate) fn irq_setup() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            // stop delegating
            "li          t0, 0x0",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",
            // Set trap handler, which will be called
            // on interrupts and cpu faults
            "la   t0, _start_trap", // this first one forces the nop sled symbol to be generated
            "la   t0, _start_trap_aligned", // this is the actual target
            "csrw mtvec, t0",
        );
    }
    // enable IRQ handling
    mim::write(0x0); // first make sure everything is disabled, so we aren't OR'ing in garbage
    // must enable external interrupts on the CPU for any of the above to matter
    unsafe { mie::set_mext() };
    unsafe { mstatus::set_mie() };
}

#[export_name = "_start_trap"]
// #[repr(align(4))] // can't do this yet.
#[inline(never)]
pub unsafe extern "C" fn _start_trap() -> ! {
    loop {
        // install a NOP sled before _start_trap() until https://github.com/rust-lang/rust/issues/82232 is stable
        core::arch::asm!("nop", "nop", "nop", "nop");
        #[export_name = "_start_trap_aligned"]
        pub unsafe extern "C" fn _start_trap_aligned() {
            #[rustfmt::skip]
            core::arch::asm!(
                "csrw        mscratch, sp",
                "li          sp, 0x6101F000", // scratch page: one page below the disk start
                "sw       x1, 0*4(sp)",
                // Skip SP for now
                "sw       x3, 2*4(sp)",
                "sw       x4, 3*4(sp)",
                "sw       x5, 4*4(sp)",
                "sw       x6, 5*4(sp)",
                "sw       x7, 6*4(sp)",
                "sw       x8, 7*4(sp)",
                "sw       x9, 8*4(sp)",
                "sw       x10, 9*4(sp)",
                "sw       x11, 10*4(sp)",
                "sw       x12, 11*4(sp)",
                "sw       x13, 12*4(sp)",
                "sw       x14, 13*4(sp)",
                "sw       x15, 14*4(sp)",
                "sw       x16, 15*4(sp)",
                "sw       x17, 16*4(sp)",
                "sw       x18, 17*4(sp)",
                "sw       x19, 18*4(sp)",
                "sw       x20, 19*4(sp)",
                "sw       x21, 20*4(sp)",
                "sw       x22, 21*4(sp)",
                "sw       x23, 22*4(sp)",
                "sw       x24, 23*4(sp)",
                "sw       x25, 24*4(sp)",
                "sw       x26, 25*4(sp)",
                "sw       x27, 26*4(sp)",
                "sw       x28, 27*4(sp)",
                "sw       x29, 28*4(sp)",
                "sw       x30, 29*4(sp)",
                "sw       x31, 30*4(sp)",
                // Save MEPC
                "csrr        t0, mepc",
                "sw       t0, 31*4(sp)",

                // Finally, save SP
                "csrr        t0, mscratch",
                "sw          t0, 1*4(sp)",
                // Restore a default stack pointer
                "li          sp, 0x6101F000", /* builds down from scratch page */
                // Note that registers $a0-$a7 still contain the arguments
                "j           _start_trap_rust",
                // Note to self: trying to assign the scratch and default pages using in(reg) syntax
                // clobbers the `a0` register and places the initialization outside of the handler loop
                // and there seems to be no way to refer directly to a symbol? the `sym` directive wants
                // to refer to an address, not a constant.
            );
        }
        _start_trap_aligned();
        core::arch::asm!("nop", "nop", "nop", "nop");
    }
}

#[export_name = "_resume_context"]
#[inline(never)]
pub unsafe extern "C" fn _resume_context(registers: u32) -> ! {
    #[rustfmt::skip]
    core::arch::asm!(
        "move        sp, {registers}",

        "lw        x1, 0*4(sp)",
        // Skip SP for now
        "lw        x3, 2*4(sp)",
        "lw        x4, 3*4(sp)",
        "lw        x5, 4*4(sp)",
        "lw        x6, 5*4(sp)",
        "lw        x7, 6*4(sp)",
        "lw        x8, 7*4(sp)",
        "lw        x9, 8*4(sp)",
        "lw        x10, 9*4(sp)",
        "lw        x11, 10*4(sp)",
        "lw        x12, 11*4(sp)",
        "lw        x13, 12*4(sp)",
        "lw        x14, 13*4(sp)",
        "lw        x15, 14*4(sp)",
        "lw        x16, 15*4(sp)",
        "lw        x17, 16*4(sp)",
        "lw        x18, 17*4(sp)",
        "lw        x19, 18*4(sp)",
        "lw        x20, 19*4(sp)",
        "lw        x21, 20*4(sp)",
        "lw        x22, 21*4(sp)",
        "lw        x23, 22*4(sp)",
        "lw        x24, 23*4(sp)",
        "lw        x25, 24*4(sp)",
        "lw        x26, 25*4(sp)",
        "lw        x27, 26*4(sp)",
        "lw        x28, 27*4(sp)",
        "lw        x29, 28*4(sp)",
        "lw        x30, 29*4(sp)",
        "lw        x31, 30*4(sp)",

        // Restore SP
        "lw        x2, 1*4(sp)",
        "mret",
        registers = in(reg) registers,
    );
    loop {}
}

#[export_name = "_start_trap_rust"]
pub extern "C" fn trap_handler(
    _a0: usize,
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> ! {
    let mc: mcause::Mcause = mcause::read();
    // crate::println!("it's a trap! 0x{:x}", mc.bits());
    // 2 is illegal instruction
    if mc.bits() == 2 {
        crate::abort();
    } else if mc.bits() == 0x8000_000B {
        // external interrupt. find out which ones triggered it, and clear the source.
        let irqs_pending = mip::read();

        if (irqs_pending & (1 << utralib::utra::irqarray1::IRQARRAY1_IRQ)) != 0 {
            // handle USB interrupt
            unsafe {
                if let Some(ref mut usb_ref) = USB {
                    let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);

                    // immediately clear the interrupt and re-enable it so we can catch an interrupt
                    // that is generated while we are handling the interrupt.
                    let pending = usb.irq_csr.r(utralib::utra::irqarray1::EV_PENDING);
                    // clear pending
                    usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, pending);
                    // re-enable interrupts
                    usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);

                    let status = usb.csr.r(USBSTS);
                    // usb.print_status(status);
                    if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
                        crate::println!("System error");
                        usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
                        crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
                    } else {
                        if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
                            // from udc_handle_interrupt
                            let mut ret = cramium_hal::usb::driver::CrgEvent::None;
                            let status = usb.csr.r(USBSTS);
                            // self.print_status(status);
                            let result = if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
                                crate::println!("System error");
                                usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
                                crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
                                cramium_hal::usb::driver::CrgEvent::Error
                            } else {
                                if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
                                    usb.csr.wfo(USBSTS_EINT, 1);
                                    // divert to the loader-based event ring handler
                                    ret = usb.process_event_ring(); // there is only one event ring
                                }
                                ret
                            };
                            crate::println!("Result: {:?}", result);
                        }
                        if usb.csr.rf(IMAN_IE) != 0 {
                            usb.csr.wo(IMAN, usb.csr.ms(IMAN_IE, 1) | usb.csr.ms(IMAN_IP, 1));
                        }
                    }
                }
            }
        }
        if (irqs_pending & (1 << 19)) != 0 {
            // handle irq19 sw trigger test
            let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut u32);
            let pending = irqarray19.r(utralib::utra::irqarray19::EV_PENDING);
            irqarray19.wo(utralib::utra::irqarray19::EV_PENDING, pending);
            // software interrupt should not require a 0-write to reset it
        }
    } else {
        crate::abort();
    }

    // re-enable interrupts
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            "csrr        t0, mstatus",
            "ori         t0, t0, 3",
            "csrw        mstatus, t0",
        );
    }
    unsafe { mie::set_mext() };
    unsafe { _resume_context(SCRATCH_PAGE as u32) };
}
