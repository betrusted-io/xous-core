use bao1x_api::signatures::FunctionCode;

pub const ALLOWED_FUNCTIONS: [u32; 5] = [
    FunctionCode::Baremetal as u32,
    FunctionCode::UpdatedBaremetal as u32,
    FunctionCode::Loader as u32,
    FunctionCode::UpdatedLoader as u32,
    FunctionCode::Developer as u32,
];

fn seal_boot1_keys() {
    // This is a security-critical initialization. Failure to do this correctly totally breaks
    // the hardware access control scheme for key/data slots.
    //
    // However, this does not offer unbreakable security. Rather, it prevents someone with an arbitrary read
    // primitive from accessing secret keys. An arbitrary-exec primitive can, of course, forge the ASID
    // and work around the coreuser mechanism.
    let mut cu = bao1x_hal::coreuser::Coreuser::new();
    cu.set(); // re-sets the coreuser settings. Just in case they got modified along the way...
    // locks out future modifications to the Coreuser setting. Also inverts mm sense, which means you must
    // enter a virtual memory user state to access sealed keys.
    cu.protect();
}

pub fn boot_or_die() -> ! {
    // loader is at the same offset as baremetal. Accept either as valid boot.
    // This diverges if the signature check is successful
    match bao1x_hal::sigcheck::validate_image(
        bao1x_api::LOADER_START as *const u32,
        bao1x_api::BOOT1_START as *const u32,
        bao1x_api::BOOT1_REVOCATION_OFFSET,
        &ALLOWED_FUNCTIONS,
        false,
        None,
    ) {
        Ok((k, tag)) => {
            crate::println!(
                "Booting with key {}({})",
                k,
                core::str::from_utf8(&tag).unwrap_or("invalid tag")
            );
            // the tag is from signed, trusted data
            // k is just a nominal slot number. If either match, assume we are dealing with a
            // developer image.
            if tag == *bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]
                || k == bao1x_api::pubkeys::DEVELOPER_KEY_SLOT
            {
                crate::println!("Developer key detected, erasing secrets (broken in alpha-0 release)");
                bao1x_hal::sigcheck::erase_secrets();
            }
            // this has to be called *after* erase_secrets, because we can't erase the secrets
            // once the mappings have been sealed off. This is why we can't use the auto-jump method
            // like we do in boot0.
            seal_boot1_keys();
            bao1x_hal::sigcheck::jump_to(bao1x_api::LOADER_START);
        }
        Err(e) => crate::println!("Image did not validate: {:?}", e),
    }
    crate::println!("No valid loader or baremetal image found. Halting!");
    bao1x_hal::sigcheck::die_no_std();
}
