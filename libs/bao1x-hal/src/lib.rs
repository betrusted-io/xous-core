#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
#[cfg(not(feature = "hosted-baosec"))]
pub mod debug;
#[cfg(feature = "axp2101")]
pub mod axp2101;
#[cfg(feature = "bmp180")]
pub mod bmp180;
pub mod board;
#[cfg(feature = "camera-gc2145")]
pub mod gc2145;
#[cfg(not(feature = "hosted-baosec"))]
pub mod ifram;
#[cfg(not(feature = "hosted-baosec"))]
pub mod iox;
#[cfg(feature = "camera-ov2640")]
pub mod ov2640;
#[cfg(not(feature = "hosted-baosec"))]
pub mod sce;
#[cfg(feature = "display-sh1107")]
pub mod sh1107;
#[cfg(not(feature = "hosted-baosec"))]
pub mod shared_csr;
#[cfg(not(feature = "hosted-baosec"))]
pub mod udma;
#[cfg(not(feature = "hosted-baosec"))]
pub mod usb;
#[cfg(not(feature = "hosted-baosec"))]
pub use shared_csr::*;
#[cfg(not(feature = "hosted-baosec"))]
pub mod acram;
// this implements the abstract library calls for BIO
#[cfg(all(feature = "std", not(feature = "hosted-baosec")))]
pub mod bio;
// this implements the no-std hardware interfaces for BIO
#[cfg(not(feature = "hosted-baosec"))]
pub mod bio_hw;
#[cfg(not(feature = "hosted-baosec"))]
pub mod buram;
#[cfg(not(feature = "hosted-baosec"))]
pub mod clocks;
#[cfg(not(feature = "hosted-baosec"))]
pub mod coreuser;
#[cfg(feature = "security")]
pub mod hardening;
#[cfg(feature = "std")]
pub mod i2c;
#[cfg(all(not(feature = "hosted-baosec"), feature = "std"))]
pub mod kpc_aoint;
#[cfg(not(feature = "hosted-baosec"))]
pub mod mbox;
#[cfg(not(feature = "hosted-baosec"))]
pub mod rram;
#[cfg(not(feature = "hosted-baosec"))]
pub mod rtc;
#[cfg(feature = "security")]
pub mod sigcheck;
#[cfg(not(feature = "hosted-baosec"))]
pub mod sram_trim;

#[inline(always)]
pub fn cache_flush() {
    unsafe {
        // cache flush
        #[rustfmt::skip]
        core::arch::asm!(
            "fence.i",
            ".word 0x500F",
            "nop",
            "nop",
            "nop",
            "nop",
        );
    }
}

/// A function for dumping stack. Used to help diagnose tricky problems. Invoke as:
/// `unsafe { bao1x_hal::dump_stack!(0x300) };` inside any stack frame where
/// you want to dump some stack. The extents are specified as number of bytes,
/// and should be word-aligned.
#[macro_export]
macro_rules! dump_stack {
    ($extent_bytes:expr) => {{
        $crate::dump_stack!($extent_bytes, bao1x_hal::read_sp);
    }};
    // Explicit SP reader path
    ($extent_bytes:expr, $read_sp:path) => {{
        // No internal `unsafe` so the caller must use `unsafe { dump_stack!(...) }`
        let sp = $read_sp();
        let __word: usize = core::mem::size_of::<u32>();
        let __extent_words: usize = ($extent_bytes) / __word;
        let mut __i: usize = 0;
        while __i < __extent_words {
            if __i % 8 == 0 {
                $crate::print!("\n\r{:08x}|{:04x}: ", sp + __i * __word, __i * __word);
            }
            // volatile read of the stack word
            $crate::print!("{:08x} ", ((sp as *const u32).add(__i)).read_volatile());
            __i += 1;
        }
        $crate::println!("");
    }};
}

#[inline(always)]
pub unsafe fn read_sp() -> usize {
    let sp: usize;
    core::arch::asm!("mv {0}, sp", out(reg) sp);
    sp
}

/// DUART is first-come, first-served in Xous environment. This stub
/// can be called early in a server's initialization process so that it can get
/// exclusive access to the DUART (assuming the kernel is configured to relinquish it)
#[cfg(all(feature = "std", not(feature = "hosted-baosec")))]
pub fn claim_duart() {
    crate::println!("PID {} got duart", xous::process::id());
}
