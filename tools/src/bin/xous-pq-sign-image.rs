//! Minimal SLH-DSA (NIST SP 800-230, SLH-DSA-SHA2-128-24) signing tool.
//!
//!   slh-sign keygen <secret-key-out>
//!       Generate a fresh secret key and write it (raw binary) to <secret-key-out>.
//!       Layout is FIPS-205: sk_seed || sk_prf || pk_seed || pk_root (4*n = 64 bytes).
//!
//!   slh-sign pubkey <secret-key-in> <pubkey-out>
//!       Read the secret key and write the corresponding public (verifying)
//!       key as raw binary to <pubkey-out>. Layout is FIPS-205:
//!       pk_seed || pk_root (2*n = 32 bytes for SHA2-128-24). This is the
//!       trailing 2*n bytes of the secret key: pk_seed and pk_root are both
//!       stored verbatim in the secret key, so no recomputation is needed and
//!       no entropy is drawn. The output matches `VerifyingKey::to_bytes`, i.e.
//!       what the `hardened-verify` boot verifier consumes.
//!
//!   slh-sign sign <secret-key-in> <message-file> <signature-out> [tree-cache-in]
//!       Read the secret key, compute SHA-256 of the message file, and sign
//!       the 32-byte digest (pre-hash / HashSLH-DSA-style flow: the verifier
//!       must likewise hash the payload and verify the digest as the
//!       message). Writes the raw 3856-byte signature to <signature-out>. If
//!       a tree-cache file is given, it is validated against the key and used
//!       to skip rebuilding the (message-independent) XMSS tree — ~5x faster.
//!
//!   slh-sign cache <secret-key-in> <tree-cache-out>
//!       Build the message-independent XMSS tree cache for the key and write
//!       it to <tree-cache-out> (~128 KiB for SHA2-128-24: the top levels of
//!       the tree; a tiny per-leaf subtree is recomputed at each signature).
//!       One-time cost of one full tree build; every subsequent `sign` with
//!       it skips ~80% of the signing work.
//!
//!   slh-sign production-sign <secret-key-in> <image-in> <image-out>
//!                            [tree-cache-in] [--force]
//!       Take a complete bao1x flash image (signature block || payload, as
//!       emitted by the image generator), sign it, and write the image back
//!       out with the PQ signature attached at the offset the boot ROM
//!       expects. See `production_sign` below for the exact layout and the
//!       list of preflight checks. `--force` downgrades the "signing key is
//!       not in this image's PQ public key slots" error to a warning.
//!
//!   slh-sign production-verify <image-in>
//!       Recompute the digest over a signed image and check the trailing PQ
//!       signature against every non-empty `pubkeys_pq` slot in the header.
//!       This is the same computation the boot verifier does, so it is a
//!       useful post-signing QA gate.
//!
//! Signatures use the FIPS-205 *internal* path (`slh_sign_internal`: raw
//! message, no context/domain-separation prefix), matching the KAT vectors and
//! the `hardened-verify` boot verifier. They are deterministic: signing the
//! same message with the same key twice yields identical bytes. NOTE: these
//! signatures are NOT interoperable with standard "pure SLH-DSA" verifiers,
//! which expect the context prefix; this is intentional for the closed
//! signer/verifier system.

// time RUSTFLAGS='--cfg sha2_256_backend="x86_sha"' cargo run --package xous-tools --release --bin
// xous-pq-sign-image production-sign devkey/dev-pq.key target/riscv32imac-unknown-xous-elf/release/swap.img
// swap.img devkey/dev-pq.cache

use std::convert::{TryFrom, TryInto};
use std::fs;
use std::process::exit;

use rand::RngCore; // rand 0.8.5: `fill_bytes`
use sha2::{Digest as _, Sha256};
use slh_dsa::signature::Keypair; // `verifying_key()`
use slh_dsa::{Sha2_128_24, Signature, SigningKey, VerifyingKey, XmssTreeCache};

/// n (in bytes) for SHA2-128-24. The secret key seed material is 3*n bytes
/// (sk_seed || sk_prf || pk_seed); the serialized key is 4*n (adds pk_root).
const N: usize = 16;

/// The parameter set this tool uses. Swap for another set if desired.
type Params = Sha2_128_24;

// ---------------------------------------------------------------------------
// bao1x signature-record layout
// ---------------------------------------------------------------------------
//
// The image generator writes exactly three things, in order:
//
//   [0                      .. UNSIGNED_LEN)   `SignatureInFlash` up to (not
//                                              including) `sealed_data`:
//                                              _jal_instruction || signature ||
//                                              aad_len || aad.  NOT signed.
//   [UNSIGNED_LEN           .. + signed_len)   `protected` = sealed_data ||
//                                              zero padding || payload.
//                                              Covered by BOTH the ed25519
//                                              signature and the PQ signature.
//   [UNSIGNED_LEN+signed_len.. + 3856)         `SignaturePqInFlash`.
//
// `signed_len` is stored in the header as `SIGBLOCK_LEN - UNSIGNED_LEN +
// payload.len()`, so `UNSIGNED_LEN + signed_len` is exactly
// `SignatureInFlash::sealed_data_end()` — the offset the boot ROM uses to find
// the PQ signature. That is the only offset this tool needs, and it is the
// only region it writes.
//
// Rather than hardcode UNSIGNED_LEN (which depends on AAD_LENGTH and would
// silently drift if bao1x-api changed), the header is located by scanning for
// the magic number and backing up one u32 to the `version` field. The result
// is then validated against the version/length invariants below.

/// `SealedFields::version`. Effectively frozen at 0x1_00 by the legacy version check.
const BAOCHIP_SIG_VERSION: u32 = 0x1_00;
/// `SealedFields::magic`, as u32 *values* (stored little-endian in the file).
const MAGIC_NUMBER: [u32; 2] = [u32::from_be_bytes(*b"yumy"), u32::from_be_bytes(*b"Bao3")];

// Byte offsets within `SealedFields` (#[repr(C)], all fields align 4 or 1):
//   version           u32           0..4
//   magic             [u32; 2]      4..12
//   signed_len        u32          12..16
//   function_code     u32          16..20
//   anti_rollback     u32          20..24
//   min_semver        [u8; 16]     24..40
//   semver            [u8; 16]     40..56
//   pubkeys           [Pubkey; 4]  56..200     (4 * 36)
//   toolchain         [u8; 20]    200..220
//   corrected_version u32         220..224
//   pq_enabled        u32         224..228
//   pubkeys_pq        [PubkeyPq;4]228..372     (4 * 36)
const SF_VERSION: usize = 0;
const SF_MAGIC: usize = 4;
const SF_SIGNED_LEN: usize = 12;
const SF_FUNCTION_CODE: usize = 16;
const SF_SEMVER: usize = 40;
const SF_CORRECTED_VERSION: usize = 220;
const SF_PQ_ENABLED: usize = 224;
const SF_PUBKEYS_PQ: usize = 228;
const SEALED_FIELDS_LEN: usize = 372;

/// `PubkeyPq` = pk[PUBLIC_KEY_PQ_LENGTH] || tag[4]
const PUBLIC_KEY_PQ_LENGTH: usize = 32; // 2*n for SHA2-128-24 (pk_seed || pk_root)
const PUBKEY_PQ_ENTRY_LEN: usize = PUBLIC_KEY_PQ_LENGTH + 4;
const PUBKEY_PQ_SLOTS: usize = 4;

/// `SIGNATURE_PQ_LENGTH` for SHA2-128-24. Used for fast preflight checks; the
/// value is re-asserted against the signature actually produced.
const SIGNATURE_PQ_LENGTH: usize = 3856;

/// How far into the image to look for the header magic. The whole signature
/// block is well under this.
const HEADER_SCAN_LIMIT: usize = 8192;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("keygen") if args.len() == 3 => keygen(&args[2]),
        Some("pubkey") if args.len() == 4 => pubkey(&args[2], &args[3]),
        Some("sign") if args.len() == 5 => sign(&args[2], &args[3], &args[4], None),
        Some("sign") if args.len() == 6 => sign(&args[2], &args[3], &args[4], Some(&args[5])),
        Some("cache") if args.len() == 4 => cache(&args[2], &args[3]),
        Some("production-sign") => production_sign(&args[2..]),
        Some("production-verify") if args.len() == 3 => production_verify(&args[2]),
        _ => usage(),
    }
}

fn keygen(sk_out: &str) {
    // Draw 3*n bytes of entropy from the OS CSPRNG using rand 0.8.5, then build
    // the key via `slh_keygen_internal` (raw byte slices). This deliberately
    // avoids `SigningKey::new`, whose `CryptoRng` bound is rand_core 0.10 — so
    // this tool integrates with a rand-0.8.5-pinned suite without version friction.
    let mut seed = [0u8; 3 * N];
    rand::rngs::OsRng.fill_bytes(&mut seed); // 0.8.5 OsRng: infallible
    let sk = SigningKey::<Params>::slh_keygen_internal(&seed[..N], &seed[N..2 * N], &seed[2 * N..3 * N]);

    let bytes = sk.to_bytes();
    fs::write(sk_out, bytes.as_slice())
        .unwrap_or_else(|e| die(&format!("writing secret key to {sk_out}: {e}")));
    eprintln!("wrote {}-byte secret key to {sk_out}", bytes.len());
}

fn pubkey(sk_in: &str, pk_out: &str) {
    // The public key is the FIPS-205 verifying key: pk_seed || pk_root
    // (2*n = 32 bytes for SHA2-128-24). Both halves live verbatim in the last
    // 2*n bytes of the secret key, so `verifying_key()` just hands them back —
    // this is a pure serialization of already-present material, deterministic
    // and side-effect free.
    let sk = load_sk(sk_in);
    let vk = sk.verifying_key();

    let bytes = vk.to_bytes();
    fs::write(pk_out, bytes.as_slice())
        .unwrap_or_else(|e| die(&format!("writing public key to {pk_out}: {e}")));
    eprintln!("wrote {}-byte public key to {pk_out}", bytes.len());
}

fn cache(sk_in: &str, cache_out: &str) {
    let sk = load_sk(sk_in);
    eprintln!(
        "building XMSS tree cache (one full tree build; use --features parallel builds of this tool for speed)..."
    );
    let cache = sk.build_tree_cache();
    fs::write(cache_out, cache.to_bytes())
        .unwrap_or_else(|e| die(&format!("writing tree cache to {cache_out}: {e}")));
    eprintln!("wrote tree cache to {cache_out}");
}

fn sign(sk_in: &str, msg_file: &str, sig_out: &str, cache_in: Option<&str>) {
    let sk = load_sk(sk_in);
    let msg = fs::read(msg_file).unwrap_or_else(|e| die(&format!("reading message {msg_file}: {e}")));
    // Pre-hash: the signed message is SHA-256 of the payload. The verifier
    // hashes the payload the same way (e.g. on a hardware hasher, streamed
    // from flash) and verifies the digest, so the SLH-DSA code never touches
    // the full image.
    let digest = Sha256::digest(&msg);
    let msg: &[u8] = digest.as_slice();

    let sig = sign_digest(&sk, msg, cache_in);

    let sig_bytes = sig.to_bytes();
    fs::write(sig_out, sig_bytes.as_slice())
        .unwrap_or_else(|e| die(&format!("writing signature to {sig_out}: {e}")));
    eprintln!("wrote {}-byte signature to {sig_out}", sig_bytes.len());
}

// ---------------------------------------------------------------------------
// production-sign
// ---------------------------------------------------------------------------

/// Sign a complete, already-assembled bao1x flash image in place.
///
/// The image generator emits `signature block || payload` and then appends the
/// PQ signature. This command performs only that last step, so it can run on a
/// machine that holds the PQ private key without that machine having to
/// rebuild (or even understand) the image.
///
/// It never modifies a single byte inside `[UNSIGNED_LEN, UNSIGNED_LEN +
/// signed_len)`: that region is covered by the ed25519 signature that has
/// already been applied, so any edit there would invalidate the classical
/// signature. In particular `pq_enabled` cannot be flipped on here — if it is
/// zero, the image must be regenerated with PQ enabled.
///
/// Accepted inputs:
///   - an image that ends exactly at the payload end (no PQ signature yet): the signature is appended;
///   - an image that already carries a `SIGNATURE_PQ_LENGTH` tail (a placeholder, or an earlier signature):
///     the tail is overwritten.
///
/// Signing is deterministic, so re-signing an image with the same key is
/// idempotent and byte-reproducible.
fn production_sign(args: &[String]) {
    let mut force = false;
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        match a.as_str() {
            "--force" => force = true,
            s if s.starts_with("--") => die(&format!("unknown option {s}")),
            s => positional.push(s),
        }
    }
    if positional.len() < 3 || positional.len() > 4 {
        usage();
    }
    let (sk_in, image_in, image_out) = (positional[0], positional[1], positional[2]);
    let cache_in = positional.get(3).copied();

    let sk = load_sk(sk_in);
    let vk = sk.verifying_key();
    let vk_bytes = vk.to_bytes();
    if vk_bytes.len() != PUBLIC_KEY_PQ_LENGTH {
        die(&format!(
            "parameter-set mismatch: this key's public key is {} bytes, the header reserves {}",
            vk_bytes.len(),
            PUBLIC_KEY_PQ_LENGTH
        ));
    }

    let img = fs::read(image_in).unwrap_or_else(|e| die(&format!("reading image {image_in}: {e}")));
    let hdr = parse_header(&img, image_in);
    hdr.report();

    // --- preflight ---------------------------------------------------------

    if hdr.pq_enabled == 0 {
        die("pq_enabled is 0 in this image: the boot verifier will ignore any PQ signature.\n       \
             This field lives inside the ed25519-signed region and cannot be patched here.\n       \
             Regenerate the image with the PQ private key supplied to the image generator.");
    }

    // Which key slot are we signing for? The verifier walks pubkeys_pq, so a
    // signature from a key that isn't listed is dead on arrival.
    match hdr.pq_key_slot(vk_bytes.as_slice()) {
        Some(slot) => eprintln!("signing key matches pubkeys_pq slot {slot}"),
        None => {
            let msg = format!(
                "this signing key's public key ({}) is not in any pubkeys_pq slot of {image_in}",
                hex(vk_bytes.as_slice())
            );
            if force {
                eprintln!("warning: {msg} (--force given, continuing anyway)");
            } else {
                die(&format!("{msg}\n       pass --force to sign anyway"));
            }
        }
    }

    let tail = img.len() - hdr.payload_end;
    if tail == 0 {
        eprintln!("image has no PQ signature tail; appending one");
    } else if tail == SIGNATURE_PQ_LENGTH {
        let existing = &img[hdr.payload_end..];
        if existing.iter().all(|&b| b == 0) {
            eprintln!("image has a zeroed {SIGNATURE_PQ_LENGTH}-byte PQ placeholder; filling it in");
        } else {
            eprintln!(
                "note: image already carries a PQ signature (sha256 {}); it will be replaced",
                hex(&Sha256::digest(existing))
            );
        }
    } else {
        die(&format!(
            "unexpected trailing data: {tail} bytes after the signed region (expected 0, or \
             {SIGNATURE_PQ_LENGTH} for a placeholder/previous signature)"
        ));
    }

    // --- sign --------------------------------------------------------------

    // Exactly the digest the image generator computes: SHA-256 over
    // `protected`, i.e. sealed_data || padding || payload.
    let digest = Sha256::digest(&img[hdr.sealed_off..hdr.payload_end]);
    let msg: &[u8] = digest.as_slice();
    eprintln!("payload digest (sha256 of protected region): {}", hex(msg));

    let sig = sign_digest(&sk, msg, cache_in);
    let sig_bytes = sig.to_bytes();
    if sig_bytes.len() != SIGNATURE_PQ_LENGTH {
        die(&format!(
            "internal error: produced a {}-byte signature, but the flash record reserves {}",
            sig_bytes.len(),
            SIGNATURE_PQ_LENGTH
        ));
    }

    // Self-check before anything is written: cheap relative to signing, and it
    // catches a corrupted key file or a mismatched cache before a bad image
    // ever reaches a device.
    vk.slh_verify_internal(&[msg], &sig)
        .unwrap_or_else(|_| die("self-check failed: the signature just produced does not verify"));

    // --- emit --------------------------------------------------------------

    let mut out = Vec::with_capacity(hdr.payload_end + SIGNATURE_PQ_LENGTH);
    out.extend_from_slice(&img[..hdr.payload_end]);
    out.extend_from_slice(sig_bytes.as_slice());

    fs::write(image_out, &out).unwrap_or_else(|e| die(&format!("writing image to {image_out}: {e}")));

    eprintln!("signature sha256: {}", hex(&Sha256::digest(sig_bytes.as_slice())));
    eprintln!("image sha256:     {}", hex(&Sha256::digest(&out)));
    eprintln!(
        "wrote {}-byte signed image to {image_out} ({}-byte PQ signature at offset {})",
        out.len(),
        SIGNATURE_PQ_LENGTH,
        hdr.payload_end
    );
}

/// Recompute the digest over a signed image and verify the trailing PQ
/// signature against the public keys committed in the image's own header —
/// the same check the boot verifier performs.
fn production_verify(image_in: &str) {
    let img = fs::read(image_in).unwrap_or_else(|e| die(&format!("reading image {image_in}: {e}")));
    let hdr = parse_header(&img, image_in);
    hdr.report();

    if img.len() != hdr.payload_end + SIGNATURE_PQ_LENGTH {
        die(&format!(
            "image is {} bytes; expected {} ({} of payload + {SIGNATURE_PQ_LENGTH} of PQ signature)",
            img.len(),
            hdr.payload_end + SIGNATURE_PQ_LENGTH,
            hdr.payload_end
        ));
    }
    if hdr.pq_enabled == 0 {
        eprintln!("warning: pq_enabled is 0 — the boot verifier will not check this signature");
    }

    let sig = Signature::<Params>::try_from(&img[hdr.payload_end..])
        .unwrap_or_else(|_| die("trailing bytes are not a well-formed SLH-DSA signature"));
    let digest = Sha256::digest(&img[hdr.sealed_off..hdr.payload_end]);
    let msg: &[u8] = digest.as_slice();
    eprintln!("payload digest (sha256 of protected region): {}", hex(msg));

    for slot in 0..PUBKEY_PQ_SLOTS {
        let pk = hdr.pq_pubkey(slot);
        if pk.iter().all(|&b| b == 0) {
            eprintln!("slot {slot}: empty");
            continue;
        }
        match VerifyingKey::<Params>::try_from(pk) {
            Ok(vk) => match vk.slh_verify_internal(&[msg], &sig) {
                Ok(()) => {
                    eprintln!("slot {slot}: VALID ({})", hex(pk));
                    eprintln!("signature verifies against pubkeys_pq slot {slot}");
                    return;
                }
                Err(_) => eprintln!("slot {slot}: does not verify ({})", hex(pk)),
            },
            Err(_) => eprintln!("slot {slot}: malformed public key ({})", hex(pk)),
        }
    }
    die("signature does not verify against any public key slot in this image");
}

// ---------------------------------------------------------------------------
// header parsing
// ---------------------------------------------------------------------------

struct Header<'a> {
    img: &'a [u8],
    /// Offset of `SealedFields` — i.e. `UNSIGNED_LEN`, the start of the signed region.
    sealed_off: usize,
    /// End of the signed region == offset of the PQ signature.
    payload_end: usize,
    signed_len: u32,
    version: u32,
    corrected_version: u32,
    pq_enabled: u32,
    function_code: u32,
}

impl<'a> Header<'a> {
    fn pq_pubkey(&self, slot: usize) -> &'a [u8] {
        let off = self.sealed_off + SF_PUBKEYS_PQ + slot * PUBKEY_PQ_ENTRY_LEN;
        &self.img[off..off + PUBLIC_KEY_PQ_LENGTH]
    }

    fn pq_key_slot(&self, pk: &[u8]) -> Option<usize> {
        (0..PUBKEY_PQ_SLOTS).find(|&slot| self.pq_pubkey(slot) == pk)
    }

    fn report(&self) {
        let semver = &self.img[self.sealed_off + SF_SEMVER..self.sealed_off + SF_SEMVER + 16];
        eprintln!("header found at offset {} (UNSIGNED_LEN)", self.sealed_off);
        eprintln!(
            "  version {:#x}  corrected_version {:#010x}  function_code {}",
            self.version, self.corrected_version, self.function_code
        );
        eprintln!(
            "  signed_len {}  signed region [{}, {})  pq_enabled {:#010x}",
            self.signed_len, self.sealed_off, self.payload_end, self.pq_enabled
        );
        eprintln!("  semver {}", hex(semver));
    }
}

/// Locate and validate the signature record. The header is found by scanning
/// for the magic number rather than by hardcoding `UNSIGNED_LEN`, so this stays
/// correct if the unsigned prefix (jal || ed25519 sig || aad_len || aad) ever
/// changes size.
fn parse_header<'a>(img: &'a [u8], name: &str) -> Header<'a> {
    let mut needle = [0u8; 8];
    needle[..4].copy_from_slice(&MAGIC_NUMBER[0].to_le_bytes());
    needle[4..].copy_from_slice(&MAGIC_NUMBER[1].to_le_bytes());

    let limit = img.len().min(HEADER_SCAN_LIMIT);
    let magic_off = (SF_MAGIC..limit.saturating_sub(needle.len() - 1))
        .step_by(4)
        .find(|&off| img[off..off + needle.len()] == needle)
        .unwrap_or_else(|| {
            die(&format!(
                "no bao1x signature record magic found in the first {limit} bytes of {name} — \
                 is this a signed image?"
            ))
        });
    let sealed_off = magic_off - SF_MAGIC;

    if img.len() < sealed_off + SEALED_FIELDS_LEN {
        die(&format!("{name} is truncated: the sealed-fields block runs past the end of the file"));
    }

    let version = rd_u32(img, sealed_off + SF_VERSION);
    if version != BAOCHIP_SIG_VERSION {
        die(&format!(
            "unsupported signature record version {version:#x} (expected {BAOCHIP_SIG_VERSION:#x})"
        ));
    }

    let signed_len = rd_u32(img, sealed_off + SF_SIGNED_LEN);
    let payload_end = sealed_off
        .checked_add(signed_len as usize)
        .unwrap_or_else(|| die("signed_len overflows the address space"));
    if (signed_len as usize) < SEALED_FIELDS_LEN {
        die(&format!(
            "signed_len ({signed_len}) is smaller than the sealed-fields block ({SEALED_FIELDS_LEN})"
        ));
    }
    if payload_end > img.len() {
        die(&format!(
            "signed_len ({signed_len}) runs {} bytes past the end of {name} — truncated image?",
            payload_end - img.len()
        ));
    }

    let corrected_version = rd_u32(img, sealed_off + SF_CORRECTED_VERSION);
    if corrected_version == 0 {
        eprintln!("warning: corrected_version is 0 (legacy record); such images predate the PQ fields");
    }

    Header {
        img,
        sealed_off,
        payload_end,
        signed_len,
        version,
        corrected_version,
        pq_enabled: rd_u32(img, sealed_off + SF_PQ_ENABLED),
        function_code: rd_u32(img, sealed_off + SF_FUNCTION_CODE),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Deterministic internal-path signature (raw message, no context prefix;
/// opt_rand = None makes the randomizer deterministic per FIPS-205). The
/// cached and uncached paths produce bit-identical output.
fn sign_digest(sk: &SigningKey<Params>, msg: &[u8], cache_in: Option<&str>) -> Signature<Params> {
    match cache_in {
        Some(path) => {
            let bytes = fs::read(path).unwrap_or_else(|e| die(&format!("reading tree cache {path}: {e}")));
            let cache = XmssTreeCache::<Params>::from_bytes(&bytes)
                .unwrap_or_else(|e| die(&format!("parsing tree cache {path}: {e}")));
            if !cache.validate(&sk.verifying_key()) {
                die(&format!("tree cache {path} does not match this key (or is corrupt)"));
            }
            sk.slh_sign_internal_with_cache(&[msg], None, &cache)
        }
        None => sk.slh_sign_internal(&[msg], None),
    }
}

fn rd_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().expect("slice is 4 bytes"))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn load_sk(sk_in: &str) -> SigningKey<Params> {
    let sk_bytes = fs::read(sk_in).unwrap_or_else(|e| die(&format!("reading secret key {sk_in}: {e}")));
    SigningKey::<Params>::try_from(sk_bytes.as_slice()).unwrap_or_else(|_| {
        die(&format!(
            "invalid secret key in {sk_in}: expected 64 bytes (4*n for SHA2-128-24), got {}",
            sk_bytes.len()
        ))
    })
}

fn usage() -> ! {
    eprintln!(
        "usage:\n  \
         slh-sign keygen <secret-key-out>\n  \
         slh-sign pubkey <secret-key-in> <pubkey-out>\n  \
         slh-sign sign <secret-key-in> <message-file> <signature-out> [tree-cache-in]\n  \
         slh-sign cache <secret-key-in> <tree-cache-out>\n  \
         slh-sign production-sign <secret-key-in> <image-in> <image-out> [tree-cache-in] [--force]\n  \
         slh-sign production-verify <image-in>"
    );
    exit(2);
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}
