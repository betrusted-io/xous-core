use bao1x_api::{signatures::FunctionCode, *};
use bao1x_hal::board::KeyPress;

const ALLOWED_FUNCTIONS: [u32; 5] = [
    FunctionCode::Baremetal as u32,
    FunctionCode::UpdatedBaremetal as u32,
    FunctionCode::Loader as u32,
    FunctionCode::UpdatedLoader as u32,
    FunctionCode::Developer as u32,
];
pub fn try_boot<T: IoSetup + IoGpio>(board_type: &BoardTypeCoding, iox: &T) -> Option<KeyPress> {
    if let Some(key) = crate::platform::get_key(board_type, iox) {
        // TODO: on baosec v2, we should not get Invalid keys. However, as we wait for the new
        // boards to come in this will be a thing.
        if key != KeyPress::Invalid {
            // skip boot if a key is pressed; record what key it is so we know to check that it has
            // become *unpressed* before looking for a new press
            return Some(key);
        }
    }

    let one_way = bao1x_hal::acram::OneWayCounter::new();
    if one_way.get_decoded::<BootWaitCoding>().expect("internal error") == BootWaitCoding::Enable {
        // enter repl, but indicate that no key was pressed.
        return None;
    }

    // do the secure signature stuff now
    seal_boot1_keys();
    // loader is at the same offset as baremetal. Accept either as valid boot.
    // This diverges if the signature check is successful
    match bao1x_hal::sigcheck::validate_image(
        bao1x_api::LOADER_START as *const u32,
        bao1x_api::BOOT1_START as *const u32,
        bao1x_api::BOOT1_REVOCATION_OFFSET,
        &ALLOWED_FUNCTIONS,
        true,
    ) {
        Ok(k) => crate::println!("**should be unreachable** Booted with key {}", k),
        Err(e) => crate::println!("Image did not validate: {:?}", e),
    }
    crate::println!("No valid loader or baremetal image found. Halting!");
    bao1x_hal::sigcheck::die_no_std();
}

fn seal_boot1_keys() {
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

pub fn boot_or_die() -> ! {
    seal_boot1_keys();
    // loader is at the same offset as baremetal. Accept either as valid boot.
    // This diverges if the signature check is successful
    match bao1x_hal::sigcheck::validate_image(
        bao1x_api::LOADER_START as *const u32,
        bao1x_api::BOOT1_START as *const u32,
        bao1x_api::BOOT1_REVOCATION_OFFSET,
        &ALLOWED_FUNCTIONS,
        true,
    ) {
        Ok(k) => crate::println!("**should be unreachable** Booted with key {}", k),
        Err(e) => crate::println!("Image did not validate: {:?}", e),
    }
    crate::println!("No valid loader or baremetal image found. Halting!");
    bao1x_hal::sigcheck::die_no_std();
}
