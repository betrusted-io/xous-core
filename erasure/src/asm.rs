use core::arch::asm;

use crate::platform;
// Assembly stubs for entering into the loader, and exiting it.

// Note: inline constants are not yet stable in Rust: https://github.com/rust-lang/rust/pull/104087
#[link_section = ".text.init"]
#[export_name = "_start"]
#[cfg(not(feature = "bao1x"))]
pub extern "C" fn _start() {
    unsafe {
        #[rustfmt::skip]
        asm! (
            // Place the stack pointer at the end of RAM
            "mv          sp, {ram_top}",
            // subtract four from sp to make room for a DMA "gutter"
            "addi        sp, sp, -4",

            // Install a machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",

            // Start Rust
            "j   rust_entry",

            ram_top = in(reg) (platform::RAM_BASE + platform::RAM_SIZE),
            options(noreturn)
        );
    }
}

#[link_section = ".text.init"]
#[export_name = "_start"]
#[cfg(feature = "bao1x")]
pub extern "C" fn _start() {
    unsafe {
        #[rustfmt::skip]
        asm! (
            // Place the stack pointer at the end of RAM
            "mv          sp, {ram_top}",
            // subtract four from sp to make room for a DMA "gutter"
            "addi        sp, sp, -4",

        "50:",
            // Install a machine mode trap handler
            "la          t0, abort",
            "csrw        mtvec, t0",

            // Start Rust
            "j   rust_entry",

            ram_top = in(reg) (platform::RAM_BASE + platform::RAM_SIZE),
            options(noreturn)
        );
    }
}

#[link_section = ".text.init"]
#[export_name = "abort"]
/// This is only used in debug mode
pub extern "C" fn abort() -> ! {
    unsafe {
        #[rustfmt::skip]
        asm!(
            "li          t0, 0x40042000",
            "li          t1, 0x75",
            "li          t2, 1024",
        "10:", // abort by printing u (0x75) to duart
            "sw          t1, 0x0(t0)",
        "11:",
            "lw          t3, 0x8(t0)", // check SR
            "bne         x0, t3, 11b", // wait for 0
            "li          t3, 4000",
            // add a short delay between characters to help differentiate them on oscope
        "12:",
            "addi        t3, t3, -1",
            "bne         x0, t3, 12b",
            // check how many prints
            "addi        t2, t2, -1",
            "bne         x0, t2, 10b",
        "300:",
            "j 300b",
            options(noreturn)
        );
    }
}
