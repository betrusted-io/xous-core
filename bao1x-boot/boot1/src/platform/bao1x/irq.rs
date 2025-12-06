use bao1x_hal::board::DEFAULT_FCLK_FREQUENCY;
use bao1x_hal::usb::utra::*;
use riscv::register::{mcause, mie, mstatus};
use vexriscv::register::vexriscv::{mim, mip};

use crate::usb::USB;
use crate::*;

static ATTACKS_SINCE_BOOT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub fn irq_setup() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            // Set trap handler, which will be called
            // on interrupts and cpu faults
            "la   t0, _start_trap", // this first one forces the nop sled symbol to be generated
            "la   t0, _start_trap_aligned", // this is the actual target
            "csrw mtvec, t0",
        );
    }

    mim::write(0x0); // first make sure everything is disabled, so we aren't OR'ing in garbage
    unsafe {
        mstatus::set_mie();
    }
    // must enable external interrupts on the CPU for any of the above to matter
    unsafe { mie::set_mext() };
}

pub fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    mim::write(mim::read() | (1 << irq_no));
    // must enable external interrupts on the CPU for any of the above to matter
    unsafe { mie::set_mext() };
}

pub fn disable_all_irqs() {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    mim::write(0);
    unsafe { mie::clear_mext() };
    unsafe { mstatus::clear_mie() };
    // redo delegations
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",
            // Re-install the machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",
        );
    }
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
                "li          sp, {scratch_page}", // scratch_page, grows up
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
                "li          sp, {scratch_page}", // scratch_page, grows down

                // Note that registers $a0-$a7 still contain the arguments
                "j           _start_trap_rust",

                scratch_page = const SCRATCH_PAGE
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

/// Just handles specific traps for testing CPU interactions. Doesn't do anything useful with the traps.
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
    let cause: mcause::Mcause = mcause::read();
    // crate::println!("cause {:x}", cause.bits());
    // 2 is illegal instruction
    if cause.bits() == 2 {
        // skip past the illegal instruction, in case that's what we want to do...
        unsafe {
            #[rustfmt::skip]
            core::arch::asm!(
                "csrr        t0, mepc",
                "addi        t0, t0, 4",
                "csrw        mepc, t0",
            );
        }
        // ...but also panic.
        panic!("Illegal Instruction");
    } else if cause.bits() == 4 {
        let epc = riscv::register::mepc::read();
        panic!("Load address misaligned @ {:x}", epc);
    } else if cause.bits() == 6 {
        let epc = riscv::register::mepc::read();
        panic!("Store address misaligned @ {:x}", epc);
    } else if cause.bits() == 0x8000_000b {
        // external machine interrupt
        // external interrupt. find out which ones triggered it, and clear the source.
        let irqs_pending = mip::read();
        if (irqs_pending & (1 << utra::timer0::TIMER0_IRQ)) != 0 {
            let ms = bao1x_api::SYSTEM_TICK_INTERVAL_MS;
            let mut timer0 = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
            timer0.wfo(utra::timer0::EV_PENDING_ZERO, 1);
            // TODO: link this to dabao targets. Right now this value is correct for baosec only.
            timer0.wfo(utra::timer0::RELOAD_RELOAD, (DEFAULT_FCLK_FREQUENCY / 1_000) * ms);
        } else if (irqs_pending & (1 << utra::irqarray5::IRQARRAY5_IRQ)) != 0 {
            crate::uart_irq_handler();
        } else if (irqs_pending & (1 << utra::irqarray13::IRQARRAY13_IRQ)) != 0 {
            // irq13 -> {aoramerr, secirq, ifsuberr, sceerr, coresuberr}
            // this IRQ will trigger an attack response if any of the sensors trigger *or*
            // if the ECC-protected memory banks trigger an ECC error
            bao1x_hal::hardening::glitch_handler(
                ATTACKS_SINCE_BOOT.fetch_add(1, core::sync::atomic::Ordering::SeqCst),
            );
        } else if (irqs_pending & (1 << utra::irqarray15::IRQARRAY15_IRQ)) != 0 {
            // irq15 -> {meshirq, sensorcirq, glcirq}
            // those one is kind of redundant with irq13 in this scenario, because secirq is the OR
            // of all three IRQs going into this array. So, we handle this simply in irqarray13.
        }
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
                            let mut ret = bao1x_hal::usb::driver::CrgEvent::None;
                            let status = usb.csr.r(USBSTS);
                            // self.print_status(status);
                            let _result = if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
                                crate::println!("System error");
                                usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
                                crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
                                bao1x_hal::usb::driver::CrgEvent::Error
                            } else {
                                if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
                                    usb.csr.wfo(USBSTS_EINT, 1);
                                    // divert to the loader-based event ring handler
                                    ret = usb.process_event_ring(); // there is only one event ring
                                }
                                ret
                            };
                            // crate::println!("Result: {:?}", _result);
                        }
                        if usb.csr.rf(IMAN_IE) != 0 {
                            usb.csr.wo(IMAN, usb.csr.ms(IMAN_IE, 1) | usb.csr.ms(IMAN_IP, 1));
                        }
                    }
                }
            }
        }
    }

    // re-enable interrupts
    unsafe { mie::set_mext() };

    // crate::println!("restoring from {:x}", crate::platform::SCRATCH_PAGE);
    unsafe { _resume_context(crate::platform::SCRATCH_PAGE as u32) };
}
