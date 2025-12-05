use bao1x_api::{PARANOID_MODE, PARANOID_MODE_DUPE, bollard, pubkeys::BOOT1_TO_LOADER_OR_BAREMETAL};
use bao1x_hal::hardening::Csprng;

#[inline(always)]
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

pub fn try_boot(or_die: bool, csprng: &mut Csprng) {
    let one_way = bao1x_hal::acram::OneWayCounter::new();
    bollard!(bao1x_hal::sigcheck::die_no_std, 4);
    csprng.random_delay();
    let (paranoid1, paranoid2) = one_way.hardened_get2(PARANOID_MODE, PARANOID_MODE_DUPE).unwrap();
    csprng.random_delay();

    // loader is at the same offset as baremetal. Accept either as valid boot.
    // This diverges if the signature check is successful
    match bao1x_hal::sigcheck::validate_image(BOOT1_TO_LOADER_OR_BAREMETAL, None, Some(csprng)) {
        Ok((key, key_inv, tag, target)) => {
            if paranoid1 == 0 && paranoid2 == 0 {
                // only emit prints if not in paranoid mode
                crate::println!(
                    "Booting with key {}/{}({})",
                    key,
                    !key_inv,
                    core::str::from_utf8(&tag).unwrap_or("invalid tag")
                );
            }
            if key != !key_inv {
                bao1x_hal::sigcheck::die_no_std();
            }

            // disable IRQs in preparation for next phase
            crate::platform::irq::disable_all_irqs();

            // the tag is from signed, trusted data
            // k is just a nominal slot number. If either match, assume we are dealing with a
            // developer image.
            bao1x_hal::sigcheck::hardened_erase_policy(paranoid1, paranoid2, key, key_inv, tag, csprng)
                .inspect_err(|e| crate::println!("{}", e))
                .ok(); // "ok" because the expected error is a check on logic/configuration bugs, not attacks

            // this print message is not hardened, and it's actually retrospective of the policy
            // implementation
            if tag == *bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]
                || key == bao1x_api::pubkeys::DEVELOPER_KEY_SLOT
                || !key_inv == bao1x_api::pubkeys::DEVELOPER_KEY_SLOT
            {
                crate::println!("Developer key detected, ensuring secrets are erased");
            }

            // double up the security check if in paranoid mode
            csprng.random_delay();
            if paranoid1 != 0 || paranoid2 != 0 {
                bollard!(bao1x_hal::sigcheck::die_no_std, 4);
                bao1x_hal::sigcheck::validate_image(BOOT1_TO_LOADER_OR_BAREMETAL, None, Some(csprng))
                    .unwrap_or_else(|_| bao1x_hal::hardening::die());
            }

            csprng.random_delay();
            bollard!(bao1x_hal::sigcheck::die_no_std, 4);
            // this has to be called *after* erase_secrets, because we can't erase the secrets
            // once the mappings have been sealed off. This is why we can't use the auto-jump method
            // like we do in boot0.
            seal_boot1_keys();
            bollard!(bao1x_hal::sigcheck::die_no_std, 4);
            bao1x_hal::sigcheck::jump_to((target ^ u32::from_le_bytes(tag)) as usize);
        }
        Err(e) => crate::println!("Image did not validate: {:?}", e),
    }
    if or_die {
        crate::println!("No valid loader or baremetal image found. Halting!");
        bao1x_hal::sigcheck::die_no_std();
    }
}
