extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use bao1x_api::REVOCATION_DUPE_DISTANCE;
use bao1x_api::bollard;
use bao1x_api::pubkeys::DEVELOPER_KEY_SLOT;
use bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS;
use bao1x_api::pubkeys::SecurityConfiguration;
use bao1x_api::signatures::*;
#[cfg(not(feature = "std"))]
use bao1x_api::{DEVELOPER_MODE, DataSlotAccess, RwPerms, SLOT_ELEMENT_LEN_BYTES, SlotType};
use digest::Digest;
use sha2_bao1x::{Sha256, Sha512};
use xous::arch::PAGE_SIZE;

use crate::acram::OneWayCounter;
#[cfg(not(feature = "std"))]
use crate::acram::{AccessSettings, SlotManager};
#[cfg(not(feature = "std"))]
use crate::buram::{BackupManager, ERASURE_PROOF_RANGE_BYTES};
use crate::hardening::Csprng;
use crate::udma::Spim;

// An erase value of 0 can be conflated with access permissions being incorrect. Choose a non-0 value
// for the erase value, but also, don't pick a 0-1-0-1 dense pattern because that can assist with
// calibrating microscopy techniques.
pub const ERASE_VALUE: u8 = 0x03;

/// Current draw @ 200MHz CPU ACLK (400MHz FCLK), VDD85 = 0.80V nom (measured @0.797V): ~71mA peak @ 27C,
/// measured on VDD33 (LDO path places strict upper bounds on IDD85). Target: < 100mA under all PVT.
///
/// Other notes: active current is only 2-4mA over idle current for this loop, so idle current is a good
/// proxy for current draw in boot0.
///
/// Why this matters: boot0 has to boot the chip under all modes. An external power source is needed for
/// IDD85 > 100mA. Thus we can't boot at max speed config, as not all system configurations have the
/// external regulator. So, we have to work at reduced VDD/frequency and make sure this constraint is met.
///
/// The following arguments are packed into a `SecurityConfiguration` record. These records are defined
/// in libs/bao1x-hal/src/pubkeys/mod.rs and are all of `const` type.
///     `img_offset` is a pointer to untrusted image data. It's assumed that the 0-offset of the pointer is
///     a `SignatureInFlash` structure.
///
///     `pubkeys_offset` is a pointer to trusted public key data. Because 'pubkeys_offset` is assumed to be
///     trusted minimal validation is done on this pointer. It's important that the caller has vetted this
///     pointer before using it!
///
///     `revocation_offset` is the offset into the one-way counter array that contains the revocations
///     corresponding to the pubkeys presented.
///
///     `function code` is a domain separator that ensures that signed sections can't be passed into
///     the wrong phase of the boot sequence. Passed as a list of u32-values that are allowed.
///
/// `spim`, when Some, informs validate_image to check an image contained in SPI flash.
///
/// `csprng`, when Some, allows the image validator to insert random delays to harden against glitch attacks
///
/// Returns either Ok(key_index, !key_index, tag, jump_target) or Err
///   - `key_index` is returned twice, once as the compliment of itself, to harden the return value and to
///     facilitate hardened logic based on the return values.
///   - `tag` is an informative field, mostly, but can also be used to help with security checks as it should
///     be correlated to the `key_index` value.
///   - `jump_target` is the location to jump to, XOR'd with `tag` as a u32::le_bytes()
///
/// The purpose the XOR of `jump_target` with `tag` is to prevent the compiler from simply statically
/// inferring a jump address, which becomes an ideal glitch target. The XOR itself doesn't provide
/// cryptographic masking of the target address, it simply requires the CPU to do *something* to derive the
/// jump target from a set of data that have not been corrupted by prior glitching.
pub fn validate_image(
    configuration: SecurityConfiguration,
    mut spim: Option<&mut Spim>,
    mut csprng: Option<&mut Csprng>,
) -> Result<(usize, usize, [u8; 4], u32), String> {
    // Unpack the arguments
    let img_offset: *const u32 = configuration.image_ptr;
    let pubkeys_offset: *const u32 = configuration.pubkey_ptr;
    let revocation_offset: usize = configuration.revocation_owc;
    let function_codes: &[u32] = configuration.function_codes;

    bollard!(die_no_std, 4);
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    // Copy the signature into a structure so we can unpack it.
    let mut sig = SignatureInFlash::default();
    if let Some(ref mut spim) = spim {
        spim.mem_read(img_offset as u32, sig.as_mut(), false);
    } else {
        // safety: `u8` can represent all values within the pointer.
        let sig_slice =
            unsafe { core::slice::from_raw_parts(img_offset as *const u8, size_of::<SignatureInFlash>()) };
        sig.as_mut().copy_from_slice(sig_slice);
    };

    bollard!(die_no_std, 4);
    let pubkey_ptr = pubkeys_offset as *const SignatureInFlash;
    let pk_src: &SignatureInFlash = unsafe { pubkey_ptr.as_ref().unwrap() };
    if pk_src.sealed_data.magic != MAGIC_NUMBER {
        return Err(String::from("Invalid magic number in verifying key record"));
    }

    let signed_len = sig.sealed_data.signed_len;

    bollard!(die_no_std, 4);
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    if sig.sealed_data.magic != MAGIC_NUMBER {
        return Err(String::from("Invalid magic number on incoming record to be verified"));
    }
    bollard!(die_no_std, 4);
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
    bollard!(die_no_std, 4);
    if !function_codes.contains(&sig.sealed_data.function_code) {
        crate::println!("Function code {} not expected", sig.sealed_data.function_code);
        return Err(String::from("Partition has invalid function code"));
    }

    bollard!(die_no_std, 4);

    // crate::println!("Signature: {:x?}", sig.signature);
    let one_way_counters = OneWayCounter::new();
    let mut passing_key: Option<usize> = None;
    let mut passing_key2: Option<usize> = None;
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    for (i, key) in pk_src.sealed_data.pubkeys.iter().enumerate() {
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        bollard!(die_no_std, 4);
        if key.tag == [0u8; 4] {
            continue;
        }

        // revocations are hardened by checking duplicate one-way counters. The glitch attack has to
        // succeed twice to use a revoked key.
        let (rev_a, rev_b) = one_way_counters
            .hardened_get2(revocation_offset + i, revocation_offset + i - REVOCATION_DUPE_DISTANCE)
            .expect("internal error");
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if rev_a != 0 {
            crate::println!("Key at index {} is revoked ({}), skipping", i, rev_a);
            continue;
        }
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if rev_b != 0 {
            crate::println!("Key at index {} is revoked ({}), skipping", i, rev_a);
            continue;
        }
        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(&key.pk).or(Err(String::from("invalid public key")))?;

        csprng.as_deref_mut().map(|rng| rng.random_delay());
        bollard!(die_no_std, 4);

        let ed25519_signature = ed25519_dalek::Signature::from(sig.signature);

        let mut h: Sha512 = Sha512::new();
        bollard!(die_no_std, 4);
        if let Some(ref mut spim) = spim {
            // need to read the data out page by page and hash it.
            // ASSUME: the SPIM driver has allocated a read buffer that is actually PAGE_SIZE. If the SPIM
            // driver has a smaller buffer, reads get less efficient.
            let end = img_offset as usize + UNSIGNED_LEN + signed_len as usize;
            for offset in ((img_offset as usize + UNSIGNED_LEN)..end).step_by(PAGE_SIZE) {
                let mut buf = [0u8; PAGE_SIZE];
                spim.mem_read(offset as u32, &mut buf, false);
                let valid_length = if offset + PAGE_SIZE < end { PAGE_SIZE } else { end - offset };
                h.update(&buf[..valid_length]);
            }
        } else {
            // easy peasy
            let image: &[u8] = unsafe {
                core::slice::from_raw_parts(
                    (img_offset as usize + UNSIGNED_LEN) as *const u8,
                    signed_len as usize,
                )
            };
            h.update(&image);
        }
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if sig.aad_len == 0 {
            // crate::println!("ed25519ph verifying with {:x?}", &key.pk);
            // debugging note: h.clone() does *not* work. You have to print the hash by modifying
            // the function inside the ed25519 crate.
            bollard!(die_no_std, 4);
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            // this match statement is an Achilles's heel. I don't want to fork the cryptographic
            // crates, so we have an unhardened comparison of the result. The work-around is
            // in paranoid mode, we command the system to verify things *twice*
            match verifying_key.verify_prehashed(h, None, &ed25519_signature) {
                Ok(_) => {
                    bollard!(die_no_std, 4);
                    crate::println!("ed25519ph verification passed");
                    passing_key = Some(i);
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    passing_key2 = Some(!i);
                    break;
                }
                _ => {
                    crate::println!("ed25519ph verification failed");
                }
            }
        } else {
            bollard!(die_no_std, 4);
            let sha512_hashed_image = h.finalize();
            // create a *new* hasher because a token can only sign a hash, not the full image.
            let mut h: Sha256 = Sha256::new();
            // hash dat hash!
            // crate::println!("verifying base hash {:x?}", &sha512_hashed_image.as_slice());
            h.update(&sha512_hashed_image.as_slice());
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            let hashed_hash = h.finalize();
            bollard!(die_no_std, 4);
            // crate::println!("hashed hash: {:x?}", hashed_hash.as_slice());

            let mut msg: Vec<u8> = Vec::new();
            msg.extend_from_slice(&sig.aad[..sig.aad_len as usize]);
            msg.extend_from_slice(hashed_hash.as_slice());
            // crate::println!("assembled msg({}): {:x?}", msg.len(), msg);

            bollard!(die_no_std, 4);
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            // this match statement is an Achilles's heel. I don't want to fork the cryptographic
            // crates, so we have an unhardened comparison of the result. The work-around is
            // in paranoid mode, we command the system to verify things *twice*
            match verifying_key.verify_strict(&msg, &ed25519_signature) {
                Ok(_) => {
                    bollard!(die_no_std, 4);
                    crate::println!("FIDO2 ed25519 verification passed");
                    passing_key = Some(i);
                    bollard!(die_no_std, 4);
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    passing_key2 = Some(!i);
                    break;
                }
                _ => {
                    crate::println!("FIDO2 verification failed");
                }
            }
        }
    }

    bollard!(die_no_std, 4);
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    if let Some(valid_key2) = passing_key2 {
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if let Some(valid_key) = passing_key {
            bollard!(die_no_std, 4);
            Ok((
                valid_key,
                valid_key2,
                pk_src.sealed_data.pubkeys[valid_key].tag,
                (img_offset as u32) ^ u32::from_le_bytes(pk_src.sealed_data.pubkeys[valid_key].tag),
            ))
        } else {
            Err(String::from("No valid pubkeys found or signature invalid"))
        }
    } else {
        Err(String::from("No valid pubkeys found or signature invalid"))
    }
}

#[cfg(feature = "std")]
pub fn erase_secrets(_csprng: &mut Option<&mut Csprng>) -> Result<(), String> {
    unimplemented!(
        "erase_secrets() is not available in the run-time environment; access permissions are insufficient."
    );
}

#[cfg(not(feature = "std"))]
pub fn erase_collateral(csprng: &mut Option<&mut Csprng>) -> Result<(), String> {
    let slot_mgr = SlotManager::new();
    let mut rram = crate::rram::Reram::new();

    let slot = &bao1x_api::offsets::COLLATERAL;
    for data_index in slot.try_into_data_iter().unwrap() {
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        // only clear ACL if it isn't already cleared
        if slot_mgr.get_acl(slot).unwrap().raw_u32() != 0 {
            // clear the ACL so we can operate on the data
            slot_mgr
                .set_acl(&mut rram, slot, &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0)))
                .expect("couldn't reset ACL");
        }
        let bytes = unsafe { slot_mgr.read_data_slot(data_index) };
        // only erase if the key hasn't already been erased, to avoid stressing the RRAM array
        // erase_secrets() may be called on every boot in some modes.
        bollard!(die_no_std, 4);
        if !bytes.iter().all(|&b| b == ERASE_VALUE) {
            let mut eraser = alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
            eraser.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, ERASE_VALUE);

            slot_mgr.write(&mut rram, slot, &eraser).ok();
        }
        let check = unsafe { slot_mgr.read_data_slot(data_index) };
        if !check.iter().all(|&b| b == ERASE_VALUE) {
            crate::println!("Failed to erase key at {}: {:x?}", data_index, check);
        }
        bollard!(die_no_std, 4);
    }
    Ok(())
}

#[cfg(not(feature = "std"))]
pub fn erase_secrets(csprng: &mut Option<&mut Csprng>) -> Result<(), String> {
    // ensure coreuser settings, as we could enter from a variety of loader stages
    let mut cu = crate::coreuser::Coreuser::new();
    cu.set();

    let slot_mgr = SlotManager::new();
    let mut rram = crate::rram::Reram::new();

    let mut buram = BackupManager::new();
    // erase the backup RAM region to 0 that is the erasure proof.
    bollard!(die_no_std, 4);
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    // safety: these words are excluded from the hash check because we need to pass from boot0 through
    // to the loader, but the "hard reset" check happens only in boot1
    unsafe {
        buram.store_slice_no_hash(&[0u8; 32], ERASURE_PROOF_RANGE_BYTES.start);
    }

    let mut zero_key_count = 0;
    // This is set to a higher level because we need to work around an earlier issue
    // with overly-broad ACL settings on alpha0 boards
    const ZERO_ERR_THRESH: usize = 64;
    bollard!(die_no_std, 4);
    for slot in crate::board::KEY_SLOTS.iter() {
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if slot.get_type() == SlotType::Data {
            let (_pa, rw_perms) = slot.get_access_spec();
            let mut erased_keys = 0;
            for data_index in slot.try_into_data_iter().unwrap() {
                bollard!(die_no_std, 4);
                csprng.as_deref_mut().map(|rng| rng.random_delay());
                match rw_perms {
                    RwPerms::ReadWrite | RwPerms::WriteOnly => {
                        // only clear ACL if it isn't already cleared
                        if slot_mgr.get_acl(slot).unwrap().raw_u32() != 0 {
                            // clear the ACL so we can operate on the data
                            slot_mgr
                                .set_acl(
                                    &mut rram,
                                    slot,
                                    &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0)),
                                )
                                .expect("couldn't reset ACL");
                        }
                        let bytes = unsafe { slot_mgr.read_data_slot(data_index) };
                        if bytes.iter().all(|&b| b == 0) {
                            zero_key_count += 1;
                        }
                        // only erase if the key hasn't already been erased, to avoid stressing the RRAM array
                        // erase_secrets() may be called on every boot in some modes.
                        bollard!(die_no_std, 4);
                        if !bytes.iter().all(|&b| b == ERASE_VALUE) {
                            let mut eraser =
                                alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
                            eraser.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, ERASE_VALUE);

                            slot_mgr.write(&mut rram, slot, &eraser).ok();
                        }
                        let check = unsafe { slot_mgr.read_data_slot(data_index) };
                        if !check.iter().all(|&b| b == ERASE_VALUE) {
                            crate::println!("Failed to erase key at {}: {:x?}", data_index, check);
                            /* // commented out - can lead to boot loops
                            // reboot on failure to erase
                            let mut rcurst =
                                utralib::CSR::new(utralib::utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                            rcurst.wo(utralib::utra::sysctrl::SFR_RCURST0, 0x55AA);
                            */
                        } else {
                            erased_keys += 1;
                        }
                    }
                    _ => {}
                }
                bollard!(die_no_std, 4);
            }
            crate::println!(
                "Key range at {}: {}/{} keys confirmed erased",
                slot.get_base(),
                erased_keys,
                slot.len()
            );
            bollard!(die_no_std, 4);
        }
        bollard!(die_no_std, 4);
    }
    bollard!(die_no_std, 4);

    // store the proof that the key array was erased - could lead to disclosure of one key,
    // but we also can't simply trust that the oneway counter below is accurate
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    // safety: these words are excluded from the hash check because we need to pass from boot0 through
    // to the loader, but the "hard reset" check happens only in boot1
    unsafe {
        buram.store_slice_no_hash(
            slot_mgr.read(&crate::board::ERASE_PROOF).unwrap(),
            ERASURE_PROOF_RANGE_BYTES.start,
        );
    }

    let owc = OneWayCounter::new();
    // once all secrets are erased, advance the DEVELOPER_MODE state
    // safety: the offset is correct because we're pulling it from our pre-defined constants and
    // those are manually checked.
    bollard!(die_no_std, 4);
    if owc.get(DEVELOPER_MODE).unwrap() < 15 {
        // limit incrementing to avoid memory wear-out, as erase_secrets() can be called every time on boot.
        unsafe { owc.inc(DEVELOPER_MODE).unwrap() };
    }
    if zero_key_count > ZERO_ERR_THRESH {
        Err(String::from("Saw too many zero-keys. Insufficient privilege to erase keys!"))
    } else {
        Ok(())
    }
}

/// This implements hardened erase policy implementation: basically, if developer mode
/// is detected, erase the secret keys.
#[inline(always)]
pub fn hardened_erase_policy(
    paranoid1: u32,
    paranoid2: u32,
    key: usize,
    key_inv: usize,
    tag: [u8; 4],
    csprng: &mut Csprng,
) -> Result<(), String> {
    if key == DEVELOPER_KEY_SLOT {
        // this is a common case - if we're not under attack, and we're in developer mode,
        // just short circuit the rest of the checks and erase the keys.
        return erase_secrets(&mut Some(csprng));
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    // if the tag isn't one of the first 3 "blessed" tags, assume developer mode. This is
    // a supplemental check, so we don't harden it.
    if !KEYSLOT_INITIAL_TAGS[..3].contains(&&tag) {
        erase_secrets(&mut Some(csprng))?;
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    // second check on the inverse-key type - this requires a double-glitch to bypass the key number check
    if !key_inv == DEVELOPER_KEY_SLOT {
        erase_secrets(&mut Some(csprng))?;
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    // these won't match if we're under attack - erase the keys if attack is detected in this case
    if paranoid1 != paranoid2 {
        erase_secrets(&mut Some(csprng))?;
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();

    if paranoid1 != 0 || paranoid2 != 0 {
        // the whole code up there is repeated again - check twice, written out in linear form, instead
        // of a loop, so glitches have a chance to land basically somewhere in this morass.
        if key == DEVELOPER_KEY_SLOT {
            // this is a common case - if we're not under attack, and we're in developer mode,
            // just short circuit the rest of the checks and erase the keys.
            erase_secrets(&mut Some(csprng))?;
        }
        bollard!(die_no_std, 4);
        csprng.random_delay();
        // if the tag isn't one of the first 3 "blessed" tags, assume developer mode. This is
        // a supplemental check, so we don't harden it.
        if !KEYSLOT_INITIAL_TAGS[..3].contains(&&tag) {
            erase_secrets(&mut Some(csprng))?;
        }
        bollard!(die_no_std, 4);
        csprng.random_delay();
        // second check on the key type - this requires a double-glitch to bypass the key number check
        if !key_inv == DEVELOPER_KEY_SLOT {
            erase_secrets(&mut Some(csprng))?;
        }
        bollard!(die_no_std, 4);
        csprng.random_delay();
        // these won't match if we're under attack - erase the keys if attack is detected in this case
        if paranoid1 != paranoid2 {
            erase_secrets(&mut Some(csprng))?;
        }
        bollard!(die_no_std, 4);
        csprng.random_delay();
        // these won't match if we're under attack - erase the keys if attack is detected in this case
        if paranoid1 != paranoid2 {
            erase_secrets(&mut Some(csprng))?;
        }
    }
    Ok(())
}

pub fn jump_to(target: usize) -> ! {
    // loader expects a0 to have the address of the kernel image pre-loaded
    let kernel_loc = bao1x_api::offsets::KERNEL_START;
    unsafe {
        core::arch::asm!(
            "mv t0, {target}",
            "mv a0, {kernel_loc}",
            "mv a1, x0",
            "jr t0",
            target = in(reg) target,
            kernel_loc = in(reg) kernel_loc,
            options(noreturn)
        );
    }
}

pub fn die_no_std() -> ! {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm! (
            // TODO: any SCE other security-sensitive registers to be zeorized?

            //  - bureg zeroize - this is priority because it has the ephemeral key
            "li          x1, 0x40065000",
            "li          x2, 0x40065020",
        "30:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 30b",

            //  - AORAM_MEM zeroize - also priority because it can have ephemeral secrets
            "li          x1, 0x50300000",
            "li          x2, 0x50304000",
        "16:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 16b",

            // key regions
            "li          x1, 0x40020000",
            "li          x2, 0x40022700",
        "10:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 10b",
            // SCE_MEM
            "li          x1, 0x40028000",
            "li          x2, 0x40030000",
        "15:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 15b",

            //  - IFRAM0/1 zeroize
            "li          x1, 0x50000000",
            "li          x2, 0x50040000",
        "11:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 11b",
            //  - UDC_MEM zeroize
            "li          x1, 0x50200000",
            "li          x2, 0x50210000",
        "12:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 12b",
            //  - BIO_MEM zeroize
            "li          x1, 0x50125000",
            "li          x2, 0x50129000",
        "13:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 13b",

            // zeroize main RAM
            "li          x1, 0x61000000",
            "li          x2, 0x61200000",
        "14:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 14b",

            // zeroize CPU registers
            "mv          x1, x0",
            "mv          x2, x0",
            "mv          x3, x0",
            "mv          x4, x0",
            "mv          x5, x0",
            "mv          x6, x0",
            "mv          x7, x0",
            "mv          x8, x0",
            "mv          x9, x0",
            "mv          x10, x0",
            "mv          x11, x0",
            "mv          x12, x0",
            "mv          x13, x0",
            "mv          x14, x0",
            "mv          x15, x0",
            "mv          x16, x0",
            "mv          x17, x0",
            "mv          x18, x0",
            "mv          x19, x0",
            "mv          x20, x0",
            "mv          x21, x0",
            "mv          x22, x0",
            "mv          x23, x0",
            "mv          x24, x0",
            "mv          x25, x0",
            "mv          x26, x0",
            "mv          x27, x0",
            "mv          x28, x0",
            "mv          x29, x0",
            "mv          x30, x0",
            "mv          x31, x0",

            // emit a loop out of DUART to indicate successful death
            "li          t0, 0x40042000",
            // print 'X' (0x58)
            "li          t1, 0x58",
            "li          t2, 256",
        "20:",
            "sw          t1, 0x0(t0)",
        "21:",
            "lw          t3, 0x8(t0)", // check SR
            "bne         x0, t3, 21b", // wait for 0
            "addi        t2, t2, -1",
            "bne         x0, t2, 20b",

        "22:",
            // multiple jump-backs in case PC is glitched beyond the branch
            // ... a cache line even if this gets turned into C-form ...
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            "j           22b",
            // and one to grow on
            "j           22b",

            options(noreturn)
        );
    }
}
