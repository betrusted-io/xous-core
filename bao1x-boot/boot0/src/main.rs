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

use bao1x_api::pubkeys::{
    BOOT0_SELF_CHECK, BOOT0_TO_ALTBOOT1, BOOT0_TO_BOOT1, DEVELOPER_KEY_SLOT, KEYSLOT_INITIAL_TAGS,
};
use bao1x_api::{
    BOOT0_PUBKEY_FAIL, BOOT1_PUBKEY_FAIL, BOOT1_RECEIPT_SLOTS, DEVELOPER_MODE, HardenedBool, bollard,
};
use bao1x_api::{PARANOID_MODE, PARANOID_MODE_DUPE};
use bao1x_hal::acram::OneWayCounter;
use bao1x_hal::hardening::{
    Csprng, check_pll, die, disable_clock_skipping, mesh_setup, reseed_skipping, setup_clock_skipping,
};
use bao1x_hal::sigcheck::{hardened_erase_policy, jump_to};
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
    // glitch_safety: it is safe to write this register multiple times, and so it is.
    let mut rram = CSR::new(utra::rrc::HW_RRC_BASE as *mut u32);
    rram.wfo(utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);

    // set user level so we can access keys.
    // glitch_safety: failing to execute this would actually reduce access permissions
    let mut cu = bao1x_hal::coreuser::Coreuser::new();
    cu.set();

    let mut csprng = crate::platform::early_init();
    csprng.random_delay();
    rram.wfo(utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);

    bollard!(bao1x_hal::sigcheck::die_no_std, 4);
    // Mesh check takes 100ms for the signal to propagate. Setup the mesh check here, then check the
    // result in boot1. In boot1, the opposite state (`true`) is checked.
    mesh_setup(false, None);
    bollard!(bao1x_hal::sigcheck::die_no_std, 4);
    csprng.random_delay();
    bollard!(bao1x_hal::sigcheck::die_no_std, 4);
    rram.wfo(utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);
    bollard!(bao1x_hal::sigcheck::die_no_std, 4);

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
    let slot_mgr = bao1x_hal::acram::SlotManager::new();

    // check that the pubkeys in boot0 matches the reference keys
    match bao1x_hal::hardening::compare_refkeys(
        &owc,
        &slot_mgr,
        &mut csprng,
        bao1x_api::BOOT0_START as *const bao1x_api::signatures::SignatureInFlash,
        BOOT0_PUBKEY_FAIL,
    )
    .is_true()
    {
        Some(true) => (),
        // if comparison is false or None, erase secrets
        _ => {
            bao1x_hal::sigcheck::erase_secrets(&mut Some(&mut csprng))
                .inspect_err(|e| crate::println!("{}", e))
                .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
        }
    };
    bollard!(die, 4);

    let use_skipping = setup_clock_skipping(csprng.get_u32());
    let (paranoid1, paranoid2) = owc.hardened_get2(PARANOID_MODE, PARANOID_MODE_DUPE).unwrap();

    // == self-validate the image with the keys we put in, just to make sure our code wasn't tampered with ==
    let boot0_check1 =
        bao1x_hal::sigcheck::validate_image(BOOT0_SELF_CHECK, None, Some(&mut csprng), HardenedBool::FALSE)
            .unwrap_or_else(|_| die());
    csprng.random_delay();
    if paranoid1 != paranoid2 || boot0_check1.0 != !boot0_check1.1 {
        die();
    }
    bollard!(die, 4);
    csprng.random_delay();
    if paranoid1 != 0 || paranoid2 != 0 {
        // do a redundant boot0 check if paranoid mode.
        let boot0_check2 = bao1x_hal::sigcheck::validate_image(
            BOOT0_SELF_CHECK,
            None,
            Some(&mut csprng),
            HardenedBool::FALSE,
        )
        .unwrap_or_else(|_| die());
        csprng.random_delay();
        if boot0_check2.0 != !boot0_check2.1 {
            die();
        }
    }
    bollard!(die, 4);
    // == end self validation ==

    // If the developer bit is set, ensure that keys are erased. The edge case we're worried about is
    // if an attacker sets developer bit, even with signed images - this can allow for an easier
    // time of booting a malicious kernel because we can't erase secret keys inside the loader
    // due to access restrictions.
    csprng.random_delay();
    let (dev1, dev2) = owc.hardened_get(DEVELOPER_MODE).unwrap();
    if dev1 != 0 {
        bao1x_hal::sigcheck::erase_secrets(&mut Some(&mut csprng))
            .inspect_err(|e| crate::println!("{}", e))
            .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
    }
    bollard!(die, 4);
    csprng.random_delay();
    if dev2 != 0 {
        bao1x_hal::sigcheck::erase_secrets(&mut Some(&mut csprng))
            .inspect_err(|e| crate::println!("{}", e))
            .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
    }

    bollard!(die, 4);
    let boot_order = match owc.get_decoded::<bao1x_api::AltBootCoding>() {
        // Primary boot selected. Check Boot1 first, then fall back to LOADER/BAREMETAL.
        Ok(bao1x_api::AltBootCoding::PrimaryPartition) => {
            match owc.get_decoded::<bao1x_api::Boot1DeveloperState>() {
                // no print on the Good case
                Ok(bao1x_api::Boot1DeveloperState::Good) => [BOOT0_TO_BOOT1, BOOT0_TO_ALTBOOT1],
                Ok(bao1x_api::Boot1DeveloperState::Staged) => {
                    crate::println!("Boot1DeveloperState Staged -> Attempted");
                    // roll forward to the attempted state. This should go from Staged -> Attempted
                    // a successful boot1 boot will bring us to Good
                    owc.inc_coded::<bao1x_api::Boot1DeveloperState>().ok();
                    [BOOT0_TO_BOOT1, BOOT0_TO_ALTBOOT1]
                }
                Ok(bao1x_api::Boot1DeveloperState::Attempted) => {
                    crate::println!("Boot1DeveloperState Attempted -> Bad");
                    // roll forward to the attempted state. This should go from Attempted -> Bad
                    owc.inc_coded::<bao1x_api::Boot1DeveloperState>().ok();
                    // if we return in an Attempted state, then, it was a failed boot. Fall back
                    // to the updater
                    [BOOT0_TO_ALTBOOT1, BOOT0_TO_BOOT1]
                }
                Ok(bao1x_api::Boot1DeveloperState::Bad) => {
                    crate::println!("Boot1DeveloperState is Bad");
                    [BOOT0_TO_ALTBOOT1, BOOT0_TO_BOOT1]
                }
                Err(_) => {
                    crate::println!("Internal error: boot1 update encoding is invalid!");
                    bao1x_hal::sigcheck::die_no_std();
                }
            }
        }
        // Alternate boot selected. Check LOADER/BAREMETAL, then fall back to Boot1.
        Ok(bao1x_api::AltBootCoding::AlternatePartition) => [BOOT0_TO_ALTBOOT1, BOOT0_TO_BOOT1],
        Err(_) => {
            crate::println!("Internal error: alt boot encoding is invalid!");
            bao1x_hal::sigcheck::die_no_std();
        }
    };
    for configuration in boot_order {
        bollard!(die, 4);
        csprng.random_delay();
        if use_skipping {
            reseed_skipping(csprng.get_u32());
        }
        match bao1x_hal::sigcheck::validate_image(configuration, None, Some(&mut csprng), HardenedBool::FALSE)
        {
            Ok((key, key_inv, tag, target, pq_tag)) => {
                if key != !key_inv {
                    die();
                }

                // check that the pubkeys in booting partition matches the reference keys
                match bao1x_hal::hardening::compare_refkeys(
                    &owc,
                    &slot_mgr,
                    &mut csprng,
                    configuration.image_ptr as *const bao1x_api::signatures::SignatureInFlash,
                    BOOT1_PUBKEY_FAIL,
                )
                .is_true()
                {
                    Some(true) => (),
                    // if comparison is false or None, erase secrets
                    _ => {
                        bao1x_hal::sigcheck::erase_secrets(&mut Some(&mut csprng))
                            .inspect_err(|e| crate::println!("{}", e))
                            .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
                    }
                };
                bollard!(die, 4);

                // implement mutual distrust comparison
                mutual_distrust(
                    key,
                    key_inv,
                    tag,
                    pq_tag,
                    configuration.image_ptr as usize,
                    &slot_mgr,
                    &mut csprng,
                );

                // implement the hardened erase policy. This is marked #[inline(always)].
                hardened_erase_policy(paranoid1, paranoid2, key, key_inv, tag, &mut csprng, pq_tag)
                    .inspect_err(|e| crate::println!("{}", e))
                    .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
                // second check if paranoid is not 0. While this branch can be glitched over, to get here,
                // you had to glitch into this routine.
                bollard!(die, 4);
                csprng.random_delay();
                if paranoid1 != 0 || paranoid2 != 0 {
                    // re-check the image: it *should* pass. If not, die.
                    bao1x_hal::sigcheck::validate_image(
                        configuration,
                        None,
                        Some(&mut csprng),
                        HardenedBool::FALSE,
                    )
                    .unwrap_or_else(|_| die());
                }
                csprng.random_delay();
                bollard!(die, 4);
                if use_skipping {
                    disable_clock_skipping();
                }
                jump_to(target as usize, u32::from_le_bytes(tag) as usize);
            }
            Err(e) => {
                crate::println!("Sigcheck err: {:?}", e);
                csprng.random_delay();
                bollard!(die, 4);
            }
        }
    }

    bollard!(die, 4);
    bao1x_hal::sigcheck::die_no_std();
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

#[inline(always)]
fn mutual_distrust(
    key: usize,
    key_inv: usize,
    tag: [u8; 4],
    pq_tag: Option<[u8; 4]>,
    block_start: usize,
    slot_mgr: &bao1x_hal::acram::SlotManager,
    mut csprng: &mut Csprng,
) {
    let block_ptr = block_start as *const bao1x_api::signatures::SignatureInFlash;
    let sig_block: &bao1x_api::signatures::SignatureInFlash = unsafe { block_ptr.as_ref().unwrap() };

    // In all cases - if developer key is active - erase the collateral. This plugs a hole where
    // it could be possible to bypass checks by stitching a third party header onto a developer-signed image.
    if key == DEVELOPER_KEY_SLOT
        || (!key_inv) == DEVELOPER_KEY_SLOT
        || &tag == KEYSLOT_INITIAL_TAGS[DEVELOPER_KEY_SLOT]
        || pq_tag == Some(*KEYSLOT_INITIAL_TAGS[DEVELOPER_KEY_SLOT])
    {
        bao1x_hal::sigcheck::erase_collateral(&mut Some(&mut csprng))
            .inspect_err(|e| crate::println!("{}", e))
            .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
    }

    // mutual distrust - don't trust Baochip. If any boot1 keys match those in the indelible array, erase the
    // collateral

    // The IFR (indelible) copy is weird, because the highest byte doesn't match (it's actually a flag that
    // indicates the region has to be write protected). Thus, the IFR keys are a set of four, disjointed
    // 31-byte memory areas, plus a collection of 4 bytes that correspond to the missing MSB.
    let ifr_keys = [
        unsafe { core::slice::from_raw_parts(0x6040_01A0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_01C0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_01E0 as *const u8, 31) },
        unsafe { core::slice::from_raw_parts(0x6040_0200 as *const u8, 31) },
    ];
    let ifr_msb = unsafe { core::slice::from_raw_parts(0x6040_0240 as *const u8, 4) };
    let mut any_matches = HardenedBool::FALSE;
    bollard!(die, 4);
    for (i, &ifr_key) in ifr_keys.iter().enumerate() {
        bollard!(die, 4);
        for boot1_key in sig_block.sealed_data.pubkeys.iter() {
            csprng.random_delay();
            bollard!(die, 4);
            if ifr_key == &boot1_key.pk[..31] && ifr_msb[i] == boot1_key.pk[31] {
                any_matches = HardenedBool::TRUE;
                // crate::println!("Got IFR match");
                break;
            }
        }
        if Some(true) == any_matches.is_true() {
            break;
        }
    }
    bollard!(die, 4);
    csprng.random_delay();
    match any_matches.is_true() {
        Some(false) => {
            // Third-party manifest counter-signature. Require it to be countersigned by one of its own keys,
            // otherwise a compromised Baochip key could staple a victim's manifest onto
            // hostile code and inherit their collateral.
            if Some(true)
                != bao1x_hal::sigcheck::check_counter_sig(block_start, &mut Some(&mut csprng)).is_true()
            {
                bao1x_hal::sigcheck::erase_collateral(&mut Some(&mut csprng))
                    .inspect_err(|e| crate::println!("{}", e))
                    .ok();
            }
            // mild hardening: do it twice. This is a relatively fast check because the hash is short.
            csprng.random_delay();
            bollard!(die, 4);
            if Some(true)
                != bao1x_hal::sigcheck::check_counter_sig(block_start, &mut Some(&mut csprng)).is_true()
            {
                bao1x_hal::sigcheck::erase_collateral(&mut Some(&mut csprng))
                    .inspect_err(|e| crate::println!("{}", e))
                    .ok();
                // this is printed after the second instance check, to avoid this being a glitch trigger
                // but this gives useful diagnostics in case the third party forgot to apply their
                // counter-signature
                crate::println!("Invalid counter-signature, erasing collateral.");
            }
        }
        _ => {
            // crate::println!("Collateral erase check!");
            bao1x_hal::sigcheck::erase_collateral(&mut Some(&mut csprng))
                .inspect_err(|e| crate::println!("{}", e))
                .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks
        }
    }
    bollard!(die, 4);

    // mutual distrust - don't trust other third party firmware. If the boot1 key block doesn't match the
    // previously captured receipt, erase collateral
    for (key, slot) in sig_block.sealed_data.pubkeys.iter().zip(BOOT1_RECEIPT_SLOTS) {
        csprng.random_delay();
        bollard!(die, 4);
        let receipt = slot_mgr.read(&slot).unwrap();
        if &key.pk != receipt {
            // first, erase collateral
            bao1x_hal::sigcheck::erase_collateral(&mut Some(&mut csprng))
                .inspect_err(|e| crate::println!("{}", e))
                .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks

            // next, copy all the keys into the receipt array
            let mut rram = bao1x_hal::rram::Reram::new();
            for (key, slot) in sig_block.sealed_data.pubkeys.iter().zip(BOOT1_RECEIPT_SLOTS) {
                csprng.random_delay();
                bollard!(die, 4);
                // make the error handling permissive so we don't fail to boot on coding errors
                match slot_mgr.write(&mut rram, &slot, &key.pk) {
                    Ok(_) => {}
                    Err(e) => crate::println!("Warning: couldn't update receipt slots {:?}", e),
                };
            }

            // finally break out of the loop - no need to compare any further
            break;
        }
    }
}
