#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
mod asm;
mod sim;
use sim::*;
pub mod debug;
pub use debug::*;
#[cfg(any(feature = "sce-hash", feature = "sce-rsa"))]
mod boot;
#[cfg(feature = "sce-hash")]
mod sce_hash;
#[cfg(feature = "sce-rsa")]
mod sce_rsa;
use utralib::*;

// NOTE: the heap needs initialization before use. In this minimal example, it is declared, but not
// initialized.
#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

/// Entrypoint
///
/// # Safety
///
/// This function is safe to call exactly once.
#[unsafe(export_name = "rust_entry")]
pub unsafe extern "C" fn rust_entry() -> ! {
    // this does nothing but print an "up" message, and then quit the simulation
    crate::println!("\n~~Primordial up!~~\n");

    #[cfg(feature = "sce-hash")]
    sce_hash::sce_hash();

    #[cfg(feature = "sce-rsa")]
    sce_rsa::sce_rsa();

    exit_sim()
}

fn exit_sim() -> ! {
    println!("Exiting simulation");
    // this triggers the simulation to end using a sim-only verilog hook
    let mut report = CSR::new(main::HW_MAIN_BASE as *mut u32);
    report.wfo(main::DONE_DONE, 1);
    // this sequence triggers an end of simulation on s32
    let mut test_cfg = CSR::new(csrtest::HW_CSRTEST_BASE as *mut u32);
    test_cfg.wo(csrtest::WTEST, 0xc0ded02e);
    test_cfg.wo(csrtest::WTEST, 0xc0de600d);
    loop {}
}

mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        // Skip printing the panic message to cut code size.
        crate::println!("{}", _arg);
        loop {}
    }
}
