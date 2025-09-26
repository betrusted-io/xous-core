use alloc::string::String;

use bao1x_api::signatures::*;
use bao1x_hal::acram::OneWayCounter;
use digest::Digest;
use sha2_bao1x::Sha512;

/// Offset in the one-way counter array for key revocations
pub const REVOCATION_OFFSET: usize = 124;

/// Current draw @ 200MHz CPU ACLK (400MHz FCLK), VDD85 = 0.80V nom (measured @0.797V): ~71mA peak @ 27C,
/// measured on VDD33 (LDO path places strict upper bounds on IDD85). Target: < 100mA under all PVT.
///
/// Other notes: active current is only 2-4mA over idle current for this loop, so idle current is a good
/// proxy for current draw in boot0.
///
/// Why this matters: boot0 has to boot the chip under all modes. An external power source is needed for
/// IDD85 > 100mA. Thus we can't boot at max speed config, as not all system configurations have the
/// external regulator. So, we have to work at reduced VDD/frequency and make sure this constraint is met.
pub fn validate_image(img_offset: *const u32) -> Result<(), String> {
    // conjure the signature struct directly out of memory. super unsafe.
    let sig_ptr = img_offset as *const SignatureInFlash;
    let sig: &SignatureInFlash = unsafe { sig_ptr.as_ref().unwrap() };

    let signed_len = sig.sealed_data.signed_len;
    let image: &[u8] = unsafe {
        core::slice::from_raw_parts((img_offset as usize + UNSIGNED_LEN) as *const u8, signed_len as usize)
    };

    if sig.sealed_data.version != BAOCHIP_SIG_VERSION {
        crate::println!(
            "Version {:x} sig found, should be {:x}",
            sig.sealed_data.version,
            BAOCHIP_SIG_VERSION
        );
        return Err(String::from("invalid sigblock version"));
    }

    // checking the function code prevents exploiting code meant for other partitions signed
    // with a valid signature as code for the next stage boot.
    if !(sig.sealed_data.function_code == FunctionCode::Boot0 as u32
        || sig.sealed_data.function_code == FunctionCode::Boot1 as u32
        || sig.sealed_data.function_code == FunctionCode::UpdatedBoot1 as u32)
    {
        return Err(String::from("Partition has invalid function code"));
    }

    let one_way_counters = OneWayCounter::new();
    let mut valid = false;
    let mut secure = false;
    for (i, key) in sig.sealed_data.pubkeys.iter().enumerate() {
        if key.iter().all(|&x| x == 0) {
            continue;
        }
        let revocation_value = one_way_counters.get(REVOCATION_OFFSET + i).expect("internal error");
        if revocation_value != 0 {
            crate::println!("Key at index {} is revoked ({}), skipping", i, revocation_value);
            continue;
        }

        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(key).or(Err(String::from("invalid public key")))?;

        let ed25519_signature = ed25519_dalek::Signature::from(sig.signature);
        let mut h: Sha512 = Sha512::new();
        h.update(&image);
        // debugging note: h.clone() does *not* work. You have to print the hash by modifying
        // the function inside the ed25519 crate.
        match verifying_key.verify_prehashed(h, None, &ed25519_signature) {
            Ok(_) => {
                if i != sig.sealed_data.pubkeys.len() - 1 {
                    valid = true;
                    secure = true;
                    break;
                } else if i == sig.sealed_data.pubkeys.len() - 1 {
                    // this is the developer key slot
                    valid = true;
                    secure = false;
                    break;
                }
            }
            _ => {}
        }
    }

    if !secure {
        erase_secrets();
    }
    if valid { Ok(()) } else { Err(String::from("No valid pubkeys found or signature invalid")) }
}

fn erase_secrets() {
    // actually, this should check first if the data is 0, then 0 it. Can't afford
    // to wear-out the memory every boot by repeatedly overwriting it...
    crate::println!("TODO: erase secret data stores on boot to devmode!");
}
