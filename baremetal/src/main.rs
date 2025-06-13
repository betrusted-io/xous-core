#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

// contains runtime setup
mod asm;

mod platform;
use platform::*;

/// Entrypoint
/// This makes the program self-sufficient by setting up memory page assignment
/// and copying the arguments to RAM.
/// Assume the bootloader has already set up the stack to point to the end of RAM.
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry() -> ! {
    let mut count = 0;
    loop {
        crate::println!("hello world {}!\n", count);
        count += 1;
    }
}

// Install a panic handler when not running tests.
#[cfg(all(not(test)))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}
