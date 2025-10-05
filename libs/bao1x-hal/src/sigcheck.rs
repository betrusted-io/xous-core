extern crate alloc;
use alloc::string::String;

use bao1x_api::signatures::*;
use digest::Digest;
use sha2_bao1x::Sha512;
use xous::arch::PAGE_SIZE;

use crate::acram::OneWayCounter;
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
/// If `false` the function returns the key index of the first passing public key, or an error
/// if none were found.
pub fn validate_image(
    img_offset: *const u32,
    pubkeys_offset: *const u32,
    revocation_offset: usize,
    function_codes: &[u32],
    auto_jump: bool,
    mut spim: Option<&mut Spim>,
) -> Result<usize, String> {
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

    let signed_len = sig.sealed_data.signed_len;

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

    let one_way_counters = OneWayCounter::new();
    let mut secure = false;
    let mut passing_key: Option<usize> = None;
    for (i, key) in pk_src.sealed_data.pubkeys.iter().enumerate() {
        if key.iter().all(|&x| x == 0) {
            continue;
        }
        let revocation_value = one_way_counters.get(revocation_offset + i).expect("internal error");
        if revocation_value != 0 {
            crate::println!("Key at index {} is revoked ({}), skipping", i, revocation_value);
            continue;
        }
        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(key).or(Err(String::from("invalid public key")))?;

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
            // crate::println!("Verifying image {:x?} with {:x?}", &image[..16], &key);
            h.update(&image);
        }
        // debugging note: h.clone() does *not* work. You have to print the hash by modifying
        // the function inside the ed25519 crate.
        match verifying_key.verify_prehashed(h, None, &ed25519_signature) {
            Ok(_) => {
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
            _ => {}
        }
    }

    if !secure || developer {
        erase_secrets();
    }
    if let Some(valid_key) = passing_key {
        if auto_jump {
            jump_to(img_offset as usize);
        }
        Ok(valid_key)
    } else {
        Err(String::from("No valid pubkeys found or signature invalid"))
    }
}

fn erase_secrets() {
    // actually, this should check first if the data is 0, then 0 it. Can't afford
    // to wear-out the memory every boot by repeatedly overwriting it...
    crate::println!("TODO: erase secret data stores on boot to devmode!");
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
