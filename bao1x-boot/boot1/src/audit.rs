use core::convert::TryInto;

use bao1x_api::pubkeys::{BOOT0_SELF_CHECK, BOOT0_TO_BOOT1, BOOT1_TO_LOADER_OR_BAREMETAL};
use bao1x_api::signatures::{SignatureInFlash, UNSIGNED_LEN};
use bao1x_api::*;
use bao1x_hal::acram::OneWayCounter;
use bao1x_hal::sigcheck::ERASE_VALUE;
use digest::Digest;
use sha2_bao1x::Sha512;

#[cfg(not(feature = "alt-boot1"))]
pub fn early_audit() {
    let owc = bao1x_hal::acram::OneWayCounter::new();
    let boot_count = owc.get(bao1x_api::EARLY_BOOT_COUNT).unwrap_or(bao1x_api::AUTO_AUDIT_LIMIT);
    if boot_count < AUTO_AUDIT_LIMIT {
        audit();
        // safety: the constant is vetted to be in-range
        unsafe { owc.inc(bao1x_api::EARLY_BOOT_COUNT).ok() };
    }
}

fn hash_region(region: &[u8], description: &str) {
    let mut hasher = Sha512::new();
    hasher.update(&region);
    let digest: [u8; 64] = hasher.finalize().try_into().unwrap();
    let mut buffer = [0u8; 128];
    hex::encode_to_slice(digest, &mut buffer).unwrap();
    let hex_str = core::str::from_utf8(&buffer).unwrap();
    crate::println!("{}: {}", description, hex_str);
}

pub fn audit() {
    let owc = OneWayCounter::new();
    let boardtype = owc.get_decoded::<BoardTypeCoding>().unwrap();
    crate::println!("Board type reads as: {:?}", boardtype);
    crate::println!("Boot partition is: {:?}", owc.get_decoded::<AltBootCoding>());
    crate::println!("Semver is: {}", crate::version::SEMVER);
    crate::println!("Description is: {}", crate::RELEASE_DESCRIPTION);
    let slot_mgr = bao1x_hal::acram::SlotManager::new();
    let sn = slot_mgr.read(&bao1x_hal::board::SERIAL_NUMBER).unwrap();
    crate::println!(
        "Device serializer: {:08x}-{:08x}-{:08x}-{:08x}",
        u32::from_le_bytes(sn[12..16].try_into().unwrap()),
        u32::from_le_bytes(sn[8..12].try_into().unwrap()),
        u32::from_le_bytes(sn[4..8].try_into().unwrap()),
        u32::from_le_bytes(sn[..4].try_into().unwrap())
    );
    crate::println!("Public serial number: {}", bao1x_hal::usb::derive_usb_serial_number(&owc, &slot_mgr));
    let uuid = slot_mgr.read(&bao1x_hal::board::UUID).unwrap();
    crate::println!(
        "UUID: {:08x}-{:08x}-{:08x}-{:08x}",
        u32::from_le_bytes(uuid[12..16].try_into().unwrap()),
        u32::from_le_bytes(uuid[8..12].try_into().unwrap()),
        u32::from_le_bytes(uuid[4..8].try_into().unwrap()),
        u32::from_le_bytes(uuid[..4].try_into().unwrap())
    );
    crate::println!(
        "Paranoid mode: {}/{}",
        owc.get(PARANOID_MODE).unwrap(),
        owc.get(PARANOID_MODE_DUPE).unwrap()
    );
    // this number may be non-zero because some of the sensors are on a hair-trigger
    crate::println!("Possible attack attempts: {}", owc.get(POSSIBLE_ATTACKS).unwrap());
    crate::println!("Revocations:");
    // only checks the main array, not the duplicate array
    crate::println!("Stage       key0     key1     key2     key3");
    let key_array = [
        ("boot0       ", BOOT0_REVOCATION_OFFSET),
        ("boot1       ", BOOT1_REVOCATION_OFFSET),
        ("next stage  ", LOADER_REVOCATION_OFFSET),
    ];
    for (img_type, start) in key_array {
        crate::print!("{}", img_type);
        for offset in start..(start + PUBKEY_SLOTS) {
            if owc.get(offset).unwrap_or(1) == 0 {
                crate::print!("enabled  ");
            } else {
                crate::print!("revoked  ");
            }
        }
        crate::println!("");
    }

    match bao1x_hal::sigcheck::validate_image(BOOT0_SELF_CHECK, None, None) {
        Ok((k, k2, tag, target)) => crate::println!(
            "Boot0: key {}/{} ({}) -> {:x}",
            k,
            !k2,
            core::str::from_utf8(&tag).unwrap_or("invalid tag"),
            target ^ u32::from_le_bytes(tag)
        ),
        Err(e) => crate::println!("Boot0 did not validate: {:?}", e),
    }
    match bao1x_hal::sigcheck::validate_image(BOOT0_TO_BOOT1, None, None) {
        Ok((k, k2, tag, target)) => crate::println!(
            "Boot1: key {}/{} ({}) -> {:x}",
            k,
            !k2,
            core::str::from_utf8(&tag).unwrap_or("invalid tag"),
            target ^ u32::from_le_bytes(tag)
        ),
        Err(e) => crate::println!("Boot1 did not validate: {:?}", e),
    }
    match bao1x_hal::sigcheck::validate_image(BOOT1_TO_LOADER_OR_BAREMETAL, None, None) {
        Ok((k, k2, tag, target)) => crate::println!(
            "Next stage: key {}/{} ({}) -> {:x}",
            k,
            !k2,
            core::str::from_utf8(&tag).unwrap_or("invalid tag"),
            target ^ u32::from_le_bytes(tag)
        ),
        Err(e) => crate::println!("Next stage did not validate: {:?}", e),
    }

    // leak info about the erase proof coupon - this makes CI testing *so much easier*
    // the erase proof coupon is a "key" that's treated in a similar fashion in terms of lifecyle
    // to all the other keys, except its purpose is to be inspected.
    let erase_check = slot_mgr.read(&bao1x_hal::board::ERASE_PROOF).unwrap();
    // check length is a subset of the full 256-bit key, just because...why leak the whole thing
    // if you don't need to? 96 bits is a pretty big collision space for a single pair of values.
    const CHECK_LEN: usize = 12;
    let erased_state = [ERASE_VALUE; CHECK_LEN];
    let uninit_state = [0u8; CHECK_LEN];
    if &erase_check[..CHECK_LEN] == &erased_state {
        crate::println!("Erase proof: erased");
    } else if &erase_check[..CHECK_LEN] == &uninit_state {
        crate::println!("Erase proof: uninit or access denied");
    } else {
        crate::println!("Erase proof: not erased");
    }

    // sanity check the auto-audit limit
    crate::println!("auto-audit limit: {}", owc.get(EARLY_BOOT_COUNT).unwrap());
    // hash reports
    let boot0_region = unsafe {
        core::slice::from_raw_parts(
            bao1x_api::BOOT0_START as *const u8,
            bao1x_api::BOOT1_START - bao1x_api::BOOT0_START,
        )
    };
    // includes free space in the partition
    hash_region(boot0_region, "boot0 partition");

    let b0_pk_ptr = bao1x_api::BOOT0_START as *const SignatureInFlash;
    let b0_pk: &SignatureInFlash = unsafe { b0_pk_ptr.as_ref().unwrap() };
    let boot0_used = unsafe {
        core::slice::from_raw_parts(
            (bao1x_api::BOOT0_START + UNSIGNED_LEN) as *const u8,
            b0_pk.sealed_data.signed_len as usize,
        )
    };
    // only the portion that's protected by signature
    hash_region(boot0_used, "boot0 code only");

    let boot1_region = unsafe {
        core::slice::from_raw_parts(
            bao1x_api::BOOT1_START as *const u8,
            bao1x_api::LOADER_START - bao1x_api::BOOT1_START,
        )
    };
    // includes free space
    hash_region(boot1_region, "boot1 partition");

    // detailed state checks
    let mut secure = true;
    // check that boot1 pubkeys match the indelible entries
    let pubkey_ptr = bao1x_api::BOOT1_START as *const bao1x_api::signatures::SignatureInFlash;
    let pk_src: &bao1x_api::signatures::SignatureInFlash = unsafe { pubkey_ptr.as_ref().unwrap() };
    let reference_keys =
        [bao1x_api::BAO1_PUBKEY, bao1x_api::BAO2_PUBKEY, bao1x_api::BETA_PUBKEY, bao1x_api::DEV_PUBKEY];
    let slot_mgr = bao1x_hal::acram::SlotManager::new();
    let mut good_compare = true;
    for (boot0_key, ref_key) in pk_src.sealed_data.pubkeys.iter().zip(reference_keys.iter()) {
        let ref_data = slot_mgr.read(&ref_key).unwrap();
        if ref_data != &boot0_key.pk {
            good_compare = false;
        }
    }
    if !good_compare {
        crate::println!("== BOOT1 FAILED PUBKEY CHECK ==");
        // this may "not" be a security failure if boot1 was intentionally replaced
        // but in that case, the developer customizing the image should have also edited this
        // check out.
        secure = false;
    }
    // check developer mode
    if owc.get(DEVELOPER_MODE).unwrap() != 0 {
        crate::println!("== IN DEVELOPER MODE ==");
        secure = false;
    }
    if owc.get(BOOT0_PUBKEY_FAIL).unwrap() != 0 {
        crate::println!("== BOOT0 REPORTED PUBKEY CHECK FAILURE ==");
        secure = false;
    }
    if owc.get(BOOT1_PUBKEY_FAIL).unwrap() != 0 {
        crate::println!("== BOOT1 REPORTED PUBKEY CHECK FAILURE ==");
        secure = false;
    }
    if owc.get(CP_BOOT_SETUP_DONE).unwrap() == 0 {
        crate::println!("== CP SETUP FAILED ==");
        secure = false;
    }
    if owc.get(IN_SYSTEM_BOOT_SETUP_DONE).unwrap() == 0 {
        crate::println!("In-system keys have NOT been generated");
        secure = false;
    } else {
        crate::println!("In-system keys have been generated");
    }
    let ifr = unsafe { core::slice::from_raw_parts(0x6040_0180 as *const u8, 0x10) };
    let ref_value =
        [0x00u8, 0x00, 0x00, 0x00, 0x82, 0x8c, 0x42, 0x6a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    if ifr == &ref_value {
        crate::println!("CM7 & debug confirmed fused off");
    } else {
        crate::println!("Factory configuration error - CM7 or debug is enabled!");
        secure = false;
    }
    let collateral = slot_mgr.read(&COLLATERAL).unwrap();
    let check_val = alloc::vec![bao1x_hal::sigcheck::ERASE_VALUE; COLLATERAL.len() * SLOT_ELEMENT_LEN_BYTES];
    let uninit_val = alloc::vec![0; COLLATERAL.len() * SLOT_ELEMENT_LEN_BYTES];
    // these strings below are used in CI. If they are changed, CI needs to be updated
    if collateral == &check_val {
        crate::println!("Collateral erased");
    } else if collateral == &uninit_val {
        crate::println!("Collateral is uninitialized or access is denied");
    } else {
        crate::println!("Collateral is set");
    }
    let boot1_block_ptr = bao1x_api::BOOT1_START as *const bao1x_api::signatures::SignatureInFlash;
    let boot1_block: &bao1x_api::signatures::SignatureInFlash = unsafe { boot1_block_ptr.as_ref().unwrap() };
    let mut receipts_ok = true;
    for (key, slot) in boot1_block.sealed_data.pubkeys.iter().zip(BOOT1_RECEIPT_SLOTS) {
        let receipt = slot_mgr.read(&slot).unwrap();
        if &key.pk != receipt {
            receipts_ok = false;
            break;
        }
    }
    if !receipts_ok {
        crate::println!("Boot1 receipts do not match");
        secure = false;
    } else {
        crate::println!("Boot1 receipts OK");
    }
    let claimed_function: bao1x_api::signatures::FunctionCode = boot1_block
        .sealed_data
        .function_code
        .try_into()
        .unwrap_or(bao1x_api::signatures::FunctionCode::Invalid);
    let arb_offset = claimed_function.to_anti_rollback_counter();
    match arb_offset {
        Some(arb) => {
            let arb_value = owc.get(arb).expect("Can't read anti-rollback value");
            if arb_value != boot1_block.sealed_data.anti_rollback {
                crate::println!(
                    "Anti-rollback code mismatch! Counter {} / image {}",
                    arb_value,
                    boot1_block.sealed_data.anti_rollback
                );
                secure = false;
            } else {
                crate::println!("Boot1 anti-rollback OK");
            }
        }
        None => {
            crate::println!("Anti-rollback counter offset invalid!");
            secure = false;
        }
    }

    if !secure {
        crate::println!("** System did not meet minimum requirements for security **");
    }
}
