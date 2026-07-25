//! fido-signer - production signing for bao1x flash images.
//!
//! The image generator always emits a *development*-signed image: a complete,
//! final-layout binary whose ed25519 signature (and, when built with PQ
//! support, whose SLH-DSA signature) was produced with throwaway keys. This
//! tool replaces those signatures in place with production ones. It never
//! changes the size or layout of the image, so the input and output are
//! byte-for-byte identical apart from the signature fields (plus the toolchain
//! hash, if `--baobit-hash` is given) - the one exception being `--strip-pq`,
//! which deliberately reverts an image to the smaller legacy layout.
//!
//! The two signatures are independent. Supply `-c` to replace the ed25519
//! signature, `--pq-key` to replace the SLH-DSA signature, or both to do the
//! whole job in one invocation - which is the intended production case, so that
//! an air-gapped signing host never has to shuttle intermediate files around.
//! The ed25519 private key stays on the FIDO2 token; the SLH-DSA private key
//! lives on the signing host itself (it has to - the tree build is far too slow
//! to push through a token). At least one of `-c`, `--pq-key`, or `--strip-pq`
//! must be given.
//!
//! # Usage
//!
//!   fido-signer -c <cred.json> -f <image.bin> --pq-key <slh-dsa.key> \
//!               [--pq-cache <tree.cache>]
//!       Replace both signatures. The normal production invocation.
//!
//!   fido-signer -c <cred.json> -f <image.bin>
//!       Replace the ed25519 signature only. Correct for a classically-signed
//!       image; on a PQ-enabled image it leaves the SLH-DSA signature as it
//!       found it.
//!
//!   fido-signer -f <image.bin> --pq-key <slh-dsa.key> [--pq-cache <tree.cache>]
//!       Replace the SLH-DSA signature only, with no token present. The ed25519
//!       signature is left untouched, so this is a half-signed image until the
//!       `-c` pass runs.
//!
//!   fido-signer -c <cred.json> -f <image.bin> --strip-pq
//!       Turn a PQ image back into a legacy (pre-PQ) image and re-sign it with
//!       ed25519. Clears the PQ fields, drops the SLH-DSA tail, and produces a
//!       smaller file that exercises the pre-PQ verify path on an old bootloader
//!       and is treated as a plain ed25519 image by a new one.
//!
//!   fido-signer -c <cred.json> -m <base64>
//!       Sign a raw base64 message with the token. Testing aid; requires `-c`,
//!       and `--pq-key` / `--strip-pq` are not accepted, since there is no image
//!       header to patch.
//!
//! Splitting the work across two invocations is safe in either order: the
//! ed25519 pass writes only the unsigned prefix of the record, and the SLH-DSA
//! pass writes only the tail past `sealed_data_end()`. Neither is inside the
//! region the other one signs. `--baobit-hash` and `--strip-pq`, however, both
//! edit the signed `sealed_data` and so are not order-free - see below.
//!
//! Useful flags:
//!   -b/--baobit-hash <hex20>  Inject the toolchain hash into `sealed_data`
//!                             *before* hashing. Both signatures cover it, so
//!                             any signature applied before the injection is
//!                             invalidated by it. Use it on the first pass, or
//!                             on a single combined pass.
//!   --strip-pq                Remove the PQ signature fields entirely and drop
//!                             the SLH-DSA tail, producing a legacy image. Edits
//!                             the signed region, so it requires a following (or
//!                             same-run) `-c` pass. Mutually exclusive with
//!                             `--pq-key`.
//!   -p/--function-code <code> Override the function code read out of the image.
//!                             Rarely needed; see below.
//!   --no-uf2                  Suppress the .uf2 output.
//!   --pq-force                Sign even if the PQ public key is not listed in
//!                             the image's own `pubkeys_pq` slots. Only useful
//!                             for bring-up; such an image will not boot.
//!
//! # What --strip-pq clears
//!
//! A genuine pre-PQ image ends its meaningful `sealed_data` at `toolchain`;
//! everything after it is zero-padding from the old generator. `--strip-pq`
//! reproduces that by zeroing all three of the PQ-era fields:
//!
//!   - `pq_enabled`        what makes a modern verifier attempt a PQ check at all;
//!   - `pubkeys_pq`        the committed PQ public keys;
//!   - `corrected_version` the marker a modern verifier reads as "legacy record" (a value of 0 selects the
//!     legacy compatibility path).
//!
//! and then truncates the trailing `SignaturePqInFlash`. Clearing
//! `corrected_version` is what makes this a *legacy* image rather than a
//! *modern non-PQ* image; drop that one field from the strip if you want the
//! latter. A pre-PQ bootloader reads none of these three, so it accepts the
//! result on the strength of the ed25519 signature alone; a modern bootloader
//! sees a legacy record with no PQ signature and verifies ed25519 only.
//!
//! # Function code and the .uf2
//!
//! The function code is read out of `sealed_data.function_code`, so a plain
//! `-f ...` invocation is enough: if the code is one this tool knows how to
//! place (boot0, boot1, loader, kernel, app, swap, baremetal) the matching
//! `<image>.uf2` is written automatically. `-p` overrides the embedded value
//! and is only needed for an image whose function code is unset or unrecognised;
//! when it disagrees with the image, the override is called out on the console.
//! `--no-uf2` skips the conversion entirely.
//!
//! Because the code now comes from the image, the *location* of the signature
//! record does too. A `swap` image carries its metadata header ahead of the
//! signature block, so the record does not sit at offset 0; the tool probes
//! both candidate offsets for the magic number and reports where it landed.
//! This is why `-p swap` is no longer required to sign a swap image, and why
//! `-p` can no longer put the tool on the wrong offset.
//!
//! # Order of operations
//!
//! 1. Locate the signature record and recover the function code from it.
//! 2. Patch the toolchain hash, if given. It lives inside `sealed_data`, so it must be final before anything
//!    is hashed.
//! 3. Strip the PQ fields, if `--strip-pq`. Also edits `sealed_data` and drops the tail, so it too must
//!    happen before hashing.
//! 4. Run every PQ preflight check - key loads, cache matches the key, the image has `pq_enabled` set, the
//!    image reserves a signature slot of the right size, and the key's public key is one of the four
//!    committed in the header. All of this happens *before* the token is touched, so a mismatched key never
//!    costs an operator a wasted user-presence gesture.
//! 5. ed25519, if `-c`: hash the sealed region, get the assertion (touch the token), patch `signature` /
//!    `aad` / `aad_len`.
//! 6. SLH-DSA, if `--pq-key`: hash the same region with SHA-256, sign the digest, write the signature at
//!    `sealed_data_end()`. This is the long, unattended step, so it deliberately runs after the operator's
//!    touch rather than before it.
//! 7. Read the file back and verify the PQ signature against the public key committed in the image's own
//!    header - the same computation the boot verifier performs.
//! 8. Emit the .uf2 for the function code from step 1.
//!
//! # What is covered by what
//!
//! Within the image (all offsets relative to `offset`, which is nonzero only
//! for `swap`, whose metadata header precedes the signature block):
//!
//!   [0, sealed_data_offset())          jal || ed25519 sig || aad_len || aad.
//!                                      Unsigned; this is what step 5 patches.
//!   [sealed_data_offset(),             `protected` = sealed_data || zero pad
//!    sealed_data_end())                || payload. Covered by BOTH signatures.
//!                                      `sealed_data_end()` is
//!                                      `signed_len + sealed_data_offset()`.
//!   [sealed_data_end(),                `SignaturePqInFlash`. Covered by
//!    + SIGNATURE_PQ_LENGTH)            neither; this allows ed25519 to work without pq
//!
//! Note the ed25519 digest is bounded by `sealed_data_end()`, *not* by the end
//! of the file. Those are the same thing only for an image with no PQ tail; if
//! the hash ran to EOF on a PQ-enabled image it would include the PQ signature
//! bytes and diverge from what the boot verifier computes.
//!
//! # Build
//!
//! Requires slh-dsa with the `tree-cache` feature (for `--pq-cache`); the
//! `parallel` feature is strongly recommended on the signing host, as it
//! fork-joins the Merkle tree build and produces identical output.
//!
//!   slh-dsa = { path = "../slh-dsa", features = ["tree-cache", "parallel"] }

use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use bao1x_api::signatures::{FunctionCode, PUBLIC_KEY_PQ_LENGTH, SIGNATURE_PQ_LENGTH, SignatureInFlash};
use base64::{Engine as _, engine::general_purpose};
use clap::{ArgGroup, Parser};
use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Assertion;
use ctap_hid_fido2::{Cfg, FidoKeyHidFactory, fidokey::GetAssertionArgsBuilder};
use digest::Digest;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha512};
use slh_dsa::signature::Keypair; // `verifying_key()`
use slh_dsa::{
    Sha2_128_24, Signature as PqSignature, SigningKey as PqSigningKey, VerifyingKey as PqVerifyingKey,
    XmssTreeCache,
};

/// The SLH-DSA parameter set. Must match the boot verifier and the size of the
/// `pubkeys_pq` / `SignaturePqInFlash` fields in `bao1x_api`.
type PqParams = Sha2_128_24;

/// Number of PQ public key slots committed in a signature record.
const PUBKEY_PQ_SLOTS: usize = 4;

/// Where the signature record sits in a `swap` image: the swap metadata header
/// occupies `[0, SWAP_SIG_OFFSET)` and the signature block follows it, ending
/// at `SWAP_HEADER_LEN`. Every other function code puts the record at 0.
const SWAP_SIG_OFFSET: usize =
    bao1x_api::offsets::baosec::SWAP_HEADER_LEN - bao1x_api::signatures::SIGBLOCK_LEN;

/// FIDO Signer - Sign messages or files using FIDO credentials
#[derive(Parser, Debug)]
#[command(
    name = "fido-signer",
    author = "bunnie",
    about = "Sign messages and bao1x image files with a FIDO token and/or an SLH-DSA key"
)]
#[command(version, long_about = None)]
#[command(group(
    ArgGroup::new("input")
        .required(true)
        .args(["message", "file"])
))]
struct Args {
    /// Path to the credential file (JSON format). When omitted, the ed25519
    /// signature is left untouched and only the PQ signature is applied;
    /// `--pq-key` is then required.
    #[arg(short = 'c', long, value_name = "FILE")]
    credential_file: Option<PathBuf>,

    /// Message to sign (base64 encoded)
    #[arg(short = 'm', long, value_name = "BASE64_MESSAGE", conflicts_with = "file")]
    message: Option<String>,

    /// File to sign
    #[arg(short = 'f', long, value_name = "FILE", conflicts_with = "message")]
    file: Option<PathBuf>,

    /// Function code e.g. partition. Normally read out of the image; supply
    /// this only to override an unset or unrecognised embedded value.
    #[arg(short = 'p', long = "function-code", value_name = "CODE")]
    function_code: Option<String>,

    /// Do not emit a .uf2 file.
    #[arg(long = "no-uf2")]
    no_uf2: bool,

    /// Hash to embed in the signature (hex string)
    #[arg(short = 'b', long = "baobit-hash", value_name = "HASH")]
    baobit_hash: Option<String>,

    /// Remove the PQ signature fields entirely, producing a legacy (pre-PQ)
    /// image: clears `corrected_version`, `pq_enabled`, and `pubkeys_pq` in
    /// `sealed_data` and drops the trailing SLH-DSA signature. Edits the signed
    /// region, so an ed25519 re-sign (`-c`) must follow (ideally in the same
    /// run). Mutually exclusive with `--pq-key`.
    #[arg(long = "strip-pq", conflicts_with_all = ["message", "pq_key"])]
    strip_pq: bool,

    /// SLH-DSA (post-quantum) secret key, raw 4*n bytes as emitted by
    /// `xous-pq-sign-image keygen`. When present, the image's PQ signature is
    /// replaced. May be used with or without `-c`.
    #[arg(short = 'q', long = "pq-key", value_name = "FILE", conflicts_with = "message")]
    pq_key: Option<PathBuf>,

    /// XMSS tree cache matching --pq-key. Optional; skips ~80% of the signing
    /// work. Validated against the key before use.
    #[arg(long = "pq-cache", value_name = "FILE", requires = "pq_key")]
    pq_cache: Option<PathBuf>,

    /// Apply the PQ signature even if the key's public key is not committed in
    /// the image's `pubkeys_pq` slots. The resulting image will not boot; this
    /// exists only for bring-up.
    #[arg(long = "pq-force", requires = "pq_key")]
    pq_force: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct Credentials {
    credential_id: String, // Base64 encoded, will be decoded to Vec<u8>
    pin: String,           // UTF-8 string
}

/// A loaded, decoded credential, ready to hand to the token.
struct FidoCredential {
    credential_id: Vec<u8>,
    pin: String,
}

fn load_credential(path: &Path) -> Result<FidoCredential, Box<dyn std::error::Error>> {
    let credential_data =
        fs::read_to_string(path).map_err(|e| format!("Failed to read credential file: {}", e))?;

    let credentials: Credentials = serde_json::from_str(&credential_data)
        .map_err(|e| format!("Failed to parse credential JSON: {}", e))?;

    // Decode the credential_id from base64
    let credential_id: Vec<u8> = general_purpose::STANDARD
        .decode(&credentials.credential_id)
        .map_err(|e| format!("Failed to decode credential_id: {}", e))?;

    Ok(FidoCredential { credential_id, pin: credentials.pin })
}

/// Map a raw `sealed_data.function_code` back to the string the rest of the
/// toolchain uses. Written as guard arms rather than literal patterns so that
/// it depends only on the variant names in `bao1x_api`, never on the numeric
/// discriminants - the enum can be renumbered without touching this.
fn function_code_name(code: u32) -> Option<&'static str> {
    match code {
        c if c == FunctionCode::Boot0 as u32 => Some("boot0"),
        c if c == FunctionCode::Boot1 as u32 => Some("boot1"),
        c if c == FunctionCode::Loader as u32 => Some("loader"),
        c if c == FunctionCode::Kernel as u32 => Some("kernel"),
        c if c == FunctionCode::App as u32 => Some("app"),
        c if c == FunctionCode::Swap as u32 => Some("swap"),
        c if c == FunctionCode::Baremetal as u32 => Some("baremetal"),
        _ => None,
    }
}

/// Find the signature record by probing the offsets it is allowed to occupy.
///
/// Only `swap` shifts the record, so there are two candidates. Probing rather
/// than being told which one to use is what lets the function code be recovered
/// from the image: the old flow needed `-p swap` before it could read the
/// header, and the header is where the function code lives.
fn locate_signature_record(file: &[u8]) -> Option<(usize, SignatureInFlash)> {
    let mut candidates = vec![0usize];
    if SWAP_SIG_OFFSET != 0 {
        candidates.push(SWAP_SIG_OFFSET);
    }
    for offset in candidates {
        if file.len() < offset + size_of::<SignatureInFlash>() {
            continue;
        }
        let mut sig = SignatureInFlash::default();
        sig.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);
        if sig.sealed_data.magic == bao1x_api::signatures::MAGIC_NUMBER {
            return Some((offset, sig));
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("bao1x Production Signer");
    println!("=======================\n");

    // Parse command line arguments
    let args = Args::parse();

    // Each of the three passes is optional, but doing none of them is always a
    // mistake rather than a no-op worth honouring silently.
    if args.credential_file.is_none() && args.pq_key.is_none() && !args.strip_pq {
        return Err(String::from(
            "Nothing to do: supply -c <cred.json> to replace the ed25519 signature, \
             --pq-key <key> to replace the SLH-DSA signature, --strip-pq to make a legacy image, \
             or a combination.",
        )
        .into());
    }
    // There is no PQ path (add or strip) for a raw message: the PQ fields only
    // exist inside an image header. clap already excludes --pq-key/--strip-pq
    // from --message; this covers the remaining -c-less message case.
    if args.message.is_some() && args.credential_file.is_none() {
        return Err(String::from("--message requires -c <cred.json>").into());
    }

    let baobit_hash: Option<[u8; 20]> = args.baobit_hash.as_deref().map(|hex_str| {
        let vec = hex::decode(hex_str).expect("invalid hex string");
        vec.try_into().expect("Baobit hash must be exactly 20 bytes")
    });

    let credential = match args.credential_file {
        Some(ref path) => {
            println!("Credential file: {:?}", path);
            Some(load_credential(path)?)
        }
        None => {
            println!("No credential file given: the ed25519 signature will be left as-is.");
            None
        }
    };

    // Both --baobit-hash and --strip-pq mutate `sealed_data`, which the ed25519
    // signature covers. Doing either without re-applying ed25519 in the same run
    // leaves that signature stale until a later `-c` pass fixes it.
    if baobit_hash.is_some() && credential.is_none() {
        println!(
            "WARNING: --baobit-hash without -c invalidates the image's existing ed25519 \
             signature.\n         An ed25519 pass must follow before this image will boot."
        );
    }
    if args.strip_pq && credential.is_none() {
        println!(
            "WARNING: --strip-pq without -c invalidates the image's existing ed25519 \
             signature.\n         An ed25519 pass must follow before this image will boot."
        );
    }

    // Determine what to sign
    let mut is_bao1x = false;
    let mut offset = 0;
    // Populated only for bao1x images; carries everything needed to apply the
    // PQ signature once the ed25519 side is done.
    let mut pq_ctx: Option<PqContext> = None;
    // Function code to build the .uf2 for: from the image, unless overridden.
    let mut uf2_function_code: Option<String> = None;
    // The ed25519 payload. `None` whenever no ed25519 pass is being run.
    let mut data_to_sign: Option<Vec<u8>> = None;

    if let Some(message_b64) = args.message {
        // Decode message from base64 - used for testing signing operations with small pieces of temporary
        // data
        data_to_sign = Some(
            general_purpose::STANDARD
                .decode(&message_b64)
                .map_err(|e| format!("Failed to decode message: {}", e))?,
        );
    } else if let Some(ref file_path) = args.file {
        // Read file contents.
        let file = fs::read(&file_path)
            .map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?;

        // Check to see if this is a signed bao1x binary, by looking at the magic
        // number at each offset the signature record is allowed to occupy.
        if let Some((found_offset, _)) = locate_signature_record(&file) {
            is_bao1x = true;
            offset = found_offset;

            // inject the baobit hash here, if it is specified
            if let Some(toolchain) = baobit_hash {
                patch_hash_in_file(&file_path, &toolchain, offset)?;
            }

            // strip the PQ fields here, if requested. Like the baobit patch this
            // edits `sealed_data`, and it also truncates the tail, so it must
            // land before the sealed region is hashed below.
            if args.strip_pq {
                println!("\nStripping PQ fields to produce a legacy image...");
                strip_pq_from_file(&file_path, offset)?;
            }

            // Re-read the file so we hash the patched contents
            let file = fs::read(&file_path)
                .map_err(|e| format!("Failed to re-read file '{}': {}", file_path.display(), e))?;

            // Re-parse the header: the patches above changed `sealed_data`.
            let mut sig = SignatureInFlash::default();
            sig.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);

            // Bounds of the region both signatures cover. `sealed_data_end()` is
            // `signed_len + sealed_data_offset()`, i.e. exactly the `protected`
            // buffer the image generator hashed. Do NOT hash to EOF: on a
            // PQ-enabled image the tail holds the SLH-DSA signature, which is
            // outside the signed region by construction.
            let sealed_start = offset + SignatureInFlash::sealed_data_offset();
            let sealed_end = offset + sig.sealed_data_end();
            if sealed_end > file.len() {
                return Err(format!(
                    "signed_len ({}) runs {} bytes past the end of '{}' - truncated image?",
                    sig.sealed_data.signed_len,
                    sealed_end - file.len(),
                    file_path.display()
                )
                .into());
            }

            // Recover the function code from the image itself.
            let embedded = function_code_name(sig.sealed_data.function_code);

            println!("\nSignature record at offset {}", offset);
            match embedded {
                Some(name) => {
                    println!("  function code: {} ({:#x})", name, sig.sealed_data.function_code)
                }
                None => println!("  function code: UNRECOGNISED ({:#x})", sig.sealed_data.function_code),
            }
            println!(
                "  signed region: [{}, {}) ({} bytes)",
                sealed_start,
                sealed_end,
                sealed_end - sealed_start
            );
            println!("  file is {} bytes ({} bytes of tail)", file.len(), file.len() - sealed_end);
            println!(
                "  corrected_version: {:#010x}  pq_enabled: {:#010x}",
                sig.sealed_data.corrected_version, sig.sealed_data.pq_enabled
            );

            // A `swap` image is the only one whose record is displaced. If the
            // embedded code and the offset the record was found at disagree,
            // something is wrong with the image - say so rather than quietly
            // producing a .uf2 that flashes to the wrong address.
            let offset_implies_swap = offset == SWAP_SIG_OFFSET && SWAP_SIG_OFFSET != 0;
            if embedded == Some("swap") && !offset_implies_swap {
                println!(
                    "  WARNING: image claims function code 'swap' but its signature record is at \
                     offset {} rather than {}",
                    offset, SWAP_SIG_OFFSET
                );
            } else if embedded != Some("swap") && offset_implies_swap {
                println!(
                    "  WARNING: signature record is at the swap offset ({}) but the image's \
                     function code is not 'swap'",
                    SWAP_SIG_OFFSET
                );
            }

            // A PQ-only run leaves whatever ed25519 signature the image arrived
            // with - which, for a freshly generated image, is the development
            // one. Worth stating plainly on a production console.
            if credential.is_none() && sig.sealed_data.pq_enabled != 0 {
                println!(
                    "  NOTE: PQ-only run; the ed25519 signature in this image is unchanged \
                     (still the development one, unless a -c pass has already run)."
                );
            }

            // `-p` overrides whatever the image says; otherwise use the image.
            uf2_function_code = match args.function_code.as_deref() {
                Some(explicit) => {
                    match embedded {
                        Some(name) if name == explicit => {}
                        Some(name) => println!(
                            "  NOTE: -p {} overrides the function code embedded in the image ({})",
                            explicit, name
                        ),
                        None => println!(
                            "  NOTE: -p {} supplies a function code the image does not carry",
                            explicit
                        ),
                    }
                    Some(explicit.to_string())
                }
                None => {
                    if embedded.is_none() && !args.no_uf2 {
                        println!(
                            "  NOTE: no recognised function code in the image; no .uf2 will be \
                             written. Pass -p <code> to force one."
                        );
                    }
                    embedded.map(|s| s.to_string())
                }
            };

            // Everything that can fail about the PQ side is checked here, before
            // the operator is asked to touch the token. (--strip-pq and --pq-key
            // are mutually exclusive, so this never runs on a stripped image.)
            if let Some(ref pq_key_path) = args.pq_key {
                pq_ctx = Some(pq_preflight(
                    pq_key_path,
                    args.pq_cache.as_deref(),
                    &sig,
                    file.len(),
                    offset,
                    args.pq_force,
                )?);
            }

            if credential.is_some() {
                let mut h: Sha512 = Sha512::new();
                // hash the sealed region
                h.update(&file[sealed_start..sealed_end]);
                // finalize it and send it on for signing
                data_to_sign = Some(h.finalize().as_slice().to_vec());
            }
        } else {
            // Not a bao1x image: there is no header, so no PQ slot to patch, no
            // PQ fields to strip, and nothing to do but sign the raw bytes.
            if args.pq_key.is_some() {
                return Err(format!(
                    "'{}' is not a signed bao1x image (no magic number at offset 0 or {}); \
                     there is no PQ signature slot to patch",
                    file_path.display(),
                    SWAP_SIG_OFFSET
                )
                .into());
            }
            if args.strip_pq {
                return Err(format!(
                    "'{}' is not a signed bao1x image (no magic number at offset 0 or {}); \
                     there are no PQ fields to strip",
                    file_path.display(),
                    SWAP_SIG_OFFSET
                )
                .into());
            }
            data_to_sign = Some(file);
        }
    } else {
        // This shouldn't happen due to clap's ArgGroup, but handle it just in case
        return Err("Either --message or --file must be provided".into());
    }

    // ----- ed25519 pass -----------------------------------------------------

    let assertion = match (credential, data_to_sign) {
        (Some(cred), Some(data)) => {
            println!("\nEd25519 Signing with FIDO2 Token");
            println!("--------------------------------");
            println!("Credential ID (decoded): {} bytes", cred.credential_id.len());
            println!("PIN: [REDACTED]"); // Don't print the actual PIN
            println!("Data to sign: {} bytes", data.len());

            // Sign the hash
            // println!("Signing {:x?}", data);
            // let mut h_debug = sha2::Sha256::new();
            // h_debug.update(&data);
            // let h_debug_final = h_debug.finalize();
            // println!("hashed hash: {:x?}", h_debug_final.as_slice());
            let assertion = sign_ed25519_hash(&cred.credential_id, &data, &cred.pin)?;

            println!("\n* Signing successful!");
            println!("Signature length: {} bytes", assertion.signature.len());

            if assertion.signature.len() == 64 {
                println!("* Valid Ed25519 signature");
                // Ed25519 signature components
                let (r, s) = assertion.signature.split_at(32);
                println!("  R (hex): {}", hex::encode(r));
                println!("  S (hex): {}", hex::encode(s));
                println!("  auth_data (hex): {}", hex::encode(&assertion.auth_data));

                println!("  signature (b64): {}", general_purpose::STANDARD.encode(&assertion.signature));
                println!("  auth_data (b64): {}", general_purpose::STANDARD.encode(&assertion.auth_data));
            }
            Some(assertion)
        }
        _ => None,
    };

    // ----- patch the image --------------------------------------------------

    if is_bao1x {
        if let Some(ref file_path) = args.file {
            if let Some(ref assertion) = assertion {
                println!("\nPatching image file with ed25519 signature...");
                patch_signature_in_file(&file_path, &assertion.signature, &assertion.auth_data, offset)?;
            }

            // The PQ signature covers `protected`, which the ed25519 patch above
            // does not touch (it writes only the unsigned prefix), so this can
            // run afterwards - or in a separate invocation, in either order -
            // without invalidating anything.
            if let Some(ctx) = pq_ctx {
                patch_pq_signature_in_file(&file_path, offset, &ctx)?;
            }

            // The function code came from the image unless `-p` overrode it, so
            // the .uf2 is emitted without the operator having to name the
            // partition. If --strip-pq truncated the tail, the file is already
            // shorter here, so the .uf2 excludes the removed signature too.
            if !args.no_uf2 {
                if let Some(ref function_code) = uf2_function_code {
                    let output = file_path.with_extension("uf2");
                    println!(
                        "\nBuilding image file for partition {}, writing to {:?}...",
                        function_code, output
                    );
                    convert_to_uf2(file_path, &output, Some(function_code.as_str()), None)?
                }
            }
        }
    }

    Ok(())
}

fn sign_ed25519_hash(
    credential_id: &[u8],
    hash: &[u8],
    pin: &str,
) -> Result<Assertion, Box<dyn std::error::Error>> {
    let cfg = Cfg::init();

    let mut devices = ctap_hid_fido2::get_fidokey_devices();
    if devices.is_empty() {
        return Err("No FIDO2 devices found".into());
    }

    let device = devices.pop().unwrap();
    let fidokey = FidoKeyHidFactory::create_by_params(&[device.param], &cfg).unwrap();
    // println!("Using key {:?}", fidokey.get_info().unwrap());

    let assertion = GetAssertionArgsBuilder::new("ssh:", hash).pin(pin).credential_id(credential_id).build();
    match fidokey.get_assertion_with_args(&assertion) {
        Ok(mut a) => {
            println!("Key used {} times", a[0].sign_count);
            Ok(a.pop().unwrap())
        }
        Err(e) => Err(e.try_into().unwrap()),
    }
}

// ----- legacy conversion (--strip-pq) ---------------------------------------

/// Zero the PQ-era fields inside `sealed_data` and drop the trailing SLH-DSA
/// signature, turning a PQ image back into a faithful legacy (pre-PQ) image.
///
/// Clears `corrected_version`, `pq_enabled`, and `pubkeys_pq` - exactly the
/// region the pre-PQ generator left as zero-padding - and truncates the
/// `SignaturePqInFlash` tail. Everything touched is inside the ed25519-signed
/// region (or changes the file length), so the caller MUST re-apply the ed25519
/// signature afterwards or the image will not boot.
///
/// Returns the number of tail bytes removed.
fn strip_pq_from_file(file_path: &PathBuf, offset: usize) -> Result<usize, Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new().read(true).write(true).open(file_path)?;
    let mut header = read_header_from_file(&mut file, offset)?;

    let was_corrected = header.sealed_data.corrected_version;
    let was_enabled = header.sealed_data.pq_enabled;

    header.sealed_data.corrected_version = 0;
    header.sealed_data.pq_enabled = 0;
    for pk in header.sealed_data.pubkeys_pq.iter_mut() {
        *pk = Default::default();
    }

    write_header_to_file(&mut file, &header, offset)?;

    // Drop the PQ signature tail, if present. `sealed_data_end()` is where the
    // signed region ends and the tail begins; only a lone, correctly-sized PQ
    // signature is removed - anything else is left alone and flagged, since we
    // can't tell what it is.
    let sealed_end = (offset + header.sealed_data_end()) as u64;
    let file_len = file.seek(SeekFrom::End(0))?;
    let tail = file_len.saturating_sub(sealed_end);

    let removed = if tail == 0 {
        0
    } else if tail as usize == SIGNATURE_PQ_LENGTH {
        file.set_len(sealed_end)?;
        SIGNATURE_PQ_LENGTH
    } else {
        println!(
            "  WARNING: {} trailing bytes after the signed region is not a {}-byte PQ signature; \
             leaving the tail in place",
            tail, SIGNATURE_PQ_LENGTH
        );
        0
    };
    file.flush()?;

    println!(
        "  cleared corrected_version ({:#010x} -> 0), pq_enabled ({:#010x} -> 0), pubkeys_pq",
        was_corrected, was_enabled
    );
    if removed > 0 {
        println!("  removed {}-byte PQ signature tail (file now {} bytes)", removed, sealed_end);
    } else {
        println!("  no PQ signature tail to remove");
    }

    Ok(removed)
}

// ----- post-quantum (SLH-DSA) signing ---------------------------------------

/// Everything the PQ signing step needs, assembled and validated before the
/// FIDO token is touched.
struct PqContext {
    sk: PqSigningKey<PqParams>,
    cache: Option<XmssTreeCache<PqParams>>,
    /// Which `pubkeys_pq` slot this key matches, if any. `None` only under
    /// `--pq-force`.
    slot: Option<usize>,
}

/// Validate the PQ key, the (optional) tree cache, and the image, so that every
/// recoverable failure surfaces before the operator is asked for user presence.
///
/// Checks, in order:
///  - the key file parses as an SLH-DSA secret key for `PqParams`;
///  - the cache, if given, is well-formed and belongs to this key;
///  - `pq_enabled` is nonzero. It sits inside `sealed_data`, i.e. inside the ed25519-signed region, so it
///    cannot be patched here: an image built without PQ support has to be regenerated, not fixed up;
///  - the image reserves exactly `SIGNATURE_PQ_LENGTH` bytes after `sealed_data_end()` to hold the signature;
///  - the key's public key is one of the four committed in `pubkeys_pq`. The boot verifier walks that list,
///    so a signature from an unlisted key is dead on arrival.
fn pq_preflight(
    pq_key: &Path,
    pq_cache: Option<&Path>,
    header: &SignatureInFlash,
    file_len: usize,
    offset: usize,
    force: bool,
) -> Result<PqContext, Box<dyn std::error::Error>> {
    println!("\nPost-quantum (SLH-DSA) preflight");
    println!("--------------------------------");

    let sk_bytes = fs::read(pq_key)
        .map_err(|e| format!("Failed to read PQ secret key '{}': {}", pq_key.display(), e))?;
    let sk = PqSigningKey::<PqParams>::try_from(sk_bytes.as_slice()).map_err(|_| {
        format!(
            "Invalid PQ secret key in '{}': expected 64 bytes (4*n for SHA2-128-24), got {}",
            pq_key.display(),
            sk_bytes.len()
        )
    })?;
    let vk = sk.verifying_key();
    let vk_bytes = vk.to_bytes();
    if vk_bytes.len() != PUBLIC_KEY_PQ_LENGTH {
        return Err(format!(
            "Parameter set mismatch: this key's public key is {} bytes, the signature record \
             reserves {}",
            vk_bytes.len(),
            PUBLIC_KEY_PQ_LENGTH
        )
        .into());
    }
    println!("  public key: {}", hex::encode(vk_bytes.as_slice()));

    // The cache is message-independent, so it is safe to reuse across images -
    // but only with the key it was built from, hence the validation.
    let cache = match pq_cache {
        Some(path) => {
            let bytes = fs::read(path)
                .map_err(|e| format!("Failed to read tree cache '{}': {}", path.display(), e))?;
            let cache = XmssTreeCache::<PqParams>::from_bytes(&bytes)
                .map_err(|e| format!("Failed to parse tree cache '{}': {}", path.display(), e))?;
            if !cache.validate(&vk) {
                return Err(format!(
                    "Tree cache '{}' does not match this key (or is corrupt)",
                    path.display()
                )
                .into());
            }
            println!("  tree cache: {} ({} bytes, validated)", path.display(), bytes.len());
            Some(cache)
        }
        None => {
            println!("  tree cache: none - the full XMSS tree will be rebuilt (slow)");
            None
        }
    };

    if header.sealed_data.pq_enabled == 0 {
        return Err(String::from(
            "pq_enabled is 0 in this image: the boot verifier will ignore any PQ signature. \
             This field lives inside the ed25519-signed region and cannot be patched here - \
             regenerate the image with PQ support enabled.",
        )
        .into());
    }

    let pq_offset = offset + header.pq_signature_offset();
    let reserved = file_len.saturating_sub(pq_offset);
    if reserved != SIGNATURE_PQ_LENGTH {
        return Err(format!(
            "Image reserves {} bytes for the PQ signature at offset {}, expected {}. \
             This tool replaces a signature in place and will not change the image layout.",
            reserved, pq_offset, SIGNATURE_PQ_LENGTH
        )
        .into());
    }
    println!("  signature slot: offset {} ({} bytes)", pq_offset, SIGNATURE_PQ_LENGTH);

    let slot =
        (0..PUBKEY_PQ_SLOTS).find(|&i| header.sealed_data.pubkeys_pq[i].pk.as_slice() == vk_bytes.as_slice());
    match slot {
        Some(i) => println!("  key matches pubkeys_pq slot {}", i),
        None => {
            let msg = format!(
                "This key's public key is not committed in any pubkeys_pq slot of the image; \
                 the boot verifier would reject the result"
            );
            if force {
                println!("  WARNING: {} (--pq-force given, continuing)", msg);
            } else {
                return Err(format!("{}. Pass --pq-force to sign anyway.", msg).into());
            }
        }
    }

    Ok(PqContext { sk, cache, slot })
}

/// Compute the PQ signature over the sealed region and write it into the
/// reserved slot, then read the file back and verify the result the same way
/// the boot verifier will.
///
/// The signed message is SHA-256 of `protected`, matching the image generator's
/// pre-hash flow: the verifier streams the payload through a hardware hasher
/// and verifies the 32-byte digest, so the SLH-DSA code never touches the full
/// image.
fn patch_pq_signature_in_file(
    file_path: &PathBuf,
    offset: usize,
    ctx: &PqContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nApplying post-quantum (SLH-DSA) signature");
    println!("----------------------------------------");

    let file =
        fs::read(file_path).map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?;
    let mut header = SignatureInFlash::default();
    header.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);

    let sealed_start = offset + SignatureInFlash::sealed_data_offset();
    let sealed_end = offset + header.sealed_data_end();
    let pq_offset = offset + header.pq_signature_offset();

    let mut h: Sha256 = Sha256::new();
    h.update(&file[sealed_start..sealed_end]);
    let digest = h.finalize();
    let msg: &[u8] = digest.as_slice();
    println!("  payload digest (sha256 of protected region): {}", hex::encode(msg));

    // Deterministic internal-path signature (raw message, no context prefix;
    // opt_rand = None makes the randomizer deterministic per FIPS-205). The
    // cached and uncached paths produce bit-identical output, so re-signing the
    // same image with the same key is idempotent.
    let started = Instant::now();
    let sig = match ctx.cache {
        Some(ref cache) => ctx.sk.slh_sign_internal_with_cache(&[msg], None, cache),
        None => ctx.sk.slh_sign_internal(&[msg], None),
    };
    let sig_bytes = sig.to_bytes();
    println!("  signed in {:.1}s", started.elapsed().as_secs_f32());

    if sig_bytes.len() != SIGNATURE_PQ_LENGTH {
        return Err(format!(
            "Internal error: produced a {}-byte signature, but the flash record reserves {}",
            sig_bytes.len(),
            SIGNATURE_PQ_LENGTH
        )
        .into());
    }
    println!("  signature sha256: {}", hex::encode(Sha256::digest(sig_bytes.as_slice())));

    let mut f = OpenOptions::new().read(true).write(true).open(file_path)?;
    f.seek(SeekFrom::Start(pq_offset as u64))?;
    f.write_all(sig_bytes.as_slice())?;
    f.flush()?;
    drop(f);
    println!("  wrote {} bytes at offset {}", SIGNATURE_PQ_LENGTH, pq_offset);

    // End-to-end check against the image as it now sits on disk, using the
    // public key the image itself commits to rather than the one we just signed
    // with. This is the same computation the boot verifier performs.
    verify_pq_signature_in_file(file_path, offset, ctx)?;

    Ok(())
}

fn verify_pq_signature_in_file(
    file_path: &PathBuf,
    offset: usize,
    ctx: &PqContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = fs::read(file_path)
        .map_err(|e| format!("Failed to re-read file '{}': {}", file_path.display(), e))?;
    let mut header = SignatureInFlash::default();
    header.as_mut().copy_from_slice(&file[offset..offset + size_of::<SignatureInFlash>()]);

    let sealed_start = offset + SignatureInFlash::sealed_data_offset();
    let sealed_end = offset + header.sealed_data_end();
    let pq_offset = offset + header.pq_signature_offset();

    let mut h: Sha256 = Sha256::new();
    h.update(&file[sealed_start..sealed_end]);
    let digest = h.finalize();
    let msg: &[u8] = digest.as_slice();

    let sig = PqSignature::<PqParams>::try_from(&file[pq_offset..pq_offset + SIGNATURE_PQ_LENGTH])
        .map_err(|_| "Signature slot does not hold a well-formed SLH-DSA signature")?;

    // Under --pq-force there is no committed key to check against, so fall back
    // to the signing key's own public key.
    let pk_bytes: Vec<u8> = match ctx.slot {
        Some(i) => header.sealed_data.pubkeys_pq[i].pk.to_vec(),
        None => ctx.sk.verifying_key().to_bytes().as_slice().to_vec(),
    };
    let vk =
        PqVerifyingKey::<PqParams>::try_from(pk_bytes.as_slice()).map_err(|_| "Malformed PQ public key")?;
    vk.slh_verify_internal(&[msg], &sig)
        .map_err(|_| "PQ signature verification FAILED against the image's own public key")?;

    match ctx.slot {
        Some(i) => println!("* PQ signature verifies against pubkeys_pq slot {}", i),
        None => println!("* PQ signature verifies against the signing key (NOT committed in the image)"),
    }
    Ok(())
}

// ----- ed25519 header patching ----------------------------------------------

fn patch_signature_in_file(
    file_path: &PathBuf,
    signature: &Vec<u8>,
    auth_data: &Vec<u8>,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open file with read and write permissions
    let mut file = OpenOptions::new().read(true).write(true).open(file_path)?;

    let mut signature_struct = read_header_from_file(&mut file, offset)?;

    signature_struct.signature.copy_from_slice(&signature);
    if auth_data.len() > signature_struct.aad.len() {
        println!(
            "Auth data returned by key is {} bytes and exceeds the capacity {} of the signature record",
            auth_data.len(),
            signature_struct.aad.len()
        );
        return Err(String::from("Auth data exceeds capacity of the signature field").into());
    }
    signature_struct.aad[..auth_data.len()].copy_from_slice(&auth_data);
    signature_struct.aad_len = auth_data.len() as u32;

    write_header_to_file(&mut file, &signature_struct, offset)?;

    Ok(())
}

fn patch_hash_in_file(
    file_path: &PathBuf,
    hash: &[u8],
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open file with read and write permissions
    let mut file = OpenOptions::new().read(true).write(true).open(file_path)?;

    let mut signature_struct = read_header_from_file(&mut file, offset)?;
    signature_struct.sealed_data.toolchain.copy_from_slice(&hash);

    write_header_to_file(&mut file, &signature_struct, offset)?;

    Ok(())
}

fn read_header_from_file(
    file: &mut std::fs::File,
    offset: usize,
) -> Result<SignatureInFlash, Box<dyn std::error::Error>> {
    // Ensure we're at the beginning of the file
    file.seek(SeekFrom::Start(offset as u64))?;

    // Create a buffer for the struct
    let struct_size = mem::size_of::<SignatureInFlash>();
    let mut buffer = vec![0u8; struct_size];

    // Read exactly the size of the struct
    file.read_exact(&mut buffer)?;

    let mut signature_struct = SignatureInFlash::default();
    signature_struct.as_mut().copy_from_slice(&buffer);

    // Check the magic *after* the copy - checking it on the default-initialized
    // struct would only be testing `Default`.
    assert!(
        signature_struct.sealed_data.magic == bao1x_api::signatures::MAGIC_NUMBER,
        "Magic number does not match in bao header file!"
    );

    Ok(signature_struct)
}

fn write_header_to_file(
    file: &mut std::fs::File,
    signature: &SignatureInFlash,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Seek back to the beginning where the struct starts
    file.seek(SeekFrom::Start(offset as u64))?;

    // Write the bytes back to the file
    file.write_all(signature.as_ref())?;

    // Ensure the data is flushed to disk
    file.flush()?;

    Ok(())
}

// ----- code below is vendored in from tools/src/sign_image.rs. Maybe this should be a library,
// but let's see what the e2e integration looks like before we puzzle on how to librarify-this.
pub fn convert_to_uf2<S, T>(
    input: &S,
    output: &T,
    function_code: Option<&str>,
    offset: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsRef<Path>,
    T: AsRef<Path>,
{
    let mut source = vec![];
    let mut source_file = std::fs::File::open(input)?;
    let mut dest_file = std::fs::File::create(output)?;
    source_file.read_to_end(&mut source)?;

    let app_start_addr = match function_code {
        Some("boot0") => bao1x_api::BOOT0_START,
        Some("boot1") => bao1x_api::BOOT1_START,
        Some("loader") => bao1x_api::LOADER_START,
        Some("baremetal") => bao1x_api::BAREMETAL_START,
        Some("kernel") => bao1x_api::KERNEL_START,
        Some("swap") => bao1x_api::SWAP_START_UF2,
        Some("app") => bao1x_api::dabao::APP_RRAM_START,
        _ => return Err(String::from("UF2 Image Requires a function code").into()),
    };

    match bin_to_uf2(
        &source,
        bao1x_api::BAOCHIP_1X_UF2_FAMILY,
        app_start_addr as u32 + offset.unwrap_or(0) as u32,
    ) {
        Ok(u2f_blob) => {
            dest_file.write_all(&u2f_blob)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

use byteorder::{LittleEndian, WriteBytesExt};

const UF2_MAGIC_START0: u32 = 0x0A324655; // "UF2\n"
const UF2_MAGIC_START1: u32 = 0x9E5D5157; // Randomly selected
const UF2_MAGIC_END: u32 = 0x0AB16F30; // Ditto
// Vendored in from https://github.com/sajattack/uf2conv-rs/blob/master/lib/src/lib.rs
// The code is MIT-licensed, and authored by Paul Sajna
pub fn bin_to_uf2(bytes: &Vec<u8>, family_id: u32, app_start_addr: u32) -> Result<Vec<u8>, std::io::Error> {
    let datapadding = [0u8; 512 - 256 - 32 - 4];
    let nblocks: u32 = ((bytes.len() + 255) / 256) as u32;
    let mut outp: Vec<u8> = Vec::new();
    for blockno in 0..nblocks {
        let ptr = 256 * blockno;
        let chunk = match bytes.get(ptr as usize..ptr as usize + 256) {
            Some(bytes) => bytes.to_vec(),
            None => {
                let mut chunk = bytes[ptr as usize..bytes.len()].to_vec();
                while chunk.len() < 256 {
                    chunk.push(0);
                }
                chunk
            }
        };
        let mut flags: u32 = 0;
        if family_id != 0 {
            flags |= 0x2000
        }

        // header
        outp.write_u32::<LittleEndian>(UF2_MAGIC_START0)?;
        outp.write_u32::<LittleEndian>(UF2_MAGIC_START1)?;
        outp.write_u32::<LittleEndian>(flags)?;
        outp.write_u32::<LittleEndian>(ptr + app_start_addr)?;
        outp.write_u32::<LittleEndian>(256)?;
        outp.write_u32::<LittleEndian>(blockno)?;
        outp.write_u32::<LittleEndian>(nblocks)?;
        outp.write_u32::<LittleEndian>(family_id)?;

        // data
        outp.write(&chunk)?;
        outp.write(&datapadding)?;

        // footer
        outp.write_u32::<LittleEndian>(UF2_MAGIC_END)?;
    }
    Ok(outp)
}
