#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;

mod platform;
#[cfg(feature = "unsafe-dev")]
mod repl;
#[cfg(feature = "unsafe-dev")]
use alloc::collections::VecDeque;
#[cfg(feature = "unsafe-dev")]
use core::cell::RefCell;

use bao1x_api::signatures::FunctionCode;
#[cfg(feature = "unsafe-dev")]
use critical_section::Mutex;
use platform::*;
#[allow(unused_imports)]
use utralib::*;

#[allow(unused_imports)]
use crate::delay;

#[cfg(feature = "unsafe-dev")]
static UART_RX: Mutex<RefCell<VecDeque<u8>>> = Mutex::new(RefCell::new(VecDeque::new()));

#[cfg(feature = "unsafe-dev")]
pub fn uart_irq_handler() {
    use crate::debug::SerialRead;
    let mut uart = crate::debug::Uart {};

    loop {
        match uart.getc() {
            Some(c) => {
                critical_section::with(|cs| {
                    UART_RX.borrow(cs).borrow_mut().push_back(c);
                });
            }
            _ => break,
        }
    }
}

/// Entrypoint
///
/// # Safety
///
/// This function is safe to call exactly once.
#[export_name = "rust_entry"]
pub unsafe extern "C" fn rust_entry() -> ! {
    crate::platform::early_init();
    crate::println!("\n~~boot0 up!~~\n");

    #[cfg(feature = "unsafe-dev")]
    let mut repl = crate::repl::Repl::new();
    // do the main loop through the serial port
    #[cfg(feature = "unsafe-dev")]
    loop {
        // Handle keyboard events.
        critical_section::with(|cs| {
            let mut queue = UART_RX.borrow(cs).borrow_mut();
            while let Some(byte) = queue.pop_front() {
                repl.rx_char(byte);
            }
        });

        // Process any command line requests
        match repl.process() {
            Err(e) => {
                if let Some(m) = e.message {
                    crate::println!("{}", m);
                    repl.abort_cmd();
                }
            }
            _ => (),
        };
    }
    // self-validate the image with the keys we put in, just to make sure our code wasn't tampered with
    if bao1x_hal::sigcheck::validate_image(
        bao1x_api::BOOT0_START as *const u32,
        bao1x_api::BOOT0_START as *const u32,
        bao1x_api::BOOT0_REVOCATION_OFFSET,
        &[FunctionCode::Boot0 as u32], // only boot0 is allowed for boot0
        false,
    )
    .is_ok()
    {
        seal_boot0_keys();
        let allowed_functions =
            [FunctionCode::Boot1 as u32, FunctionCode::UpdatedBoot1 as u32, FunctionCode::Developer as u32];
        let one_way_access = bao1x_hal::acram::OneWayCounter::new();
        match one_way_access.get_decoded::<bao1x_api::AltBootCoding>() {
            Ok(bao1x_api::AltBootCoding::PrimaryPartition) => {
                // Primary boot selected. Check Boot1 first, then fall back to LOADER/BAREMETAL.
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                )
                .ok();
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::LOADER_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                )
                .ok();
            }
            Ok(bao1x_api::AltBootCoding::AlternatePartition) => {
                // Alternate boot selected. Check LOADER/BAREMETAL, then fall back to Boot1.
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::LOADER_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                )
                .ok();
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                )
                .ok();
            }
            Err(_) => {
                crate::println!("Internal error: alt boot encoding is invalid!");
                bao1x_hal::sigcheck::die_no_std();
            }
        }
    }
    bao1x_hal::sigcheck::die_no_std();
}

fn seal_boot0_keys() {
    // TODO:
    //  - setup an initial coreuser table
    //  - set an ASID that locks out any boot0 secrets (currently none, as it's PK based)
    //  - this does not offer strong security, but prevents someone with an arbitrary read primitive from
    //    accessing any boot0 secrets. An arbitrary-exec primitive at this point can, of course, undo the ASID
    //    mapping.
    //
    // Only necessary if we have secrets to seal. The current implementation only contains a public key,
    // so there's no secrets to seal.
}
