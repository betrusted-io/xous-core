//! Parse and verify signed Baochip firmware images (.img files).
//!
//! Reads signed boot images directly, extracts the signature block, computes
//! SHA-512/SHA-256 hashes, and verifies both the classical Ed25519ph/FIDO2
//! signature and the post-quantum SLH-DSA-SHA2-128-24 signature.
//!
//! Image layout produced by `sign_image.rs` (`Version::Bao1xV1`):
//!
//! ```text
//!   [0            .. 132)                unsigned header: jal, sig, aad_len, aad
//!   [132          .. 132 + signed_len)   "protected" / signed region:
//!        [132     .. 504)                  SealedFields  (372 bytes)
//!        [504     .. 768)                  zero padding  (264 bytes)
//!        [768     .. 132 + signed_len)     the firmware payload ("presign" data)
//!   [132+signed_len .. +3856)            SLH-DSA signature, iff pq_enabled != 0
//! ```
//!
//! Classical signature: Ed25519ph over SHA-512(signed region), or FIDO2 form
//! `Ed25519(aad || SHA-256(SHA-512(signed region)))` when `aad_len > 0`.
//!
//! PQ signature: `slh_sign_internal([SHA-256(signed region)], None)` -- i.e. the
//! *internal* (context-free) FIPS-205 entry point over the 32-byte SHA-256
//! digest of the same signed region. Verification must therefore use
//! `slh_verify_internal`, NOT `verify()`, which would prepend a context header.
//!
//! Usage:
//!   verify-binary boot0.img boot1.img
//!   verify-binary boot0.img --output-presign presign.bin
//!   verify-binary boot0.img --output-pq-signature pq.sig
//!   verify-binary boot0.img --json
//!
//! This tool was coded by Claude Opus 4.8-high

use std::fs;
use std::path::PathBuf;
use std::process;

use bytemuck::{Pod, Zeroable};
use clap::Parser;
use digest::{FixedOutput, HashMarker, Output, OutputSizeUser, Reset, Update};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Serialize;
use sha2::{Digest, Sha256, Sha512};
use slh_dsa::{Sha2_128_24, Signature as PqSignature, VerifyingKey as PqVerifyingKey};

/// Parameter set used for Baochip PQ image signing. Must match `type Params`
/// in `signing/sign_image.rs`.
type PqParams = Sha2_128_24;

// ── Constants (mirrors bao1x-api/src/signatures.rs) ─────────────────

const SIGNATURE_LENGTH: usize = 64;
const PUBLIC_KEY_LENGTH: usize = 32;
const AAD_LENGTH: usize = 60;

/// Length of an SLH-DSA-SHA2-128-24 public key (pk_seed || pk_root, 2n = 32).
const PUBLIC_KEY_PQ_LENGTH: usize = 32;
/// Length of an SLH-DSA-SHA2-128-24 signature.
const SIGNATURE_PQ_LENGTH: usize = 3856;

/// Total reserved space for the signature block.
const SIGBLOCK_LEN: usize = 768;

/// Offset where sealed (signed) data begins.
const UNSIGNED_LEN: usize = size_of::<u32>() + SIGNATURE_LENGTH + size_of::<u32>() + AAD_LENGTH; // 132

/// Zero fill between the end of `SealedFields` and the start of the payload.
const PADDING_LEN: usize = SIGBLOCK_LEN - size_of::<SignatureInFlash>();

const MAGIC_NUMBER: [u32; 2] = [u32::from_be_bytes(*b"yumy"), u32::from_be_bytes(*b"Bao3")];

/// Due to an implementation bug, this is effectively a fixed number.
const BAOCHIP_SIG_VERSION: u32 = 0x1_00;

const BAOCHIP_SIG_MAJ_VER: u8 = 0x01;
const BAOCHIP_SIG_MIN_VER: u8 = 0x00;
const BAOCHIP_SIG_REV_VER: u16 = 0x0000;

// Layout guards: if any of these fire, this tool has drifted from
// bao1x-api/src/signatures.rs and MUST be re-synced before it is trusted.
const _: () = assert!(UNSIGNED_LEN == 132);
const _: () = assert!(size_of::<SealedFields>() == 372);
const _: () = assert!(size_of::<SignatureInFlash>() == 504);
const _: () = assert!(PADDING_LEN == 264);
const _: () = assert!(UNSIGNED_LEN + size_of::<SealedFields>() + PADDING_LEN == SIGBLOCK_LEN);

// ── Known public keys ───────────────────────────────────────────────
//
// Transcribed from libs/bao1x-api/src/pubkeys/{bao1,bao2,beta,developer}.rs.
// Slot 3 is always the developer key. Names are cosmetic: identification is by
// key bytes, so slot order does not matter here.

struct KnownKey {
    name: &'static str,
    ed25519: [u8; PUBLIC_KEY_LENGTH],
    slh_dsa: [u8; PUBLIC_KEY_PQ_LENGTH],
}

const KNOWN_KEYS: &[KnownKey] = &[
    KnownKey {
        name: "bao1",
        // bao1.rs :: ID_ED25519_SK_PUB
        ed25519: [
            0xa8, 0x7a, 0x5f, 0x98, 0xda, 0xab, 0xfb, 0x51, 0x2f, 0xc3, 0xc2, 0xe5, 0x74, 0x9b, 0x3b, 0xeb,
            0x19, 0x23, 0x88, 0xd2, 0x01, 0x60, 0xa7, 0xdd, 0x58, 0x88, 0xfb, 0x9d, 0xa4, 0x09, 0x52, 0x3a,
        ],
        // bao1.rs :: SLH_DSA_PUB   (marked "Temporary stand-in" upstream)
        slh_dsa: [
            0xD6, 0x86, 0x93, 0xED, 0x10, 0x3D, 0xD0, 0x43, 0x00, 0x23, 0x4D, 0x2E, 0x5F, 0x37, 0x98, 0xF3,
            0x2A, 0x1B, 0x69, 0xB7, 0x52, 0xF4, 0x15, 0x5F, 0x03, 0x1C, 0x24, 0x72, 0x9A, 0x2B, 0x99, 0xD1,
        ],
    },
    KnownKey {
        name: "bao2",
        // bao2.rs :: ID_ED25519_SK_PUB
        ed25519: [
            0x79, 0x13, 0x5d, 0xc6, 0x67, 0xaf, 0xf4, 0xf7, 0xd3, 0x52, 0xb9, 0x03, 0x28, 0x78, 0x8e, 0xbf,
            0x92, 0xc7, 0x86, 0x78, 0x21, 0x38, 0xb3, 0x77, 0x37, 0x0b, 0x15, 0x19, 0x4e, 0x31, 0x28, 0x88,
        ],
        // bao2.rs :: SLH_DSA_PUB   (marked "Temporary stand-in" upstream)
        slh_dsa: [
            0x97, 0x58, 0x96, 0xA4, 0xB1, 0x8A, 0xC1, 0x1B, 0x03, 0x84, 0x06, 0x8D, 0x5C, 0x1A, 0x70, 0x9D,
            0x9E, 0xE2, 0x6E, 0xB3, 0x97, 0xBA, 0xE6, 0x56, 0x65, 0xBE, 0x4C, 0x74, 0xFF, 0x5C, 0x4B, 0x24,
        ],
    },
    KnownKey {
        name: "beta",
        // beta.rs :: ID_ED25519_SK_PUB
        ed25519: [
            0x80, 0x97, 0x99, 0x29, 0xed, 0xd0, 0x4e, 0x40, 0x12, 0x4b, 0x52, 0xca, 0xe9, 0xae, 0x54, 0xb2,
            0x4b, 0xdf, 0xf7, 0x2a, 0x7b, 0x8a, 0x00, 0x4c, 0x41, 0x06, 0x5b, 0xd1, 0x40, 0x20, 0x78, 0xa7,
        ],
        // beta.rs :: SLH_DSA_PUB
        slh_dsa: [
            0xD7, 0x60, 0x57, 0xC8, 0x61, 0x43, 0x83, 0xF9, 0xF8, 0x7A, 0x9F, 0x79, 0x1A, 0xBF, 0x47, 0xBD,
            0x13, 0x6D, 0xC9, 0x35, 0x44, 0x31, 0xE2, 0x95, 0x66, 0xA5, 0xF9, 0x3F, 0x85, 0xA2, 0x01, 0xFD,
        ],
    },
    KnownKey {
        name: "developer",
        // developer.rs :: PUB
        ed25519: [
            0x1c, 0x9b, 0xea, 0xe3, 0x2a, 0xea, 0xc8, 0x75, 0x07, 0xc1, 0x80, 0x94, 0x38, 0x7e, 0xff, 0x1c,
            0x74, 0x61, 0x42, 0x82, 0xaf, 0xfd, 0x81, 0x52, 0xd8, 0x71, 0x35, 0x2e, 0xdf, 0x3f, 0x58, 0xbb,
        ],
        // developer.rs :: SLH_DSA_PUB
        slh_dsa: [
            0xD7, 0xA6, 0x8F, 0xCD, 0xC5, 0xC4, 0x78, 0xF1, 0x95, 0xD6, 0x52, 0x37, 0x08, 0xF9, 0xC9, 0xA5,
            0x5E, 0xE4, 0xC9, 0x05, 0x37, 0x49, 0x2D, 0xCE, 0x2F, 0x8B, 0xAC, 0x8D, 0x61, 0x83, 0x99, 0x28,
        ],
    },
];

/// The test PQ key baked into `sign_image.rs` (`TEST_KEY_PQ_PUB`), used for
/// images built with `--fake-pubkeys`. Recognised so such builds are labelled
/// rather than reported as an unknown key. Never valid for production.
const TEST_KEY_PQ_PUB: [u8; PUBLIC_KEY_PQ_LENGTH] = [
    0x88, 0xED, 0x70, 0x2A, 0xE1, 0xE6, 0xA1, 0x21, 0x62, 0xED, 0x8E, 0x85, 0xA5, 0x5E, 0x44, 0x8C, 0x14,
    0x26, 0xDC, 0x52, 0xAC, 0x0C, 0xDC, 0xD1, 0x1B, 0x3D, 0x60, 0x1D, 0x37, 0xDE, 0x8C, 0x72,
];

fn identify_ed25519(pk: &[u8; PUBLIC_KEY_LENGTH]) -> Option<&'static str> {
    KNOWN_KEYS.iter().find(|k| &k.ed25519 == pk).map(|k| k.name)
}

fn identify_pq(pk: &[u8; PUBLIC_KEY_PQ_LENGTH]) -> Option<&'static str> {
    if pk == &TEST_KEY_PQ_PUB {
        return Some("TEST KEY (fake-pubkeys build)");
    }
    KNOWN_KEYS.iter().find(|k| &k.slh_dsa == pk).map(|k| k.name)
}

// ── Binary structures (bao1x-api/src/signatures.rs) ─────────────────

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Pubkey {
    pk: [u8; PUBLIC_KEY_LENGTH],
    tag: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct PubkeyPq {
    pk: [u8; PUBLIC_KEY_PQ_LENGTH],
    tag: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct SealedFields {
    version: u32,
    magic: [u32; 2],
    signed_len: u32,
    function_code: u32,
    anti_rollback: u32,
    min_semver: [u8; 16],
    semver: [u8; 16],
    pubkeys: [Pubkey; 4],
    /// Hash of the toolchain used to generate the image (SHA-1 git hash),
    /// injected before the final signature. Often all zeroes.
    toolchain: [u8; 20],
    // ==== end of non-PQ signatures, version 01_00 ====
    // Everything below sat inside the zero padding of pre-PQ images, so it
    // reads back as 0 for those and no special-casing is required.
    /// Real signature-record version; the original `version` check was buggy
    /// and is now frozen at BAOCHIP_SIG_VERSION.
    corrected_version: u32,
    /// Non-zero iff a PQ signature is appended after the signed region.
    pq_enabled: u32,
    pubkeys_pq: [PubkeyPq; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SignatureInFlash {
    _jal_instruction: u32,
    signature: [u8; SIGNATURE_LENGTH],
    /// `0` => pure Ed25519ph. `> 0` => FIDO2/WebAuthn form.
    aad_len: u32,
    aad: [u8; AAD_LENGTH],
    /// All data from this point onward is covered by the signature.
    sealed_data: SealedFields,
}
// Safety: repr(C), all fields are Pod, no padding (verified by the const
// asserts above: 4 + 64 + 4 + 60 = 132, and SealedFields is 4-aligned).
unsafe impl Zeroable for SignatureInFlash {}
unsafe impl Pod for SignatureInFlash {}

fn baochip_sig_compat(incoming: u32) -> bool {
    if (incoming >> 24) as u8 == BAOCHIP_SIG_MAJ_VER {
        BAOCHIP_SIG_MIN_VER > (incoming >> 16) as u8
            || BAOCHIP_SIG_MIN_VER == (incoming >> 16) as u8 && (BAOCHIP_SIG_REV_VER >= incoming as u16)
    } else {
        false
    }
}

// ── PrecomputedHash (from verify_ed25519ph) ──────────────────────

/// Wraps an already-finalized SHA-512 hash so ed25519-dalek's
/// `verify_prehashed` can consume it without re-hashing.
struct PrecomputedHash {
    hash: [u8; 64],
}

impl OutputSizeUser for PrecomputedHash {
    type OutputSize = sha2::digest::typenum::U64;
}

impl FixedOutput for PrecomputedHash {
    fn finalize_into(self, out: &mut Output<Self>) { out.copy_from_slice(&self.hash); }
}

impl Default for PrecomputedHash {
    fn default() -> Self { Self { hash: [0u8; 64] } }
}

impl HashMarker for PrecomputedHash {}

impl Update for PrecomputedHash {
    fn update(&mut self, _data: &[u8]) {}
}

impl Reset for PrecomputedHash {
    fn reset(&mut self) {}
}

// ── RISC-V JAL decoder ──────────────────────────────────────────

/// Decode RISC-V JAL instruction to extract the signed jump offset.
///
/// JAL encoding: imm[20|10:1|11|19:12] rd opcode
fn decode_jal_offset(instruction: u32) -> i32 {
    let imm_20 = (instruction >> 31) & 1;
    let imm_10_1 = (instruction >> 21) & 0x3ff;
    let imm_11 = (instruction >> 20) & 1;
    let imm_19_12 = (instruction >> 12) & 0xff;

    let imm = (imm_20 << 20) | (imm_19_12 << 12) | (imm_11 << 11) | (imm_10_1 << 1);

    // Sign-extend from 21 bits
    if imm & (1 << 20) != 0 { imm as i32 - (1 << 21) } else { imm as i32 }
}

// ── Image analysis ──────────────────────────────────────────────

fn function_code_name(code: u32) -> &'static str {
    match code {
        0 => "Invalid",
        1 => "Boot0",
        2 => "Boot1",
        3 => "UpdatedBoot1",
        4 => "Loader",
        5 => "UpdatedLoader",
        6 => "Baremetal",
        7 => "UpdatedBaremetal",
        0x100 => "Kernel",
        0x101 => "UpdatedKernel",
        0x8000 => "Swap",
        0x8001 => "UpdatedSwap",
        0x10_0000 => "App",
        0x10_0001 => "UpdatedApp",
        _ => "Unknown",
    }
}

/// Partition size in bytes for zero-padding, derived from flash memory layout.
///   BOOT0: 0x6000_0000 .. 0x6002_0000  = 128 KiB
///   BOOT1: 0x6002_0000 .. 0x6006_0000  = 256 KiB
///   LOADER/BAREMETAL: 0x6006_0000 .. 0x600A_0000 = 256 KiB
fn partition_size(function_code: u32) -> Option<usize> {
    match function_code {
        1 => Some(128 * 1024),     // Boot0
        2 | 3 => Some(256 * 1024), // Boot1 / UpdatedBoot1
        4 | 5 => Some(256 * 1024), // Loader / UpdatedLoader
        6 | 7 => Some(256 * 1024), // Baremetal / UpdatedBaremetal
        _ => None,
    }
}

fn semver_str(v: &[u8; 16]) -> String {
    if v.iter().all(|&b| b == 0) { "(unset)".to_string() } else { hex::encode(v) }
}

#[derive(Serialize, Clone)]
#[serde(tag = "result", content = "detail")]
enum Outcome {
    #[serde(rename = "PASSED")]
    Passed,
    #[serde(rename = "FAILED")]
    Failed(String),
    #[serde(rename = "SKIPPED")]
    Skipped(String),
}

impl Outcome {
    fn is_failure(&self) -> bool { matches!(self, Outcome::Failed(_)) }

    fn display(&self) -> String {
        match self {
            Outcome::Passed => "PASSED".to_string(),
            Outcome::Failed(m) => format!("FAILED ({m})"),
            Outcome::Skipped(m) => format!("skipped ({m})"),
        }
    }
}

#[derive(Serialize)]
struct EmbeddedKey {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'static str>,
    tag: String,
    tag_hex: String,
}

#[derive(Serialize)]
struct ImageReport {
    #[serde(rename = "file")]
    filename: String,
    file_size: usize,
    expected_file_size: usize,
    #[serde(rename = "function")]
    function_code: String,
    mode: &'static str,
    signed_len: usize,
    signed_sha512: String,
    /// SHA-256 of the signed region -- this is the message that is PQ-signed.
    signed_sha256: String,
    presign_sha512: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    presign_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    padded_sha512: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    padded_size: Option<usize>,

    #[serde(rename = "signature")]
    signature_hex: String,
    #[serde(rename = "signing_key")]
    signing_key_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_key_name: Option<&'static str>,

    pq_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pq_signature_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pq_signature_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pq_signing_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pq_signing_key_name: Option<&'static str>,

    version: String,
    corrected_version: String,
    record_compatible: bool,
    anti_rollback: u32,
    min_semver: String,
    semver: String,
    toolchain: String,

    #[serde(rename = "embedded_keys")]
    pubkeys: Vec<EmbeddedKey>,
    #[serde(rename = "embedded_keys_pq")]
    pubkeys_pq: Vec<EmbeddedKey>,

    /// Structural sanity findings that do not by themselves fail verification.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,

    classical_verification: Outcome,
    pq_verification: Outcome,
    verification: Outcome,
}

/// Verify the classical Ed25519ph / FIDO2 signature against the embedded keys.
fn verify_classical(sig: &SignatureInFlash, signed_hash: &[u8; 64]) -> (Outcome, Option<[u8; 32]>) {
    let ed25519_signature = Signature::from_bytes(&sig.signature);
    let is_fido2 = sig.aad_len > 0;

    if is_fido2 && sig.aad_len as usize > AAD_LENGTH {
        return (
            Outcome::Failed(format!("aad_len {} exceeds the {AAD_LENGTH}-byte aad field", sig.aad_len)),
            None,
        );
    }

    for key in &sig.sealed_data.pubkeys {
        if key.tag == [0u8; 4] && key.pk == [0u8; PUBLIC_KEY_LENGTH] {
            continue;
        }

        let vk = match VerifyingKey::from_bytes(&key.pk) {
            Ok(vk) => vk,
            Err(_) => continue,
        };

        let result = if is_fido2 {
            // FIDO2: verify(aad[..aad_len] || SHA-256(SHA-512(signed_region)))
            let hashed_hash = Sha256::digest(signed_hash);
            let mut msg = Vec::new();
            msg.extend_from_slice(&sig.aad[..sig.aad_len as usize]);
            msg.extend_from_slice(&hashed_hash);
            vk.verify(&msg, &ed25519_signature)
        } else {
            // Ed25519ph: verify_prehashed with SHA-512 of the signed region
            let prehash = PrecomputedHash { hash: *signed_hash };
            vk.verify_prehashed(prehash, None, &ed25519_signature)
        };

        if result.is_ok() {
            return (Outcome::Passed, Some(key.pk));
        }
    }

    (Outcome::Failed("no embedded key validates this signature".to_string()), None)
}

/// Verify the SLH-DSA-SHA2-128-24 signature against the embedded PQ keys.
///
/// The signer calls `slh_sign_internal(&[sha256(protected)], None)`, so the
/// matching verify entry point is `slh_verify_internal` over the same 32-byte
/// digest. Using `verify()` here would silently fail: it prepends the
/// FIPS-205 context header `0x00 || ctx_len || ctx`.
fn verify_pq(
    sealed: &SealedFields,
    pq_sig_bytes: &[u8],
    signed_sha256: &[u8],
) -> (Outcome, Option<[u8; 32]>) {
    if pq_sig_bytes.len() != SIGNATURE_PQ_LENGTH {
        return (
            Outcome::Failed(format!(
                "PQ signature is {} bytes, expected {SIGNATURE_PQ_LENGTH}",
                pq_sig_bytes.len()
            )),
            None,
        );
    }

    let pq_sig = match PqSignature::<PqParams>::try_from(pq_sig_bytes) {
        Ok(s) => s,
        Err(_) => {
            return (Outcome::Failed("PQ signature failed to decode".to_string()), None);
        }
    };

    let mut candidates = 0usize;
    for key in &sealed.pubkeys_pq {
        if key.tag == [0u8; 4] && key.pk == [0u8; PUBLIC_KEY_PQ_LENGTH] {
            continue;
        }
        candidates += 1;

        let vk = match PqVerifyingKey::<PqParams>::try_from(&key.pk[..]) {
            Ok(vk) => vk,
            Err(_) => continue,
        };

        if vk.slh_verify_internal(&[signed_sha256], &pq_sig).is_ok() {
            return (Outcome::Passed, Some(key.pk));
        }
    }

    if candidates == 0 {
        return (Outcome::Failed("pq_enabled is set but no PQ public keys are embedded".to_string()), None);
    }

    (Outcome::Failed("no embedded PQ key validates this signature".to_string()), None)
}

fn collect_keys(pubkeys: &[Pubkey; 4]) -> Vec<EmbeddedKey> {
    pubkeys
        .iter()
        .filter(|k| k.tag != [0u8; 4] || k.pk != [0u8; PUBLIC_KEY_LENGTH])
        .map(|k| EmbeddedKey {
            key: hex::encode(k.pk),
            name: identify_ed25519(&k.pk),
            tag: String::from_utf8_lossy(&k.tag).trim().to_string(),
            tag_hex: hex::encode(k.tag),
        })
        .collect()
}

fn collect_keys_pq(pubkeys: &[PubkeyPq; 4]) -> Vec<EmbeddedKey> {
    pubkeys
        .iter()
        .filter(|k| k.tag != [0u8; 4] || k.pk != [0u8; PUBLIC_KEY_PQ_LENGTH])
        .map(|k| EmbeddedKey {
            key: hex::encode(k.pk),
            name: identify_pq(&k.pk),
            tag: String::from_utf8_lossy(&k.tag).trim().to_string(),
            tag_hex: hex::encode(k.tag),
        })
        .collect()
}

/// Byte ranges of interest inside an image, derived once and reused for both
/// reporting and the `--output-*` dumps.
struct Regions {
    signed: std::ops::Range<usize>,
    presign: Option<std::ops::Range<usize>>,
    pq_sig: Option<std::ops::Range<usize>>,
}

fn regions(data: &[u8]) -> Result<(SignatureInFlash, Regions), String> {
    if data.len() < size_of::<SignatureInFlash>() {
        return Err(format!(
            "File too small: {} bytes (need at least {})",
            data.len(),
            size_of::<SignatureInFlash>()
        ));
    }

    // pod_read_unaligned rather than from_bytes: a Vec<u8> from fs::read is not
    // guaranteed to satisfy the 4-byte alignment of SignatureInFlash.
    let sig: SignatureInFlash = bytemuck::pod_read_unaligned(&data[..size_of::<SignatureInFlash>()]);

    if sig.sealed_data.magic != MAGIC_NUMBER {
        return Err(format!(
            "Invalid magic: [{:#010x}, {:#010x}], expected [{:#010x}, {:#010x}]",
            sig.sealed_data.magic[0], sig.sealed_data.magic[1], MAGIC_NUMBER[0], MAGIC_NUMBER[1],
        ));
    }

    let signed_len = sig.sealed_data.signed_len as usize;
    let signed_end =
        UNSIGNED_LEN.checked_add(signed_len).ok_or_else(|| format!("signed_len {signed_len} overflows"))?;

    if data.len() < signed_end {
        return Err(format!("File too small for signed region: {} bytes, need {}", data.len(), signed_end,));
    }

    // The payload begins at SIGBLOCK_LEN; everything between the end of
    // SealedFields and there is zero padding that the signer inserted.
    let presign = if signed_end >= SIGBLOCK_LEN { Some(SIGBLOCK_LEN..signed_end) } else { None };

    let pq_sig = if sig.sealed_data.pq_enabled != 0 {
        let end = signed_end + SIGNATURE_PQ_LENGTH;
        if data.len() < end {
            return Err(format!(
                "pq_enabled is set but the file is {} bytes; the {SIGNATURE_PQ_LENGTH}-byte \
                 PQ signature at offset {signed_end} needs {end}",
                data.len()
            ));
        }
        Some(signed_end..end)
    } else {
        None
    };

    Ok((sig, Regions { signed: UNSIGNED_LEN..signed_end, presign, pq_sig }))
}

fn process_image(data: &[u8], filename: &str) -> Result<ImageReport, String> {
    let (sig, r) = regions(data)?;
    let sealed = &sig.sealed_data;
    let mut warnings: Vec<String> = Vec::new();

    let is_fido2 = sig.aad_len > 0;
    let mode = if is_fido2 { "FIDO2" } else { "Ed25519ph" };

    let signed_region = &data[r.signed.clone()];
    // copy_from_slice rather than `.into()`: the GenericArray -> [u8; 64]
    // conversion needs generic-array's `more_lengths` feature, which we do not
    // want to depend on being switched on elsewhere in the graph.
    let mut signed_hash = [0u8; 64];
    signed_hash.copy_from_slice(&Sha512::digest(signed_region));
    let mut signed_sha256 = [0u8; 32];
    signed_sha256.copy_from_slice(&Sha256::digest(signed_region));

    // ── structural sanity ──
    let jal_offset = decode_jal_offset(sig._jal_instruction);
    if jal_offset != SIGBLOCK_LEN as i32 {
        warnings.push(format!("JAL offset is {jal_offset}, expected {SIGBLOCK_LEN} (SIGBLOCK_LEN)"));
    }
    if sealed.version != BAOCHIP_SIG_VERSION {
        warnings.push(format!("version is {:#x}, expected {BAOCHIP_SIG_VERSION:#x}", sealed.version));
    }
    // The gap between SealedFields and the payload must be all zeroes; the
    // pre-PQ fields of this struct live inside that gap for legacy images, so
    // this check also confirms that reading them back as 0 is meaningful.
    if data.len() >= SIGBLOCK_LEN {
        let pad = &data[UNSIGNED_LEN + size_of::<SealedFields>()..SIGBLOCK_LEN];
        if pad.iter().any(|&b| b != 0) {
            warnings.push("padding between the sealed fields and the payload is not zero".to_string());
        }
    } else {
        warnings.push(format!(
            "image is {} bytes, shorter than the {SIGBLOCK_LEN}-byte signature block",
            data.len()
        ));
    }

    let expected_file_size = r.signed.end + if r.pq_sig.is_some() { SIGNATURE_PQ_LENGTH } else { 0 };
    if data.len() > expected_file_size {
        warnings
            .push(format!("{} trailing bytes past the end of the image", data.len() - expected_file_size));
    }

    // ── hashes ──
    let (presign_sha512, presign_size) = match &r.presign {
        Some(range) => (hex::encode(Sha512::digest(&data[range.clone()])), Some(range.len())),
        None => (format!("(signed region ends at {}, before SIGBLOCK_LEN)", r.signed.end), None),
    };

    let (padded_sha512, padded_size) = match partition_size(sealed.function_code) {
        Some(target) if data.len() <= target => {
            let mut padded = data.to_vec();
            padded.resize(target, 0);
            (Some(hex::encode(Sha512::digest(&padded))), Some(target))
        }
        Some(target) => {
            warnings.push(format!("image is {} bytes but its partition is only {target} bytes", data.len()));
            (None, None)
        }
        None => (None, None),
    };

    // ── verification ──
    let (classical, classical_key) = verify_classical(&sig, &signed_hash);

    let (pq, pq_key, pq_sig_sha256) = match &r.pq_sig {
        Some(range) => {
            let bytes = &data[range.clone()];
            let (outcome, key) = verify_pq(sealed, bytes, &signed_sha256);
            (outcome, key, Some(hex::encode(Sha256::digest(bytes))))
        }
        None => (Outcome::Skipped("pq_enabled is 0; no PQ signature present".to_string()), None, None),
    };

    let overall = if classical.is_failure() || pq.is_failure() {
        let mut reasons = Vec::new();
        if let Outcome::Failed(m) = &classical {
            reasons.push(format!("classical: {m}"));
        }
        if let Outcome::Failed(m) = &pq {
            reasons.push(format!("pq: {m}"));
        }
        Outcome::Failed(reasons.join("; "))
    } else {
        Outcome::Passed
    };

    let pubkeys = collect_keys(&sealed.pubkeys);
    let pubkeys_pq = collect_keys_pq(&sealed.pubkeys_pq);

    // Fall back to the first embedded key for display when nothing validated.
    let (signing_key_hex, signing_key_name) = match classical_key {
        Some(pk) => (hex::encode(pk), identify_ed25519(&pk)),
        None => match pubkeys.first() {
            Some(ek) => (ek.key.clone(), ek.name),
            None => (String::new(), None),
        },
    };

    Ok(ImageReport {
        filename: filename.to_string(),
        file_size: data.len(),
        expected_file_size,
        function_code: function_code_name(sealed.function_code).to_string(),
        mode,
        signed_len: r.signed.len(),
        signed_sha512: hex::encode(signed_hash),
        signed_sha256: hex::encode(signed_sha256),
        presign_sha512,
        presign_size,
        padded_sha512,
        padded_size,
        signature_hex: hex::encode(sig.signature),
        signing_key_hex,
        signing_key_name,
        pq_enabled: sealed.pq_enabled != 0,
        pq_signature_offset: r.pq_sig.as_ref().map(|r| r.start),
        pq_signature_sha256: pq_sig_sha256,
        pq_signing_key_hex: pq_key.map(hex::encode),
        pq_signing_key_name: pq_key.and_then(|pk| identify_pq(&pk)),
        version: format!("{:#x}", sealed.version),
        corrected_version: format!("{:#010x}", sealed.corrected_version),
        record_compatible: sealed.version == BAOCHIP_SIG_VERSION
            && (sealed.corrected_version == 0 || baochip_sig_compat(sealed.corrected_version)),
        anti_rollback: sealed.anti_rollback,
        min_semver: semver_str(&sealed.min_semver),
        semver: semver_str(&sealed.semver),
        toolchain: hex::encode(sealed.toolchain),
        pubkeys,
        pubkeys_pq,
        warnings,
        classical_verification: classical,
        pq_verification: pq,
        verification: overall,
    })
}

fn print_report(r: &ImageReport) {
    let key_display = |hexstr: &str, name: Option<&'static str>| match name {
        Some(n) => format!("{hexstr} ({n})"),
        None => hexstr.to_string(),
    };

    println!("File:            {} ({} bytes)", r.filename, r.file_size);
    println!("Function:        {}", r.function_code);
    println!("Mode:            {}{}", r.mode, if r.pq_enabled { " + SLH-DSA-SHA2-128-24" } else { "" });
    println!("Signed length:   {} bytes", r.signed_len);
    println!("Signed SHA512:   {}", r.signed_sha512);
    println!("Signed SHA256:   {}   <- PQ signed message", r.signed_sha256);
    match r.presign_size {
        Some(n) => println!("Presign SHA512:  {} ({} bytes)", r.presign_sha512, n),
        None => println!("Presign SHA512:  {}", r.presign_sha512),
    }
    if let (Some(hash), Some(size)) = (&r.padded_sha512, r.padded_size) {
        println!("Padded SHA512:   {hash} (zero-padded to {size} bytes)");
    }
    println!("Signature:       {}", r.signature_hex);
    println!("Signing Key:     {}", key_display(&r.signing_key_hex, r.signing_key_name));
    if r.pq_enabled {
        if let Some(off) = r.pq_signature_offset {
            println!("PQ Signature:    {SIGNATURE_PQ_LENGTH} bytes @ offset {off}");
        }
        if let Some(h) = &r.pq_signature_sha256 {
            println!("  sha256:        {h}");
        }
        match &r.pq_signing_key_hex {
            Some(k) => println!("PQ Signing Key:  {}", key_display(k, r.pq_signing_key_name)),
            None => println!("PQ Signing Key:  (none matched)"),
        }
    }
    println!("Version:         {} / corrected {}", r.version, r.corrected_version);
    println!("Compatible:      {}", if r.record_compatible { "yes" } else { "NO" });
    println!("Anti-rollback:   {}", r.anti_rollback);
    println!("SemVer:          {} (min {})", r.semver, r.min_semver);
    println!("Toolchain:       {}", r.toolchain);

    if !r.pubkeys.is_empty() {
        println!("Embedded keys (ed25519):");
        for ek in &r.pubkeys {
            match ek.name {
                Some(n) => println!("  {} ({}) [{}]", ek.key, n, ek.tag_hex),
                None => println!("  {} [{}] <- UNKNOWN", ek.key, ek.tag_hex),
            }
        }
    }
    if !r.pubkeys_pq.is_empty() {
        println!("Embedded keys (SLH-DSA):");
        for ek in &r.pubkeys_pq {
            match ek.name {
                Some(n) => println!("  {} ({}) [{}]", ek.key, n, ek.tag_hex),
                None => println!("  {} [{}] <- UNKNOWN", ek.key, ek.tag_hex),
            }
        }
    }

    for w in &r.warnings {
        println!("Warning:         {w}");
    }

    println!("Classical:       {}", r.classical_verification.display());
    println!("Post-quantum:    {}", r.pq_verification.display());
    println!("Verification:    {}", r.verification.display());
}

// ── CLI ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "verify-binary")]
#[command(about = "Parse and verify signed Baochip firmware images")]
struct Args {
    /// Signed .img files to analyze
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Save the extracted pre-signing payload (only for a single file)
    #[arg(short, long)]
    output_presign: Option<PathBuf>,

    /// Save the extracted SLH-DSA signature (only for a single file)
    #[arg(long)]
    output_pq_signature: Option<PathBuf>,

    /// Require a valid PQ signature; images without one are treated as failures
    #[arg(long)]
    require_pq: bool,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

fn dump(path: &PathBuf, bytes: &[u8], label: &str, quiet: bool) {
    match fs::write(path, bytes) {
        Ok(()) => {
            if !quiet {
                println!("{label}: {} ({} bytes)", path.display(), bytes.len());
            }
        }
        Err(e) => eprintln!("Error writing {}: {e}", path.display()),
    }
}

fn main() {
    let args = Args::parse();

    let wants_dump = args.output_presign.is_some() || args.output_pq_signature.is_some();
    if wants_dump && args.files.len() > 1 {
        eprintln!("Error: --output-* options only work with a single input file");
        process::exit(1);
    }

    let mut all_passed = true;
    let mut reports: Vec<ImageReport> = Vec::new();

    for (i, path) in args.files.iter().enumerate() {
        if !args.json && i > 0 {
            println!();
        }

        let data = match fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error reading {}: {}", path.display(), e);
                all_passed = false;
                continue;
            }
        };

        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        match process_image(&data, &filename) {
            Ok(report) => {
                if report.verification.is_failure() {
                    all_passed = false;
                }
                if args.require_pq && !report.pq_enabled {
                    all_passed = false;
                    if !args.json {
                        eprintln!("Error: {filename} has no PQ signature and --require-pq is set");
                    }
                }

                if !args.json {
                    print_report(&report);
                }

                if wants_dump {
                    // Recompute the ranges for the dumps; process_image already
                    // proved they are in bounds.
                    if let Ok((_, r)) = regions(&data) {
                        if let Some(out) = &args.output_presign {
                            match &r.presign {
                                Some(range) => dump(out, &data[range.clone()], "Presign data", args.json),
                                None => eprintln!("No presign region in {filename}"),
                            }
                        }
                        if let Some(out) = &args.output_pq_signature {
                            match &r.pq_sig {
                                Some(range) => dump(out, &data[range.clone()], "PQ signature", args.json),
                                None => eprintln!("No PQ signature in {filename}"),
                            }
                        }
                    }
                }

                if args.json {
                    reports.push(report);
                }
            }
            Err(e) => {
                eprintln!("Error processing {}: {}", filename, e);
                all_passed = false;
            }
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
    }

    process::exit(if all_passed { 0 } else { 1 });
}
