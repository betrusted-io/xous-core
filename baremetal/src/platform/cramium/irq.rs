use riscv::register::{mcause, mie, mstatus};
use vexriscv::register::vexriscv::{mim, mip};

use crate::*;

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
                "li          sp, 0x610FE000", // crate::platform::SCRATCH_PAGE - has to be hard-coded
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
                // Save x1, which was used to calculate the offset.  Prior to
                // calculating, it was stashed at 0x61006000.
                //"li          t0, 0x61006000",
                //"lw        t1, 0*4(t0)",
                //"sw       t1, 0*4(sp)",

                // Finally, save SP
                "csrr        t0, mscratch",
                "sw          t0, 1*4(sp)",
                // Restore a default stack pointer
                "li          sp, 0x61100000", // heap base, grows down

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
            let ms = SYSTEM_TICK_INTERVAL_MS;
            let mut timer0 = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
            timer0.wfo(utra::timer0::EV_PENDING_ZERO, 1);
            timer0.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
        } else if (irqs_pending & (1 << utra::irqarray5::IRQARRAY5_IRQ)) != 0 {
            crate::uart_irq_handler();
        }
    }

    // re-enable interrupts
    unsafe { mie::set_mext() };

    // crate::println!("restoring from {:x}", crate::platform::SCRATCH_PAGE);
    unsafe { _resume_context(crate::platform::SCRATCH_PAGE as u32) };
}
