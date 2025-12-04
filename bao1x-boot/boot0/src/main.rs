#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;
// contains runtime setup
mod asm;

mod platform;
#[cfg(feature = "unsafe-dev")]
mod repl;
mod version;
#[cfg(feature = "unsafe-dev")]
use alloc::collections::VecDeque;
#[cfg(feature = "unsafe-dev")]
use core::cell::RefCell;

use bao1x_api::HardenedBool;
use bao1x_api::{BOOT0_PUBKEY_FAIL, bollard, signatures::FunctionCode};
use bao1x_hal::acram::OneWayCounter;
use bao1x_hal::hardening::{check_pll, die, mesh_setup};
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
    // set the security bits on the RRAM - without these set, most security is bypassed
    // this issue is fixed on A1 silicon stepping (which is production silicon)
    // glitch_safety: this is fixed by metal mask so no hardening is needed
    let mut rram = CSR::new(utra::rrc::HW_RRC_BASE as *mut u32);
    rram.wfo(utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);

    // set user level so we can access keys.
    // glitch_safety: failing to execute this would actually reduce access permissions
    let mut cu = bao1x_hal::coreuser::Coreuser::new();
    cu.set();

    let mut csprng = crate::platform::early_init();
    csprng.random_delay();

    bollard!(4);
    // Mesh check takes 100ms for the signal to propagate. Setup the mesh check here, then check the
    // result in boot1. In boot1, the opposite state (`true`) is checked.
    mesh_setup(false, None);
    bollard!(4);

    crate::println!("\n~~boot0 up! ({})~~\n", crate::version::SEMVER);
    csprng.random_delay(); // always random-delay after printing

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

    // useful for CI, checking samples - print the IFR region
    #[cfg(feature = "print-ifr")]
    print_ifr();

    // glitch_safety: another delay and PLL check, for good measure
    csprng.random_delay();
    check_pll();
    bollard!(die, 4);

    let owc = OneWayCounter::new();
    // check that the boot0 pub keys match those burned into the indelible key area
    // glitch_safety: I'd imagine that glitching in this routine would lead to good_compare being `false`,
    // so no additional hardening is done.
    let pubkey_ptr = bao1x_api::BOOT0_START as *const bao1x_api::signatures::SignatureInFlash;
    let pk_src: &bao1x_api::signatures::SignatureInFlash = unsafe { pubkey_ptr.as_ref().unwrap() };
    let reference_keys =
        [bao1x_api::BAO1_PUBKEY, bao1x_api::BAO2_PUBKEY, bao1x_api::BETA_PUBKEY, bao1x_api::DEV_PUBKEY];
    let slot_mgr = bao1x_hal::acram::SlotManager::new();
    let mut good_compare = HardenedBool::TRUE;
    for (boot0_key, ref_key) in pk_src.sealed_data.pubkeys.iter().zip(reference_keys.iter()) {
        let ref_data = slot_mgr.read(&ref_key).unwrap();
        if ref_data != &boot0_key.pk {
            good_compare = HardenedBool::FALSE;
        }
    }

    csprng.random_delay();
    match good_compare.is_true() {
        Some(false) => {
            bollard!(die, 4);
            // safety: the offset is from a pre-validated constant, which meets the safety requirement
            unsafe {
                owc.inc(BOOT0_PUBKEY_FAIL).unwrap();
            }
            // erase secrets if the boot0 pubkey doesn't check out.
            bollard!(die, 4);
            bao1x_hal::sigcheck::erase_secrets(&mut Some(&mut csprng));
        }
        Some(true) => (),
        None => die(),
    }
    bollard!(die, 4);

    // self-validate the image with the keys we put in, just to make sure our code wasn't tampered with
    if bao1x_hal::sigcheck::validate_image(
        bao1x_api::BOOT0_START as *const u32,
        bao1x_api::BOOT0_START as *const u32,
        bao1x_api::BOOT0_REVOCATION_OFFSET,
        &[FunctionCode::Boot0 as u32], // only boot0 is allowed for boot0
        false,
        None,
        Some(&mut csprng),
    )
    .is_ok()
    {
        seal_boot0_keys();
        bollard!(die, 4);

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
                    None,
                    Some(&mut csprng),
                )
                .ok();
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::LOADER_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                    None,
                    Some(&mut csprng),
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
                    None,
                    Some(&mut csprng),
                )
                .ok();
                bao1x_hal::sigcheck::validate_image(
                    bao1x_api::BOOT1_START as *const u32,
                    bao1x_api::BOOT0_START as *const u32,
                    bao1x_api::BOOT0_REVOCATION_OFFSET,
                    &allowed_functions,
                    true,
                    None,
                    Some(&mut csprng),
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
    //  - set an ASID that locks out any boot0 secrets (currently none, as it's PK based)
    //  - this does not offer strong security, but prevents someone with an arbitrary read primitive from
    //    accessing any boot0 secrets. An arbitrary-exec primitive at this point can, of course, undo the ASID
    //    mapping.
    //
    // Only necessary if we have secrets to seal. The current implementation only contains a public key,
    // so there's no secrets to seal.
}

#[cfg(feature = "print-ifr")]
fn print_ifr() {
    let coreuser = utralib::CSR::new(utralib::utra::coreuser::HW_COREUSER_BASE as *mut u32);
    // needs to be 0x118 for IFR to be readable when the protection bit is set.
    crate::println!("coreuser status: {:x}", coreuser.r(utralib::utra::coreuser::STATUS));

    let ifr = unsafe { core::slice::from_raw_parts(0x6040_0000 as *const u32, 0x100) };
    for (i, &d) in ifr.iter().enumerate() {
        if i % 8 == 0 {
            crate::println!("");
            crate::print!("{:04x}: ", i * 4);
        }
        crate::print!("{:08x} ", d);
    }
    crate::println!("");
}
