//! Counter-signature support for third-party `boot1` images.
//!
//! # What a counter-signature is for
//!
//! `boot0` erases `collateral` whenever it cannot prove that the party owning a
//! `boot1` key manifest actually blessed the code that manifest is attached to.
//! Without that proof, a compromised Baochip key could staple a third party's
//! manifest onto hostile code and inherit their collateral. The counter-signature
//! is that proof: the manifest owner signs the *Baochip signature* over the image,
//! after Baochip has produced it.
//!
//! # Where it lives
//!
//! In the 128-byte tail of `SignatureAppendixInFlash`, immediately after the PQ
//! signature:
//!
//!   [sealed_data_end(),        `SignaturePqInFlash`
//!    + SIGNATURE_PQ_LENGTH)
//!   [.. + 64)                  `manifest_sig`
//!   [.. + AAD_LENGTH)          `manifest_aad`
//!   [.. + 4)                   `manifest_aad_len`
//!
//! Nothing signs this region, which is the whole point: it is written after both
//! the ed25519 and PQ signatures are final, and editing it invalidates neither.
//! That also means this tool never needs to re-run any other pass.
//!
//! Note the offset is `sealed_data_end() + SIGNATURE_PQ_LENGTH` *unconditionally* --
//! `manifest_appendix()` has no `pq_enabled` guard. An image built without a PQ
//! signature must still reserve the 3856-byte slot or the appendix will not be
//! where boot0 reads it. This tool refuses to write a short image rather than
//! writing bytes nothing will read.
//!
//! # Output
//!
//! A successful counter-signature also writes `<image>.uf2`, suppressible with
//! `--no-uf2`. The partition is not inferred or overridable -- it is boot1 by
//! construction.
//!
//! # The two signing protocols
//!
//! `check_counter_sig` selects on `manifest_aad_len`, mirroring how the main
//! signature selects on `aad_len`:
//!
//!   aad_len == 0   Ed25519ph. `verify_prehashed(SHA-512(classical_sig), None, sig)`.
//!                  What `--countersign-pem` produces.
//!   aad_len  > 0   FIDO2/WebAuthn. `verify_strict(aad || SHA-256(classical_sig), sig)`,
//!                  where `aad` is the token's `auth_data`. What `-c` produces.
//!
//! # Slot semantics
//!
//! The verifier walks the image's own key manifest in ascending order and accepts
//! the first slot that validates. A hit on slot 3 is read as a third-party
//! *developer* self-signature: boot proceeds, but collateral is erased anyway. This
//! tool reports which slot matched and calls out the slot-3 case, because a
//! counter-signature that "works" from slot 3 is not the outcome you are usually
//! testing for.

use std::convert::TryInto;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

use bao1x_api::signatures::{
    AAD_LENGTH, FunctionCode, SIGNATURE_LENGTH, SIGNATURE_PQ_LENGTH, SignatureAppendixInFlash,
    SignatureInFlash,
};
use base64::{Engine as _, engine::general_purpose};
use digest::Digest;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use sha2::{Sha256, Sha512};

/// Bytes of `SignatureAppendixInFlash` that follow the PQ signature. Derived from
/// the struct so it cannot drift from bao1x-api.
pub const MANIFEST_APPENDIX_LEN: usize = size_of::<SignatureAppendixInFlash>() - SIGNATURE_PQ_LENGTH;
const _: () = assert!(MANIFEST_APPENDIX_LEN == SIGNATURE_LENGTH + AAD_LENGTH + 4);

/// Offset of `manifest_aad_len` within the appendix tail.
const AAD_LEN_OFFSET: usize = SIGNATURE_LENGTH + AAD_LENGTH;

/// Slot 3 is the developer slot by position, regardless of tag. Kept local rather
/// than imported so this file has no dependency on where the constant lives.
const DEVELOPER_KEY_SLOT: usize = 3;

pub fn run(
    file_path: &Path,
    credential_file: Option<&Path>,
    pem_file: Option<&Path>,
    no_uf2: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Counter-signing a boot1 manifest");
    println!("================================\n");

    let file = fs::read(file_path).map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;

    let (offset, sig) = crate::locate_signature_record(&file).ok_or_else(|| {
        format!("'{}' is not a signed bao1x image (no signature magic found)", file_path.display())
    })?;

    // boot0 only performs the counter-signature check on `nextboot1`. Writing an
    // appendix onto anything else would produce bytes no verifier ever reads, while
    // implying the image had been countersigned. Refuse rather than mislead.
    let fc = sig.sealed_data.function_code;
    if fc != FunctionCode::Boot1 as u32 && fc != FunctionCode::UpdatedBoot1 as u32 {
        return Err(format!(
            "counter-signing is only defined for boot1 images; '{}' has function code {:#x} ({})",
            file_path.display(),
            fc,
            crate::function_code_name(fc).unwrap_or("unrecognised")
        )
        .into());
    }

    let appendix_offset = offset + sig.sealed_data_end() + SIGNATURE_PQ_LENGTH;
    if file.len() < appendix_offset + MANIFEST_APPENDIX_LEN {
        return Err(format!(
            "'{}' is {} bytes, but the manifest appendix belongs at [{}, {}).\n       \
             boot0 reads it at sealed_data_end() + {} with no pq_enabled guard, so the image must \
             carry the full PQ slot -- zero-filled if unsigned -- followed by {} appendix bytes.\n       \
             Rebuild with a sign_image that reserves them.",
            file_path.display(),
            file.len(),
            appendix_offset,
            appendix_offset + MANIFEST_APPENDIX_LEN,
            SIGNATURE_PQ_LENGTH,
            MANIFEST_APPENDIX_LEN
        )
        .into());
    }

    println!("Signature record at offset {}", offset);
    println!("  appendix slot: offset {} ({} bytes)", appendix_offset, MANIFEST_APPENDIX_LEN);
    println!("  countersigning over classical signature: {}", hex::encode(sig.signature));

    let existing = &file[appendix_offset..appendix_offset + MANIFEST_APPENDIX_LEN];
    if existing.iter().any(|&b| b != 0) {
        println!("  note: image already carries a counter-signature; it will be replaced");
    }

    let (manifest_sig, manifest_aad) = match (pem_file, credential_file) {
        (Some(pem_path), None) => sign_with_pem(pem_path, &sig.signature)?,
        (None, Some(cred_path)) => sign_with_token(cred_path, &sig.signature)?,
        (Some(_), Some(_)) => {
            return Err("--countersign takes either --countersign-pem or -c, not both".into());
        }
        (None, None) => {
            return Err("--countersign needs a key: --countersign-pem <key.pem> or -c <cred.json>".into());
        }
    };

    if manifest_aad.len() > AAD_LENGTH {
        return Err(format!(
            "auth_data is {} bytes; the appendix reserves {}",
            manifest_aad.len(),
            AAD_LENGTH
        )
        .into());
    }

    let mut appendix = [0u8; MANIFEST_APPENDIX_LEN];
    appendix[..SIGNATURE_LENGTH].copy_from_slice(&manifest_sig);
    appendix[SIGNATURE_LENGTH..SIGNATURE_LENGTH + manifest_aad.len()].copy_from_slice(&manifest_aad);
    appendix[AAD_LEN_OFFSET..].copy_from_slice(&(manifest_aad.len() as u32).to_le_bytes());

    let mut f = OpenOptions::new().read(true).write(true).open(file_path)?;
    f.seek(SeekFrom::Start(appendix_offset as u64))?;
    f.write_all(&appendix)?;
    f.flush()?;
    drop(f);
    println!("  wrote {} bytes at offset {}", MANIFEST_APPENDIX_LEN, appendix_offset);
    println!("  Updated counter-signature in {:?}", file_path);

    // Read back and run the same computation boot0 runs, against the manifest the
    // image itself commits to. Only emit the .uf2 once that passes -- a .uf2 whose
    // appendix does not verify is worse than no .uf2, because it flashes cleanly.
    verify_in_file(file_path, offset)?;

    emit_uf2(file_path, fc, no_uf2)
}

/// Ed25519ph counter-signature from a PKCS#8 PEM key. `aad_len` stays 0.
fn sign_with_pem(
    path: &Path,
    classical_sig: &[u8; SIGNATURE_LENGTH],
) -> Result<([u8; SIGNATURE_LENGTH], Vec<u8>), Box<dyn std::error::Error>> {
    println!("\nCounter-signing with a PEM key");
    println!("------------------------------");
    let seed = read_pkcs8_ed25519_seed(path)?;
    let sk = SigningKey::from_bytes(&seed);
    println!("  key file: {}", path.display());
    println!("  public key: {}", hex::encode(sk.verifying_key().to_bytes()));

    let mut h: Sha512 = Sha512::new();
    h.update(classical_sig);
    let s = sk.sign_prehashed(h, None).map_err(|e| format!("counter-signature failed: {}", e))?;
    Ok((s.to_bytes(), Vec::new()))
}

/// FIDO2 counter-signature. The token signs `auth_data || SHA-256(message)`, which
/// is exactly the `aad_len > 0` branch when the message is the classical signature.
/// This is the same helper the main ed25519 pass uses, with a different message.
fn sign_with_token(
    cred_path: &Path,
    classical_sig: &[u8; SIGNATURE_LENGTH],
) -> Result<([u8; SIGNATURE_LENGTH], Vec<u8>), Box<dyn std::error::Error>> {
    println!("\nCounter-signing with a FIDO2 token");
    println!("----------------------------------");
    let cred = crate::load_credential(cred_path)?;
    println!("  credential: {} ({} bytes)", cred_path.display(), cred.credential_id.len());
    println!("  touch the token to counter-sign...");

    let assertion = crate::sign_ed25519_hash(&cred.credential_id, classical_sig, &cred.pin)?;
    if assertion.signature.len() != SIGNATURE_LENGTH {
        return Err(format!(
            "token returned a {}-byte signature, expected {}",
            assertion.signature.len(),
            SIGNATURE_LENGTH
        )
        .into());
    }
    let mut s = [0u8; SIGNATURE_LENGTH];
    s.copy_from_slice(&assertion.signature);
    println!("  auth_data: {} bytes", assertion.auth_data.len());
    Ok((s, assertion.auth_data.clone()))
}

/// Pull the 32-byte Ed25519 seed out of a PKCS#8 PEM file.
///
/// A v1 Ed25519 PKCS#8 body ends with an OCTET STRING header (0x04 0x20) followed
/// by the 32 seed bytes. This makes the same structural assumption `sign_image.rs`
/// makes, but checks the two tag bytes rather than trusting the tail blindly.
fn read_pkcs8_ed25519_seed(path: &Path) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("Failed to read PEM '{}': {}", path.display(), e))?;
    let body: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("-----"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|e| format!("'{}' is not valid base64 PEM: {}", path.display(), e))?;

    if der.len() < 34 || der[der.len() - 34] != 0x04 || der[der.len() - 33] != 0x20 {
        return Err(format!(
            "'{}' does not look like a PKCS#8 Ed25519 private key ({} DER bytes; expected a \
             trailing 0x04 0x20 OCTET STRING of 32)",
            path.display(),
            der.len()
        )
        .into());
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&der[der.len() - 32..]);
    Ok(seed)
}

/// Replicate `check_counter_sig` against the file as it now sits on disk.
fn verify_in_file(file_path: &Path, offset: usize) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nVerifying against the image's own key manifest");
    println!("---------------------------------------------");

    let file =
        fs::read(file_path).map_err(|e| format!("Failed to re-read '{}': {}", file_path.display(), e))?;
    let mut sig = SignatureInFlash::default();
    sig.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);

    let appendix_offset = offset + sig.sealed_data_end() + SIGNATURE_PQ_LENGTH;
    let appendix = &file[appendix_offset..appendix_offset + MANIFEST_APPENDIX_LEN];

    let mut manifest_sig = [0u8; SIGNATURE_LENGTH];
    manifest_sig.copy_from_slice(&appendix[..SIGNATURE_LENGTH]);
    let manifest_aad = &appendix[SIGNATURE_LENGTH..AAD_LEN_OFFSET];
    let aad_len = u32::from_le_bytes(appendix[AAD_LEN_OFFSET..].try_into().unwrap()) as usize;

    if aad_len > AAD_LENGTH {
        return Err(format!("manifest_aad_len is {}, exceeding {}", aad_len, AAD_LENGTH).into());
    }
    // Same discipline the verifier applies: no hiding places in the aad padding.
    if manifest_aad[aad_len..].iter().any(|&b| b != 0) {
        return Err("manifest_aad padding is not zero; boot0 rejects this outright".into());
    }
    println!("  protocol: {}", if aad_len == 0 { "Ed25519ph" } else { "FIDO2" });

    let ed_sig = Signature::from_bytes(&manifest_sig);
    for (i, pk) in sig.sealed_data.pubkeys.iter().enumerate() {
        let Ok(vk) = VerifyingKey::from_bytes(&pk.pk) else { continue };
        let ok = if aad_len == 0 {
            let mut h: Sha512 = Sha512::new();
            h.update(&sig.signature);
            vk.verify_prehashed(h, None, &ed_sig).is_ok()
        } else {
            let mut h: Sha256 = Sha256::new();
            h.update(&sig.signature);
            let digest = h.finalize();
            let mut msg = [0u8; AAD_LENGTH + 32];
            msg[..aad_len].copy_from_slice(&manifest_aad[..aad_len]);
            msg[aad_len..aad_len + 32].copy_from_slice(digest.as_slice());
            vk.verify_strict(&msg[..aad_len + 32], &ed_sig).is_ok()
        };
        if ok {
            println!("* counter-signature verifies against key manifest slot {}", i);
            if i == DEVELOPER_KEY_SLOT {
                println!(
                    "  WARNING: slot {} is the developer slot. boot0 accepts this counter-signature \
                     but erases collateral anyway, treating it as a third-party dev self-signature. \
                     For a collateral-preserving flow, counter-sign with a key in slots 0-2.",
                    DEVELOPER_KEY_SLOT
                );
            }
            return Ok(());
        }
    }

    Err("counter-signature does not verify against any key in the image's own manifest. \
         boot0 would erase collateral. Check that the counter-signing key is one of the four \
         committed in this image's key manifest."
        .into())
}

/// Emit the .uf2 alongside the counter-signed image.
///
/// No `-p` plumbing and no function-code probing: counter-signing is gated to boot1
/// above, so the partition is already fixed. A boot1 signature record is also never
/// displaced the way a swap image's is, so the whole file maps to `BOOT1_START` with
/// no offset.
///
/// The .uf2 covers the file as it now stands, which includes the reserved PQ slot and
/// the 128-byte appendix. That is the point -- the counter-signature has to reach
/// flash, and boot0 reads it at a fixed distance past the payload.
///
/// `UpdatedBoot1` is deliberately excluded. It marks an image handed to a running
/// boot1 as an update payload rather than something flashed to a fixed address, so
/// there is no single correct load address to bake in. `function_code_name` omits it
/// for the same reason.
fn emit_uf2(file_path: &Path, function_code: u32, no_uf2: bool) -> Result<(), Box<dyn std::error::Error>> {
    if no_uf2 {
        return Ok(());
    }
    if function_code != FunctionCode::Boot1 as u32 {
        println!(
            "\nNo .uf2 written: function code {:#x} is an update payload, which has no fixed \
             load address. Apply this image through the updater instead.",
            function_code
        );
        return Ok(());
    }

    let output = file_path.with_extension("uf2");
    println!("\nBuilding image file for partition boot1, writing to {:?}...", output);
    crate::convert_to_uf2(&file_path, &output, Some("boot1"), None)?;
    Ok(())
}
