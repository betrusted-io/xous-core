extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use bao1x_api::signatures::*;
#[cfg(not(feature = "std"))]
use bao1x_api::{DEVELOPER_MODE, KeySlotAccess, RwPerms, SLOT_ELEMENT_LEN_BYTES, SlotType};
use digest::Digest;
use sha2_bao1x::{Sha256, Sha512};
use xous::arch::PAGE_SIZE;

use crate::acram::OneWayCounter;
#[cfg(not(feature = "std"))]
use crate::acram::{AccessSettings, SlotManager};
use crate::udma::Spim;

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
/// `img_offset` is a pointer to untrusted image data. It's assumed that the 0-offset of the pointer is
/// a `SignatureInFlash` structure.
///
/// `pubkeys_offset` is a pointer to trusted public key data. Because 'pubkeys_offset` is assumed to be
/// trusted minimal validation is done on this pointer. It's important that the caller has vetted this
/// pointer before using it!
///
/// `revocation_offset` is the offset into the one-way counter array that contains the revocations
/// corresponding to the pubkeys presented.
///
/// `function code` is a domain separator that ensures that signed sections can't be passed into
/// the wrong phase of the boot sequence. Passed as a list of u32-values that are allowed.
///
/// `auto_jump` is a flag which, when `true`, causes the code to diverge into the signed block.
/// If `false` the function returns the `(key_index, tag)` of the first passing public key, or an error
/// if none were found.
///
/// `spim`, when Some, informs validate_image to check an image contained in SPI flash.
pub fn validate_image(
    img_offset: *const u32,
    pubkeys_offset: *const u32,
    revocation_offset: usize,
    function_codes: &[u32],
    auto_jump: bool,
    mut spim: Option<&mut Spim>,
) -> Result<(usize, [u8; 4]), String> {
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

    let pubkey_ptr = pubkeys_offset as *const SignatureInFlash;
    let pk_src: &SignatureInFlash = unsafe { pubkey_ptr.as_ref().unwrap() };
    if pk_src.sealed_data.magic != MAGIC_NUMBER {
        return Err(String::from("Invalid magic number in verifying key record"));
    }

    let signed_len = sig.sealed_data.signed_len;

    if sig.sealed_data.magic != MAGIC_NUMBER {
        return Err(String::from("Invalid magic number on incoming record to be verified"));
    }
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
    if !function_codes.contains(&sig.sealed_data.function_code) {
        crate::println!("Function code {} not expected", sig.sealed_data.function_code);
        return Err(String::from("Partition has invalid function code"));
    }

    let developer = sig.sealed_data.function_code == FunctionCode::Developer as u32;

    // crate::println!("Signature: {:x?}", sig.signature);
    let one_way_counters = OneWayCounter::new();
    let mut secure = false;
    let mut passing_key: Option<usize> = None;
    for (i, key) in pk_src.sealed_data.pubkeys.iter().enumerate() {
        if key.tag == [0u8; 4] {
            continue;
        }
        let revocation_value = one_way_counters.get(revocation_offset + i).expect("internal error");
        if revocation_value != 0 {
            crate::println!("Key at index {} is revoked ({}), skipping", i, revocation_value);
            continue;
        }
        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(&key.pk).or(Err(String::from("invalid public key")))?;

        let ed25519_signature = ed25519_dalek::Signature::from(sig.signature);

        let mut h: Sha512 = Sha512::new();
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
        if sig.aad_len == 0 {
            // crate::println!("ed25519ph verifying with {:x?}", &key.pk);
            // debugging note: h.clone() does *not* work. You have to print the hash by modifying
            // the function inside the ed25519 crate.
            match verifying_key.verify_prehashed(h, None, &ed25519_signature) {
                Ok(_) => {
                    crate::println!("ed25519ph verification passed");
                    passing_key = Some(i);
                    if i != sig.sealed_data.pubkeys.len() - 1 {
                        secure = true;
                        break;
                    } else if i == sig.sealed_data.pubkeys.len() - 1 {
                        // this is the developer key slot
                        secure = false;
                        break;
                    }
                }
                _ => {
                    crate::println!("ed25519ph verification failed");
                }
            }
        } else {
            let sha512_hashed_image = h.finalize();
            // create a *new* hasher because a token can only sign a hash, not the full image.
            let mut h: Sha256 = Sha256::new();
            // hash dat hash!
            // crate::println!("verifying base hash {:x?}", &sha512_hashed_image.as_slice());
            h.update(&sha512_hashed_image.as_slice());
            let hashed_hash = h.finalize();
            // crate::println!("hashed hash: {:x?}", hashed_hash.as_slice());

            let mut msg: Vec<u8> = Vec::new();
            msg.extend_from_slice(&sig.aad[..sig.aad_len as usize]);
            msg.extend_from_slice(hashed_hash.as_slice());
            // crate::println!("assembled msg({}): {:x?}", msg.len(), msg);

            match verifying_key.verify_strict(&msg, &ed25519_signature) {
                Ok(_) => {
                    crate::println!("FIDO2 ed25519 verification passed");
                    passing_key = Some(i);
                    if i != sig.sealed_data.pubkeys.len() - 1 {
                        secure = true;
                        break;
                    } else if i == sig.sealed_data.pubkeys.len() - 1 {
                        // this is the developer key slot
                        secure = false;
                        break;
                    }
                }
                _ => {
                    crate::println!("FIDO2 verification failed");
                }
            }
        }
    }

    if let Some(valid_key) = passing_key {
        if auto_jump {
            if !secure || developer {
                erase_secrets();
            }
            jump_to(img_offset as usize);
        }
        Ok((valid_key, pk_src.sealed_data.pubkeys[valid_key].tag))
    } else {
        Err(String::from("No valid pubkeys found or signature invalid"))
    }
}

#[cfg(feature = "std")]
pub fn erase_secrets() {
    unimplemented!(
        "erase_secrets() is not available in the run-time environment; access permissions are insufficient."
    );
}

#[cfg(not(feature = "std"))]
pub fn erase_secrets() {
    // An erase value of 0 is conflateable with access permissions being incorrect. Choose a non-0 value
    // for the erase value, but also, don't pick a 0-1-0-1 dense pattern because that can assist with
    // calibrating microscopy techniques.
    const ERASE_VALUE: u8 = 0x03;

    // ensure coreuser settings, as we could enter from a variety of loader stages
    let mut cu = crate::coreuser::Coreuser::new();
    cu.set();

    let slot_mgr = SlotManager::new();
    let mut rram = crate::rram::Reram::new();

    let mut zero_key_count = 0;
    // statistically speaking, I suppose, maybe we could have "a" set of keys that are 0 out of a randomly
    // generated set. But if we see more than the threshold below of zero keys, conclude that we don't
    // have access permissions, and panic instead of allowing a boot.
    const ZERO_ERR_THRESH: usize = 2;
    for slot in crate::board::KEY_SLOTS.iter() {
        if slot.get_type() == SlotType::Key {
            let (_pa, rw_perms) = slot.get_access_spec();
            for data_index in slot.try_into_data_iter().unwrap() {
                match rw_perms {
                    RwPerms::ReadWrite | RwPerms::WriteOnly => {
                        // only clear ACL if it isn't already cleared
                        if slot_mgr.get_acl(slot).unwrap().raw_u32() != 0 {
                            // clear the ACL so we can operate on the data
                            slot_mgr
                                .set_acl(
                                    &mut rram,
                                    slot,
                                    &AccessSettings::Key(KeySlotAccess::new_with_raw_value(0)),
                                )
                                .expect("couldn't reset ACL");
                        }
                        let bytes = unsafe { slot_mgr.read_key_slot(data_index) };
                        if bytes.iter().all(|&b| b == 0) {
                            zero_key_count += 1;
                        }
                        // only erase if the key hasn't already been erased, to avoid stressing the RRAM array
                        // erase_secrets() may be called on every boot in some modes.
                        if !bytes.iter().all(|&b| b == ERASE_VALUE) {
                            let mut eraser =
                                alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
                            eraser.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, ERASE_VALUE);

                            slot_mgr.write(&mut rram, slot, &eraser).expect("couldn't erase key");
                        }
                        let check = unsafe { slot_mgr.read_key_slot(data_index) };
                        if !check.iter().all(|&b| b == ERASE_VALUE) {
                            crate::println!("Failed to erase key at {}: {:x?}", data_index, check);
                            panic!("Key erasure did not succeed, refusing to boot!");
                        } else {
                            crate::println!("Key range at {} confirmed erased", slot.get_base());
                        }
                    }
                    _ => {}
                }
                if zero_key_count > ZERO_ERR_THRESH {
                    panic!(
                        "Saw too many zero-keys. Insufficient privilege to erase keys, panicing instead of allowing a boot!"
                    );
                }
            }
        }
    }
    let owc = OneWayCounter::new();
    // once all secrets are erased, advance the DEVELOPER_MODE state
    // safety: the offset is correct because we're pulling it from our pre-defined constants and
    // those are manually checked.
    unsafe { owc.inc(DEVELOPER_MODE).unwrap() };
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

            //  - AO_MEM zeroize
            "li          x1, 0x40060000",
            "li          x2, 0x40070000",
        "16:",
            "sw          x0, 0(x1)",
            "addi        x1, x1, 4",
            "bne         x1, x2, 16b",

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
