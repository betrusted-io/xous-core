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

            // twiddle duart
            "li          t0, 0x40042000",
            // setup etuc
            "sw          x0, 0x4(t0)", // CR is 0
            "li          t1, 34", // tuned based on ringosc & oscope. not guaranteed to be precise
            "sw          t1, 0xc(t0)",
            "li          t1, 0x1",
            "sw          t1, 0x4(t0)", // CR is 1
            // print 32 instances of 'Z' (0x5A) (provided to measure baud)
            "li          t2, 32",
            "li          t1, 0x5A",
        "10:",
            "sw          t1, 0x0(t0)",
        "11:",
            "lw          t3, 0x8(t0)", // check SR
            "bne         x0, t3, 11b", // wait for 0
            "addi        t2, t2, -1",
            "bne         x0, t2, 10b",

            /*
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
            */
            /*
            // clear main ram
            "li          t0, 0x61000000",
            "li          t1, 0x61200000",
        "20:",
            "sw          x0, 0(t0)",
            "addi        t0, t0, 4",
            "bltu        t0, t1, 20b",

            ".word       0x500f",
            */
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
