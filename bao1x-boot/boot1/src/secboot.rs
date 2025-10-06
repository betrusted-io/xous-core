use bao1x_api::signatures::FunctionCode;

const ALLOWED_FUNCTIONS: [u32; 5] = [
    FunctionCode::Baremetal as u32,
    FunctionCode::UpdatedBaremetal as u32,
    FunctionCode::Loader as u32,
    FunctionCode::UpdatedLoader as u32,
    FunctionCode::Developer as u32,
];

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
        None,
    ) {
        Ok((k, tag)) => crate::println!(
            "**should be unreachable** Booted with key {}({})",
            k,
            core::str::from_utf8(&tag).unwrap_or("invalid tag")
        ),
        Err(e) => crate::println!("Image did not validate: {:?}", e),
    }
    crate::println!("No valid loader or baremetal image found. Halting!");
    bao1x_hal::sigcheck::die_no_std();
}
