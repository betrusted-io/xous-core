extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use bao1x_api::PQ_REVOCATION_DUPE_DISTANCE;
use bao1x_api::REQUIRE_PQ;
use bao1x_api::REQUIRE_PQ_DUPE;
use bao1x_api::REVOCATION_DUPE_DISTANCE;
use bao1x_api::bollard;
use bao1x_api::classic_to_pq_revocation;
use bao1x_api::pubkeys::DEVELOPER_KEY_SLOT;
use bao1x_api::pubkeys::SecurityConfiguration;
use bao1x_api::signatures::*;
#[cfg(not(feature = "std"))]
use bao1x_api::{DEVELOPER_MODE, DataSlotAccess, RwPerms, SLOT_ELEMENT_LEN_BYTES};
use digest::Digest;
use sha2_bao1x::{Sha256, Sha512};
use xous::arch::PAGE_SIZE;

use crate::ERASE_VALUE;
use crate::acram::OneWayCounter;
#[cfg(not(feature = "std"))]
use crate::acram::{AccessSettings, SlotManager};
#[cfg(not(feature = "std"))]
use crate::buram::{BackupManager, ERASURE_PROOF_RANGE_BYTES};
use crate::hardening::Csprng;
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
/// Returns either Ok(key_index, !key_index, tag, jump_target, pq_advisory) or Err
///   - `key_index` is returned twice, once as the compliment of itself, to harden the return value and to
///     facilitate hardened logic based on the return values.
///   - `tag` is an informative field, mostly, but can also be used to help with security checks as it should
///     be correlated to the `key_index` value.
///   - `jump_target` is the location to jump to, XOR'd with `tag` as a u32::le_bytes()
///   - `pq_advisory` is an unhardened advisory value if a valid pq signature was found or not (`true` means
///     it was found). The actual go/no-go decision is made inside the hardened loop, this value is provided
///     for diagnostic & debug purposes only.
///
/// The purpose the XOR of `jump_target` with `tag` is to prevent the compiler from simply statically
/// inferring a jump address, which becomes an ideal glitch target. The XOR itself doesn't provide
/// cryptographic masking of the target address, it simply requires the CPU to do *something* to derive the
/// jump target from a set of data that have not been corrupted by prior glitching.
pub fn validate_image(
    configuration: SecurityConfiguration,
    mut spim: Option<&mut Spim>,
    mut csprng: Option<&mut Csprng>,
) -> Result<(usize, usize, [u8; 4], u32, Option<[u8; 4]>), String> {
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
    if !sig.is_compatible() {
        crate::println!(
            "Version {:x} sig is too new for {:x}",
            sig.sealed_data.corrected_version,
            BAOCHIP_SIG_VERSION
        );
        return Err(String::from("incompatible sigblock version"));
    }

    // checking the function code prevents exploiting code meant for other partitions signed
    // with a valid signature as code for the next stage boot.
    bollard!(die_no_std, 4);
    if !function_codes.contains(&sig.sealed_data.function_code) {
        crate::println!("Function code {} not expected", sig.sealed_data.function_code);
        return Err(String::from("Partition has invalid function code"));
    }

    let one_way_counters = OneWayCounter::new();
    bollard!(die_no_std, 4);

    // crate::println!("Signature: {:x?}", sig.signature);
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
            crate::println!("Key at index {} is revoked ({}), skipping", i, rev_b);
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
            assert!(end <= bao1x_api::offsets::baosec::SPI_FLASH_LEN);
            for offset in ((img_offset as usize + UNSIGNED_LEN)..end).step_by(PAGE_SIZE) {
                let mut buf = [0u8; PAGE_SIZE];
                spim.mem_read(offset as u32, &mut buf, false);
                let valid_length = if offset + PAGE_SIZE < end { PAGE_SIZE } else { end - offset };
                h.update(&buf[..valid_length]);
            }
        } else {
            // sanity check the purported length of the image. It can't be any bigger than the available
            // storage in RRAM.
            assert!(
                (signed_len as usize)
                    <= bao1x_api::RRAM_STORAGE_LEN
                        - ((img_offset as usize - utralib::HW_RERAM_MEM) + UNSIGNED_LEN)
            );
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
            assert!((sig.aad_len as usize) <= sig.aad.len());
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
            // Check anti-rollback only after we have confirmed a signature to mitigate the
            // possibility of wear-out attacks by unsigned images that set the anti-rollback
            // field to a high number
            let claimed_function: FunctionCode =
                sig.sealed_data.function_code.try_into().unwrap_or(FunctionCode::Invalid);
            let arb_offset = claimed_function.to_anti_rollback_counter();
            match arb_offset {
                Some(arb) => {
                    let arb_value = one_way_counters.get(arb).expect("Can't read anti-rollback value");
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    bollard!(die_no_std, 4);
                    if arb_value > sig.sealed_data.anti_rollback {
                        let mut err_msg = String::from("Anti-rollback code too old, refusing image: ");
                        use alloc::string::ToString;
                        err_msg.push_str(&arb_value.to_string());
                        return Err(err_msg);
                    }
                    bollard!(die_no_std, 4);
                    if arb_value < sig.sealed_data.anti_rollback {
                        csprng.as_deref_mut().map(|rng| rng.random_delay());
                        if sig.sealed_data.anti_rollback >= crate::acram::ONEWAY_MAX_VALUE {
                            return Err(String::from("Proposed anti-rollback value out of range"));
                        }
                        // enforce a maximum "reasonable" increment - just as belt-and-suspenders
                        assert!(sig.sealed_data.anti_rollback - arb_value < crate::acram::ONEWAY_MAX_DELTA);
                        // increment anti-rollback counter to match the current value of the signed image
                        bollard!(die_no_std, 4);
                        while one_way_counters.get(arb).unwrap() < sig.sealed_data.anti_rollback {
                            bollard!(die_no_std, 4);
                            // safety: anti-rollback counter argument is from a set of constants in bao1x_api
                            // that are pre-validated.
                            unsafe { one_way_counters.inc(arb).ok() };
                        }
                    }
                }
                _ => return Err(String::from("Invalid anti-rollback code, aborting")),
            }

            // default to a hard-wired mask if for some reason we're called with no csprng. The primary
            // purpose of the mask is to force the Rust compiler to not optimize out the equality
            // check of the PQ signature result, by causing the mask to be XOR'd into the values deep inside
            // the API call.
            let mask = csprng.as_deref_mut().map(|rng| rng.get_u32()).unwrap_or(0x5A5A_A5A5);

            let (pq_required_a, pq_required_b) =
                one_way_counters.hardened_get2(REQUIRE_PQ, REQUIRE_PQ_DUPE).expect("internal error");
            let req_a = core::hint::black_box(pq_required_a);
            let req_b = core::hint::black_box(pq_required_b);

            // ---- normalise the PQ result into a redundant verdict pair ----
            // far-apart complementary constants: one bit-flip can't turn NOMATCH into MATCH,
            // and the two words must always agree.
            const PQ_MATCH: u32 = 0x3C5A_A53C;
            const PQ_NOMATCH: u32 = 0xC3A5_3C0F;
            let mut verdict = PQ_NOMATCH;
            let mut verdict_dup = PQ_NOMATCH;

            // insert PQ check after ed25519 checks have passed
            bollard!(die_no_std, 4);
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            let mut pq_tag: [u8; 4] =
                *bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]; // this will trigger an erase if the copy-update is glitched over
            if let Some((out, pq_tag_inner)) = pq_checks(mask, &configuration, spim, &mut csprng) {
                let (mr, er) = (out.masked_root(), out.expected_root());
                if mr.len() == 0 || er.len() == 0 || (mr.len() != er.len()) {
                    // this is a hard error
                    die_no_std();
                }
                csprng.as_deref_mut().map(|rng| rng.random_delay());
                pq_tag.copy_from_slice(&pq_tag_inner);
                bollard!(die_no_std, 4);
                let mut matched: usize = 0;
                assert!(mr.len() < 32, "corrupt mr.len()"); // catch shift overflow if mr.len() is corrupted
                for i in 0..mr.len() {
                    bollard!(die_no_std, 4);
                    let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8); // 0x00 or 0xFF
                    let cand = core::hint::black_box(mr[i]) ^ unmask;
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    if cand != er[i] {
                        break;
                    } // independent per-byte branch
                    matched += 1;
                }
                csprng.as_deref_mut().map(|rng| rng.random_delay());

                if matched == mr.len() {
                    bollard!(die_no_std, 4);
                    // verdict is set here, spatially distant from verdict_dup, so that a single
                    // glitch doesn't give us an easy win to set both
                    verdict = PQ_MATCH;

                    // independent recount, separate accumulator, sets the *dup* tracker
                    let mut recount = 0usize;
                    // change up the random delay signature
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    for i in 0..er.len() {
                        let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8);
                        bollard!(die_no_std, 4);
                        if (core::hint::black_box(mr[i]) ^ unmask) == er[i] {
                            recount += 1;
                        }
                    }
                    // the recount must agree before the duplicate value switches
                    csprng.as_deref_mut().map(|rng| rng.random_delay());
                    if recount == er.len() {
                        bollard!(die_no_std, 4);
                        verdict_dup = PQ_MATCH;
                    }
                }
                // this is a sanity check in case we glitched past the pq_checks() call and into the
                // decision analysis code path above. It's likely that mr/er registers aren't set up,
                // but if they "happen" to be 0, it would give a good chance of falsely setting a PQ_MATCH
                csprng.as_deref_mut().map(|rng| rng.random_delay());
                if mr.len() == 0 || er.len() == 0 || (mr.len() != er.len()) {
                    die_no_std();
                }
            }

            // ---- single flattened decision ----
            bollard!(die_no_std, 4);
            if verdict != verdict_dup {
                die_no_std();
            } // trackers must agree
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            if verdict ^ verdict_dup != 0 {
                die_no_std();
            } // ...same test, different form

            let pq_matched = verdict == PQ_MATCH && verdict_dup == PQ_MATCH;
            let pq_unmatched = verdict == PQ_NOMATCH && verdict_dup == PQ_NOMATCH;
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            if pq_matched == pq_unmatched {
                die_no_std();
            } // exactly one must hold

            // two independent votes, one per counter — neither nests the other, both
            // always evaluated (bitwise ops, no short-circuit).
            bollard!(die_no_std, 4);
            let ok_a = pq_matched || (req_a == 0);
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            bollard!(die_no_std, 4);
            let ok_b = !pq_unmatched || (req_b == 0);
            csprng.as_deref_mut().map(|rng| rng.random_delay());

            // default is fail-closed; booting needs BOTH votes to pass
            if !(ok_a & ok_b) {
                bollard!(die_no_std, 4);
                return Err(String::from("PQ required but no valid PQ signature found"));
            }

            // at this point, in 'honest' code, we either don't require pq, or we have
            // a valid pq sig, or both. A glitcher would want to glitch past that check above.
            // So, we re-derive the same condition in negative form before the single success sink:
            // if the !(ok_a & ok_b) is glitched-past, this statement will cause a die_no_std();
            bollard!(die_no_std, 4);
            let required = (req_a != 0) || (req_b != 0);
            csprng.as_deref_mut().map(|rng| rng.random_delay());
            if !pq_matched & required {
                die_no_std();
            }

            // continue on to return classical signature
            bollard!(die_no_std, 4);
            assert!(valid_key != valid_key2);
            Ok((
                valid_key,
                valid_key2,
                pk_src.sealed_data.pubkeys[valid_key].tag,
                (img_offset as u32) ^ u32::from_le_bytes(pk_src.sealed_data.pubkeys[valid_key].tag),
                if verdict == PQ_MATCH { Some(pq_tag) } else { None },
            ))
        } else {
            Err(String::from("No valid pubkeys found or signature invalid"))
        }
    } else {
        Err(String::from("No valid pubkeys found or signature invalid"))
    }
}

// PQ checks take +45ms for the system while in conservative speed mode (175MHz - boot0 speed)
// and +22.8ms with the CPU running at 350MHz
#[inline(never)]
pub fn pq_checks(
    mask: u32,
    configuration: &SecurityConfiguration,
    mut spim: Option<&mut Spim>,
    csprng: &mut Option<&mut Csprng>,
) -> Option<(slh_dsa::HardenedVerifyOutput<slh_dsa::Sha2_128_24>, [u8; 4])> {
    use core::convert::TryFrom;

    use slh_dsa::SignatureLen;
    use slh_dsa::*;
    use typenum::Unsigned;

    // code for performance profiling with an oscope. Remove once things are stable.
    // use bao1x_api::*;
    // use crate::iox::Iox;
    // let iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
    // iox.set_gpio_dir(IoxPort::PC, 6, IoxDir::Output);
    // iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::Low);

    csprng.as_deref_mut().map(|rng| rng.random_delay());
    // Unpack the arguments
    let img_offset: *const u32 = configuration.image_ptr;
    let pubkeys_offset: *const u32 = configuration.pubkey_ptr;
    let revocation_offset: usize =
        classic_to_pq_revocation(configuration.revocation_owc).expect("bad revocation offset");

    // ASSUME: all version checks, function code checks, etc. are correct and finished.
    // we don't reproduce them here because we're running out of code space!

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
        die_no_std();
    }
    let signed_len = sig.sealed_data.signed_len;

    let owc = OneWayCounter::new();
    bollard!(die_no_std, 4);

    // hash times: 8.4ms, 19.6ms @ sha512; 7.2ms, 16ms @ sha256
    // iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::High);
    let mut h: Sha256 = Sha256::new();
    bollard!(die_no_std, 4);
    let pq_sig = if let Some(ref mut spim) = spim {
        // need to read the data out page by page and hash it.
        // ASSUME: the SPIM driver has allocated a read buffer that is actually PAGE_SIZE. If the SPIM
        // driver has a smaller buffer, reads get less efficient.
        let end = img_offset as usize + UNSIGNED_LEN + signed_len as usize;
        assert!(
            end + <Sha2_128_24 as SignatureLen>::SigLen::USIZE <= bao1x_api::offsets::baosec::SPI_FLASH_LEN
        );
        for offset in ((img_offset as usize + UNSIGNED_LEN)..end).step_by(PAGE_SIZE) {
            let mut buf = [0u8; PAGE_SIZE];
            spim.mem_read(offset as u32, &mut buf, false);
            let valid_length = if offset + PAGE_SIZE < end { PAGE_SIZE } else { end - offset };
            h.update(&buf[..valid_length]);
        }
        // extract sig_data which should be appended directly to the end of the image in FLASH.
        let mut sig_data = [0u8; <Sha2_128_24 as SignatureLen>::SigLen::USIZE];
        spim.mem_read(end as u32, &mut sig_data, false);
        &SignaturePqInFlash { signature: sig_data }
    } else {
        // sanity check the purported length of the image. It can't be any bigger than the available
        // storage in RRAM.
        assert!(
            (signed_len as usize)
                <= bao1x_api::RRAM_STORAGE_LEN
                    - ((img_offset as usize - utralib::HW_RERAM_MEM) + UNSIGNED_LEN)
        );
        let image: &[u8] = unsafe {
            core::slice::from_raw_parts(
                (img_offset as usize + UNSIGNED_LEN) as *const u8,
                signed_len as usize,
            )
        };
        h.update(&image);

        // safety:
        //  - img_offset points to a "real" image on disk
        //  - we are retrieving an XIP image
        unsafe { sig.pq_signature(img_offset as usize) }
    };
    let digest_binding = h.finalize();
    let digest = digest_binding.as_slice();
    csprng.as_deref_mut().map(|rng| rng.random_delay());

    let mut out = None;
    for (i, key) in pk_src.sealed_data.pubkeys_pq.iter().enumerate() {
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        bollard!(die_no_std, 4);
        // don't try the verification if there's no public key in the slot
        if key.tag == [0u8; 4] {
            continue;
        }
        // revocations are hardened by checking duplicate one-way counters. The glitch attack has to
        // succeed twice to use a revoked key.
        let (rev_a, rev_b) = owc
            .hardened_get2(revocation_offset + i, revocation_offset + i - PQ_REVOCATION_DUPE_DISTANCE)
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
            crate::println!("Key at index {} is revoked ({}), skipping", i, rev_b);
            continue;
        }
        let Ok(vk) = VerifyingKey::<Sha2_128_24>::try_from(key.pk.as_slice()) else { continue };
        let Ok(pq_sig) = Signature::<Sha2_128_24>::try_from(&pq_sig.signature[..]) else { continue };
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        bollard!(die_no_std, 4);
        out = Some((vk.slh_verify_hardened(&[digest], &pq_sig, mask), key.tag.clone()));
        bollard!(die_no_std, 4);
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        if let Some((out_inner, _pq_tag)) = &out {
            // this is an advisory-only check if the signature passed. If the signatures match,
            // then we must stop checking the next public keys. If an attacker glitches past this check,
            // and the signature doesn't match, that's fine because it will be caught by the caller.
            bollard!(die_no_std, 4);
            let (mr, er) = (out_inner.masked_root(), out_inner.expected_root());
            // crate::println!("pq final: {:x?} | {:x?}", mr, er);
            if mr.len() != er.len() {
                continue;
            }
            let mut matched: usize = 0;
            for mr_i in 0..mr.len() {
                // as this check is merely advisory to see if we should check another key, only lightly harden
                bollard!(die_no_std, 4);
                let unmask = 0u8.wrapping_sub(((mask >> mr_i) & 1) as u8); // 0x00 or 0xFF
                let cand = core::hint::black_box(mr[mr_i]) ^ unmask;
                if cand != er[mr_i] {
                    break; // aborts the check, matched will be < mr.len()
                }
                matched += 1;
            }
            if matched != mr.len() {
                continue; // try next key
            }
            // if we got here, the signature is a match - pass the key back for further checks
            break;
        }
    }
    bollard!(die_no_std, 4);
    // iox.set_gpio_pin(IoxPort::PC, 6, IoxValue::Low);

    // the return value will either be None - no matches found; or Some(possibly matching key) or
    // Some(possibly not matching key). The comparison work shall be done up top.
    out
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
    bollard!(die_no_std, 4);
    csprng.as_deref_mut().map(|rng| rng.random_delay());
    // only clear ACL if it isn't already cleared
    if slot_mgr
        .get_acl(slot)
        .unwrap_or(AccessSettings::Data(DataSlotAccess::new_with_raw_value(0xFFFF_FFFF)))
        .raw_u32()
        != 0
    {
        // clear the ACL so we can operate on the data
        // Don't panic on failure: the panic can be used as a primitive to prevent
        // further erasure.
        slot_mgr.set_acl(&mut rram, slot, &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0))).ok();
    }
    let bytes = unsafe { slot_mgr.read_unchecked(slot) };
    // only erase if the key hasn't already been erased, to avoid stressing the RRAM array
    // erase_secrets() may be called on every boot in some modes.
    bollard!(die_no_std, 4);
    if !bytes.iter().all(|&b| b == ERASE_VALUE) {
        let mut eraser = alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
        eraser.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, ERASE_VALUE);

        slot_mgr.write(&mut rram, slot, &eraser).ok();
    }
    let check = unsafe { slot_mgr.read_unchecked(slot) };
    if !check.iter().all(|&b| b == ERASE_VALUE) {
        crate::println!("Failed to erase key at {:?}: {:x?}", slot, check);
    }
    bollard!(die_no_std, 4);
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
        let (_pa, rw_perms) = slot.get_access_spec();
        let mut erased_keys = 0;
        csprng.as_deref_mut().map(|rng| rng.random_delay());
        match rw_perms {
            RwPerms::ReadWrite | RwPerms::WriteOnly => {
                // only clear ACL if it isn't already cleared
                if slot_mgr
                    .get_acl(slot)
                    .unwrap_or(AccessSettings::Data(DataSlotAccess::new_with_raw_value(0xFFFF_FFFF)))
                    .raw_u32()
                    != 0
                {
                    // clear the ACL so we can operate on the data
                    match slot_mgr.set_acl(
                        &mut rram,
                        slot,
                        &AccessSettings::Data(DataSlotAccess::new_with_raw_value(0)),
                    ) {
                        Ok(_) => (),
                        Err(e) => {
                            crate::println!("Couldn't erase ACL: {:?}", e);
                        }
                    }
                }
                // safety: this function knows to check or expect invalid ACL situations
                let bytes = unsafe { slot_mgr.read_unchecked(slot) };
                if bytes.iter().all(|&b| b == 0) {
                    zero_key_count += 1;
                }
                // only erase if the key hasn't already been erased, to avoid stressing the RRAM array
                // erase_secrets() may be called on every boot in some modes.
                bollard!(die_no_std, 4);
                if !bytes.iter().all(|&b| b == ERASE_VALUE) {
                    let mut eraser = alloc::vec::Vec::with_capacity(slot.len() * SLOT_ELEMENT_LEN_BYTES);
                    eraser.resize(slot.len() * SLOT_ELEMENT_LEN_BYTES, ERASE_VALUE);
                    slot_mgr.write(&mut rram, slot, &eraser).ok();
                }
                let check = unsafe { slot_mgr.read_unchecked(slot) };
                if !check.iter().all(|&b| b == ERASE_VALUE) {
                    crate::println!("Failed to erase key at {:?}: {:x?}", slot, check);
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
        crate::println!(
            "Key range at {}: {}/{} keys confirmed erased",
            slot.get_base(),
            erased_keys,
            slot.len()
        );
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
    pq_tag: Option<[u8; 4]>,
) -> Result<(), String> {
    if key == DEVELOPER_KEY_SLOT {
        // this is a common case - if we're not under attack, and we're in developer mode,
        // just short circuit the rest of the checks and erase the keys.
        return erase_secrets(&mut Some(csprng));
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    // if the tag is the developer tag, erase the keys.
    if &tag == bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT] {
        erase_secrets(&mut Some(csprng))?;
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    if pq_tag == Some(*bao1x_api::pubkeys::KEYSLOT_INITIAL_TAGS[bao1x_api::pubkeys::DEVELOPER_KEY_SLOT]) {
        erase_secrets(&mut Some(csprng))?;
    }
    bollard!(die_no_std, 4);
    csprng.random_delay();
    // second check on the inverse-key type - this requires a double-glitch to bypass the key number check
    if (!key_inv) == DEVELOPER_KEY_SLOT {
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
        // if the tag is the developer tag, erase the keys.
        if &tag == b"dev " {
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

pub fn jump_to(target: usize, mask: usize) -> ! {
    // loader expects a0 to have the address of the kernel image pre-loaded
    let kernel_loc = bao1x_api::offsets::KERNEL_START;
    unsafe {
        core::arch::asm!(
            "mv t0, {target}",
            "mv t1, {mask}",
            "mv a0, {kernel_loc}",
            "mv a1, x0",
            "xor t0, t1, t0",
            "jr t0",
            target = in(reg) target,
            mask = in(reg) mask,
            kernel_loc = in(reg) kernel_loc,
            options(noreturn)
        );
    }
}

pub fn die_no_std() -> ! {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm! (
            // TODO: any SCE other security-sensitive registers to zeroize?

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

            "csrw        mscratch, x0",

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
