use core::arch::asm;

use crate::platform;
// Assembly stubs for entering into the loader, and exiting it.

// Note: inline constants are not yet stable in Rust: https://github.com/rust-lang/rust/pull/104087
#[cfg(not(feature = "reset-debug"))]
#[link_section = ".text.init"]
#[export_name = "_start"]
pub extern "C" fn _start(_kernel_args: usize, loader_sig: usize) {
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let _kernel_args = _kernel_args;
    #[cfg(any(feature = "cramium-soc", feature = "cramium-fpga"))]
    let _kernel_args = platform::FLASH_BASE + platform::KERNEL_OFFSET;

    // Stub for clearing IFRAM & RAM on Cramium target. This is required
    // to clear the parity check bits, which are randomly set on boot. System will
    // eventually hang if these bits aren't cleared.
    #[cfg(all(any(feature = "cramium-soc", feature = "cramium-fpga"), not(feature = "simulation-only")))]
    unsafe {
        #[rustfmt::skip]
        asm! (
            // test if ifram is cleared
            "li          t0, 0x50000000",
            "li          t1, 0x50040000",
        "60:",
            "lw          t2, 0(t0)",
            // if not 0, jump to clearing routine
            "bne         x0, t2, 30f",
            "addi        t0, t0, 4",
            // loop if we haven't checked all of ifram
            "bltu        t0, t1, 60b",
            // if we got here, all of ifram was 0, continue with boot
            "j           50f",

            // clear ifram
        "30:",
            "sw          x0, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t1, 30b",

            // clear main ram
            "li          t0, 0x61000000",
            "li          t1, 0x61200000",
        "20:",
            "sw          x0, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t1, 20b",

            ".word       0x500f",
            // reset the system
            "li          t0, 0x40040080",
            "li          t1, 0x55aa",
        "70:",
            "sw          t1, 0(t0)",
            "j           70b", // loop forever trying to trigger the reset
            // Note that BMX access is unstable at this point in time due to the
            // high number of parity errors that were flooding the system. Thus,
            // the system has to go through a reset cycle before any further progress
            // can be made.
            // --------------------------------
            // system should be rebooting at this point, code after this
            // is unreachabale
            // --------------------------------

            // continue on boot
        "50:",
        );
    }
    unsafe {
        #[rustfmt::skip]
        asm! (
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // decorate our stack area with a canary pattern
            "li          t1, 0xACE0BACE",
            "mv          t0, {stack_limit}",
            "mv          t2, {ram_top}",
        "100:", // fillstack
            "sw          t1, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t2, 100b",

            // Place the stack pointer at the end of RAM
            "mv          sp, {ram_top}",
            // subtract four from sp to make room for a DMA "gutter"
            "addi        sp, sp, -4",

            // Install a machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",

            // this forces a0/a1 to be "used" and thus not allocated for other parameters passed in
            "mv          a0, {kernel_args}",
            "mv          a1, {loader_sig}",
            // Start Rust
            "j   rust_entry",

            kernel_args = in(reg) _kernel_args,
            loader_sig = in(reg) loader_sig,
            ram_top = in(reg) (platform::RAM_BASE + platform::RAM_SIZE),
            // On Precursor - 0x40FFE01C: currently allowed stack extent - 8k - (7 words). 7 words are for kernel backup args - see bootloader in betrusted-soc
            stack_limit = in(reg) (platform::RAM_BASE + platform::RAM_SIZE - 8192 + 7 * core::mem::size_of::<usize>()),
            options(noreturn)
        );
    }
}

// This is code for debugging RV32 core reset on the cramium target.
#[cfg(feature = "reset-debug")]
#[link_section = ".text.init"]
#[export_name = "_start"]
pub extern "C" fn _start(_kernel_args: usize, loader_sig: usize) {
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let _kernel_args = _kernel_args;
    #[cfg(any(feature = "cramium-soc", feature = "cramium-fpga"))]
    let _kernel_args = platform::FLASH_BASE + platform::KERNEL_OFFSET;
    unsafe {
        #[rustfmt::skip]
        asm! (
            /*
            "li          t0, 0x40040000",
            "li          t1, 0x7f7f",
            "sw          t1, 0x14(t0)",
            "sw          t1, 0x18(t0)",
            "li          t1, 0x3f7f",
            "sw          t1, 0x1c(t0)",
            "li          t1, 0x1f3f",
            "sw          t1, 0x20(t0)",
            "li          t1, 0x0f1f",
            "sw          t1, 0x24(t0)",
            "li          t1, 0xFF",
            "sw          t1, 0x60(t0)",
            "sw          t1, 0x64(t0)",
            "sw          t1, 0x68(t0)",
            "sw          t1, 0x6c(t0)",
            "li          t2, 0x32",
            "sw          t2, 0x2c(t0)",
            */

            // GPIO twiddle
            /*
            "li          t0, 0x5012f000",
            "li          t1, 0x5550",
            "sw          t1, 0x8(t0)", // AFSEL
            "li          t2, 0x1803",
            "sw          t2, 0x14c(t0)", // OESEL
            "sw          x0, 0x134(t0)", // DAT
            "sw          t2, 0x134(t0)", // DAT
            "sw          x0, 0x134(t0)", // DAT
            "sw          t2, 0x134(t0)", // DAT
            "sw          x0, 0x134(t0)", // DAT
            "sw          t2, 0x134(t0)", // DAT
            */

            "add         x1, x0, x0",
            "li          x2, 0xF000",
            "20:",
            "addi        x1, x1, 1",
            "bne         x1, x2, 20b",

            /*
            // this, surprisingly, seems to have some effect..?
            // twiddle duart
            "li          t0, 0x40042000",
            "li          t1, 24",
            "sw          t1, 0xc(t0)",
            "li          t1, 0x1",
            "sw          x0, 0x4(t0)",
            "sw          t1, 0x4(t0)",
            "li          t1, 0x5A",
            "sw          t1, 0x0(t0)",
            */

            // ema adjust
            /*
            "li          t0, 0x40014000",
            "li          t1, 0x3",
            "sw          t1, 0xC(t0)",
            "sw          t1, 0x10(t0)",
            "sw          t1, 0x14(t0)",
            */

            // the RAM clearing code does not have an effect
            /*
            // clear ifram
            "li          t0, 0x50000000",
            "li          t1, 0x50040000",
            "30:",
            "sw          x0, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t1, 30b",
            // clear main ram
            "li          t0, 0x61000000",
            "li          t1, 0x61200000",
            "20:",
            "sw          x0, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t1, 20b",

            ".word       0x500f",
            */

            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // decorate our stack area with a canary pattern
            /*
            "li          t1, 0xACE0BACE",
            "mv          t0, {stack_limit}",
            "mv          t2, {ram_top}",
        "100:", // fillstack
            "sw          t1, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t2, 100b",
            */

            // Place the stack pointer at the end of RAM
            // "mv          sp, {ram_top}",
            "li          sp, 0x61200000",
            // subtract four from sp to make room for a DMA "gutter"
            "addi        sp, sp, -4",

            // Install a machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",

            // this forces a0/a1 to be "used" and thus not allocated for other parameters passed in
            // "mv          a0, {kernel_args}",
            "li          a0, 0x60040000",
            "mv          a1, {loader_sig}",
            // Start Rust
            "j   rust_entry",

            // kernel_args = in(reg) _kernel_args,
            loader_sig = in(reg) loader_sig,
            // ram_top = in(reg) (platform::RAM_BASE + platform::RAM_SIZE),
            // On Precursor - 0x40FFE01C: currently allowed stack extent - 8k - (7 words). 7 words are for kernel backup args - see bootloader in betrusted-soc
            // stack_limit = in(reg) (platform::RAM_BASE + platform::RAM_SIZE - 8192 + 7 * core::mem::size_of::<usize>()),
            options(noreturn)
        );
    }
}

#[link_section = ".text.init"]
#[export_name = "abort"]
/// This is only used in debug mode
pub extern "C" fn abort() -> ! {
    #[cfg(not(feature = "cramium-soc"))]
    unsafe {
        #[rustfmt::skip]
        asm!(
            "300:", // abort
            "j 300b",
            options(noreturn)
        );
    }
    #[cfg(feature = "cramium-soc")]
    unsafe {
        use utralib::generated::*;
        let uart = utra::duart::HW_DUART_BASE as *mut u32;

        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile(0xa as u32);
        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile(0xd as u32);
        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile('0' as char as u32);
        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile('x' as char as u32);
        let sc = riscv::register::scause::read();
        for &byte in sc.bits().to_be_bytes().iter() {
            let d = byte >> 4;
            let nyb = d & 0xF;
            let c = if nyb < 10 { nyb + 0x30 } else { nyb + 0x61 - 10 };
            while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
            uart.add(utra::duart::SFR_TXD.offset()).write_volatile(c as u32);
            let d = byte & 0xF;
            let nyb = d & 0xF;
            let c = if nyb < 10 { nyb + 0x30 } else { nyb + 0x61 - 10 };
            while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
            uart.add(utra::duart::SFR_TXD.offset()).write_volatile(c as u32);
        }
        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile(0xa as u32);
        while uart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
        uart.add(utra::duart::SFR_TXD.offset()).write_volatile(0xd as u32);

        loop {}
    }
}

#[inline(never)]
#[export_name = "start_kernel"]
pub extern "C" fn start_kernel(
    args: usize,
    ss: usize,
    rpt: usize,
    xpt: usize,
    satp: usize,
    entrypoint: usize,
    stack: usize,
    debug_resume: usize,
) -> ! {
    unsafe {
        #[rustfmt::skip]
        asm! (
            // these generate redundant mv's but it ensures that the arguments are marked as used
            "mv          a0, {args}",
            "mv          a1, {ss}",
            "mv          a2, {rpt}",
            "mv          a3, {xpt}",
            // Delegate as much as we can supervisor mode
            "li          t0, 0xffffffff",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",

            // Return to Supervisor mode (1 << 11) when we call `reti`.
            // Disable interrupts (0 << 5)
            "li		     t0, (1 << 11) | (0 << 5)",
            // If debug_resume is "true", also set mstatus.SUM to allow the kernel
            // to access userspace memory.
            "mv          t3, {debug_resume}",
            "andi        t3, t3, 1",
            "slli        t3, t3, 18",
            "or          t0, t0, t3",
            "csrw	     mstatus, t0",
            "mv          a7, {debug_resume}",

            // Enable the MMU (once we issue `mret`) and flush the cache
            "csrw        satp, {satp}",
            "sfence.vma",

            // Return to the address pointed to by $a4
            "csrw        mepc, {entrypoint}",

            // Reposition the stack at the offset passed by $a5
            "mv          sp, {stack}",

            // Issue the return, which will jump to $mepc in Supervisor mode
            "mret",
            args = in(reg) args,
            ss = in(reg) ss,
            rpt = in(reg) rpt,
            xpt = in(reg) xpt,
            satp = in(reg) satp,
            entrypoint = in(reg) entrypoint,
            stack = in(reg) stack,
            debug_resume = in(reg) debug_resume,
            options(noreturn)
        );
    }
}
