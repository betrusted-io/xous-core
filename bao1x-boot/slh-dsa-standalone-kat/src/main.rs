//! Standalone Known-Answer Test for **SLH-DSA-SHA2-128-24** (NIST SP 800-230 ipd).
//!
//! A single binary with no test harness: build it, copy it to the target, run
//! it, check the exit code (`0` = every check passed, `1` = something failed,
//! `2` = bad usage). Every input is baked into the executable, so it needs no
//! files, no network, no arguments.
//!
//! The vector was produced by the `sphincs/sphincsplus` C reference
//! (branch `bas/fips205`), not by this crate, so a pass cross-validates the
//! Rust implementation against an independent oracle:
//!
//! ```text
//! sk_seed  = 00 01 .. 0f      pk_seed  = 80 81 .. 8f
//! sk_prf   = 40 41 .. 4f      opt_rand = c0 c1 .. cf
//! message  = "NIST SP 800-230 SLH-DSA vector\n"   (31 bytes, empty context)
//! ```
//!
//! # What runs by default
//!
//! Only the fast half: vector integrity, encode/decode round-trips,
//! verification of the reference signature, and a battery of negative cases
//! (tampered message, tampered signature in each region, wrong key, wrong
//! lengths). This takes milliseconds and exercises SHA-256, MGF1, the FORS
//! and WOTS⁺ (w=4) reconstruction, and the 22-level XMSS root walk — i.e.
//! essentially all of the embedded verifier's code path.
//!
//! # What does *not* run by default
//!
//! `--keygen` and `--sign`. For this parameter set d = 1, so keygen builds one
//! XMSS tree of height h' = 22: 2^22 WOTS⁺ public keys, on the order of 10^9
//! hash compressions. That is minutes on a desktop and **hours** on a small
//! embedded target. Signing adds a second full tree walk on top. The negative
//! checks are the important ones for a verifier build; run the slow half on
//! the build host if you want it, or on-target overnight if you really want to
//! pin keygen on the actual silicon.
//!
//! # Usage
//!
//! ```text
//! slh-kat                 verification KATs only (fast, the default)
//! slh-kat --keygen        also derive the key from the seeds and check the public key   [SLOW]
//! slh-kat --sign          also re-sign the message and check all 3856 bytes             [SLOWER]
//! slh-kat --all           everything
//! slh-kat --help
//! ```

use std::env;
use std::process::exit;
use std::time::Instant;

use sha2::{Digest as _, Sha256};
use slh_dsa::signature::Keypair; // verifying_key()
use slh_dsa::{ParameterSet, Sha2_128_24, Signature, SigningKey, VerifyingKey};

#[cfg(feature = "hardened-verify")]
use slh_dsa::HardenedVerifyOutput;

/// The parameter set under test.
type P = Sha2_128_24;

/// n, in bytes, for this set.
const N: usize = 16;
/// Serialized signature length for this set (FIPS-205 §11: 16·(1 + 6·25 + 22 + 68)).
const SIG_BYTES: usize = 3856;
/// Serialized public key length: pk_seed || pk_root.
const PK_BYTES: usize = 2 * N;

// Signature layout, used to place the tampering probes so each region of the
// verifier gets exercised:
//   R          [0, 16)      randomizer
//   FORS       [16, 2416)   k·(1+a)·n = 6·25·16
//   WOTS+      [2416, 3504) len·n = 68·16
//   auth path  [3504, 3856) h'·n = 22·16
const OFF_FORS: usize = N;
const OFF_WOTS: usize = OFF_FORS + 6 * 25 * N;
const OFF_AUTH: usize = OFF_WOTS + 68 * N;

const MSG: &[u8] = b"NIST SP 800-230 SLH-DSA vector\n";

const PK_HEX: &str = "808182838485868788898a8b8c8d8e8fa04ad33acb292af0da32f74d3285b014";
const RANDOMIZER_HEX: &str = "eabbc67b08a221583636efddfe80483c";
const SIG_SHA256_HEX: &str = "10975fa5d31e762cc437eaa13901603c2634453e8d655944d1aac103875c3772";

/// The reference signature, embedded at compile time. Whitespace-tolerant;
/// `unhex` strips it. Keeping it in its own file keeps this source readable —
/// the binary is still self-contained at runtime.
const SIG_HEX: &str = include_str!("../vectors/sig_sphincs-sha2-128-24.hex");

// ---------------------------------------------------------------------------
// tiny helpers (no hex/serde dependency on purpose — fewer moving parts is the
// entire point of this binary)
// ---------------------------------------------------------------------------

fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("bad hex character {c:#04x} in embedded vector"),
    }
}

fn unhex(s: &str) -> Vec<u8> {
    let clean: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    assert!(clean.len() % 2 == 0, "odd-length hex in embedded vector");
    clean
        .chunks(2)
        .map(|p| (nibble(p[0]) << 4) | nibble(p[1]))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 0x00, 0x01, ... — the seed pattern the C driver used.
fn ramp(base: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| base.wrapping_add(i as u8)).collect()
}

// ---------------------------------------------------------------------------
// reporting
// ---------------------------------------------------------------------------

struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn new() -> Self {
        Report {
            passed: 0,
            failed: 0,
        }
    }

    fn check(&mut self, name: &str, ok: bool) {
        if ok {
            self.passed += 1;
            println!("  pass  {name}");
        } else {
            self.failed += 1;
            println!("  FAIL  {name}");
        }
    }

    /// Same as `check`, but also prints a detail line when it fails.
    fn check_eq(&mut self, name: &str, got: &[u8], want: &[u8]) {
        let ok = got == want;
        self.check(name, ok);
        if !ok {
            println!("          got:  {}", hex(&got[..got.len().min(48)]));
            println!("          want: {}", hex(&want[..want.len().min(48)]));
            if got.len() != want.len() {
                println!("          (lengths differ: {} vs {})", got.len(), want.len());
            }
        }
    }

    fn section(&self, title: &str) {
        println!("\n{title}");
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn usage() -> ! {
    eprintln!(
        "\
slh-kat — standalone known-answer test for SLH-DSA-SHA2-128-24

USAGE:
    slh-kat [--keygen] [--sign] [--all]

    (no flags)   verification KATs and negative cases only. Fast.
    --keygen     also derive the keypair from the seeds and check the public
                 key. SLOW: d=1 means one height-22 XMSS tree, ~2^22 WOTS+
                 public keys. Minutes on a desktop, hours on a small target.
    --sign       also re-sign the message and compare all 3856 signature
                 bytes. Implies --keygen; slower still.
    --all        --keygen --sign
    -h, --help   this message

EXIT STATUS
    0  all checks passed
    1  at least one check failed
    2  bad usage"
    );
    exit(2)
}

fn main() {
    let mut do_keygen = false;
    let mut do_sign = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--keygen" => do_keygen = true,
            "--sign" => {
                do_keygen = true;
                do_sign = true;
            }
            "--all" => {
                do_keygen = true;
                do_sign = true;
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("slh-kat: unrecognized argument {other:?}\n");
                usage();
            }
        }
    }

    let mut r = Report::new();

    // -- environment banner ------------------------------------------------
    // Printed first so a failure report carries the build context with it.
    println!("slh-kat — SLH-DSA known-answer test");
    println!("  parameter set (from crate) : {}", <P as ParameterSet>::NAME);
    println!("  pointer width              : {} bits", usize::BITS);
    println!(
        "  target endianness          : {}",
        if cfg!(target_endian = "big") {
            "big"
        } else {
            "little"
        }
    );
    println!(
        "  debug assertions           : {}",
        if cfg!(debug_assertions) {
            "ON (this is a debug build — it will be very slow)"
        } else {
            "off"
        }
    );
    println!(
        "  hardened-verify feature    : {}",
        if cfg!(feature = "hardened-verify") {
            "on"
        } else {
            "off"
        }
    );

    // -- 1. embedded vector integrity --------------------------------------
    // Separates "the vector got mangled in transit" from "the crate computes
    // the wrong answer" — without this, both look like a verification failure.
    r.section("[1] embedded vector integrity");

    let sig_bytes = unhex(SIG_HEX);
    let pk_bytes = unhex(PK_HEX);
    let want_randomizer = unhex(RANDOMIZER_HEX);

    r.check(
        &format!("signature blob is {SIG_BYTES} bytes (got {})", sig_bytes.len()),
        sig_bytes.len() == SIG_BYTES,
    );
    r.check(
        &format!("public key blob is {PK_BYTES} bytes (got {})", pk_bytes.len()),
        pk_bytes.len() == PK_BYTES,
    );

    let digest = {
        let mut h = Sha256::new();
        h.update(&sig_bytes);
        h.finalize()
    };
    r.check_eq(
        "sha256(signature) matches vector sig_digest",
        digest.as_slice(),
        &unhex(SIG_SHA256_HEX),
    );
    r.check_eq(
        "signature starts with the vector's randomizer R",
        &sig_bytes[..N],
        &want_randomizer,
    );
    r.check_eq(
        "public key starts with pk_seed (80..8f)",
        &pk_bytes[..N],
        &ramp(0x80, N),
    );

    if r.failed > 0 {
        println!("\nembedded vector is corrupt — stopping before the crypto checks.");
        finish(&r, "");
    }

    // -- 2. parsing and encoding round-trips -------------------------------
    // Catches a mis-sized SigLen / VkLen or a broken serializer before any
    // failure gets blamed on the hashing.
    r.section("[2] parse / serialize round-trips");

    let vk = match VerifyingKey::<P>::try_from(pk_bytes.as_slice()) {
        Ok(vk) => {
            r.check("VerifyingKey::try_from accepts the 32-byte key", true);
            vk
        }
        Err(_) => {
            r.check("VerifyingKey::try_from accepts the 32-byte key", false);
            println!("\ncannot continue without a parsed key.");
            finish(&r, "");
        }
    };

    let sig = match Signature::<P>::try_from(sig_bytes.as_slice()) {
        Ok(sig) => {
            r.check("Signature::try_from accepts the 3856-byte signature", true);
            sig
        }
        Err(_) => {
            r.check("Signature::try_from accepts the 3856-byte signature", false);
            println!("\ncannot continue without a parsed signature.");
            finish(&r, "");
        }
    };

    r.check_eq(
        "VerifyingKey re-serializes byte-identically",
        vk.to_bytes().as_slice(),
        &pk_bytes,
    );
    r.check_eq(
        "Signature re-serializes byte-identically",
        sig.to_bytes().as_slice(),
        &sig_bytes,
    );

    // Length discipline: a crate whose SigLen disagreed with the standard
    // would happily accept these.
    r.check(
        "Signature::try_from rejects a 3855-byte input",
        Signature::<P>::try_from(&sig_bytes[..SIG_BYTES - 1]).is_err(),
    );
    let mut too_long = sig_bytes.clone();
    too_long.push(0);
    r.check(
        "Signature::try_from rejects a 3857-byte input",
        Signature::<P>::try_from(too_long.as_slice()).is_err(),
    );
    r.check(
        "VerifyingKey::try_from rejects a 31-byte input",
        VerifyingKey::<P>::try_from(&pk_bytes[..PK_BYTES - 1]).is_err(),
    );

    // -- 3. the positive verification KAT ----------------------------------
    // `slh_verify_internal`, not `verify()`: the vector was produced by
    // slh_sign_internal, which does not prepend the FIPS-205 context bytes.
    r.section("[3] verification of the reference signature");

    let t0 = Instant::now();
    let verified = vk.slh_verify_internal(&[MSG], &sig).is_ok();
    let dt = t0.elapsed();
    r.check("slh_verify_internal accepts the C-reference signature", verified);
    println!("          (one verification took {:?})", dt);

    // The context-prefixed API must *reject* it — this pins which convention
    // the signature follows, and catches a build where the two paths got
    // crossed.
    r.check(
        "try_verify_with_context(ctx=[]) rejects it (internal-path signature)",
        vk.try_verify_with_context(MSG, &[], &sig).is_err(),
    );

    // -- 4. negative cases -------------------------------------------------
    // Without these, a verifier stuck at "always Ok" passes section 3.
    r.section("[4] negative cases (each must be rejected)");

    // 4a. tampered message
    let mut m = MSG.to_vec();
    m[0] ^= 0x01;
    r.check(
        "first message byte flipped",
        vk.slh_verify_internal(&[m.as_slice()], &sig).is_err(),
    );

    let mut m = MSG.to_vec();
    m[MSG.len() - 1] ^= 0x01;
    r.check(
        "last message byte flipped",
        vk.slh_verify_internal(&[m.as_slice()], &sig).is_err(),
    );

    r.check(
        "message truncated by one byte",
        vk.slh_verify_internal(&[&MSG[..MSG.len() - 1]], &sig).is_err(),
    );

    let mut m = MSG.to_vec();
    m.push(0x00);
    r.check(
        "message with a trailing NUL appended",
        vk.slh_verify_internal(&[m.as_slice()], &sig).is_err(),
    );

    r.check(
        "empty message",
        vk.slh_verify_internal(&[b"".as_slice()], &sig).is_err(),
    );

    // 4b. tampered signature, one probe per region
    for (label, offset) in [
        ("randomizer R", 0usize),
        ("FORS signature", OFF_FORS),
        ("FORS signature (last byte)", OFF_WOTS - 1),
        ("WOTS+ signature", OFF_WOTS),
        ("WOTS+ signature (last byte)", OFF_AUTH - 1),
        ("XMSS auth path", OFF_AUTH),
        ("XMSS auth path (last byte)", SIG_BYTES - 1),
    ] {
        let mut bad = sig_bytes.clone();
        bad[offset] ^= 0x01;
        let bad_sig = Signature::<P>::try_from(bad.as_slice())
            .expect("a bit-flip must not change the encoded length");
        r.check(
            &format!("bit flipped in {label} (offset {offset})"),
            vk.slh_verify_internal(&[MSG], &bad_sig).is_err(),
        );
    }

    // 4c. wrong key
    let mut bad_pk = pk_bytes.clone();
    bad_pk[0] ^= 0x01; // pk_seed
    let bad_vk = VerifyingKey::<P>::try_from(bad_pk.as_slice()).unwrap();
    r.check(
        "wrong pk_seed",
        bad_vk.slh_verify_internal(&[MSG], &sig).is_err(),
    );

    let mut bad_pk = pk_bytes.clone();
    bad_pk[N] ^= 0x01; // pk_root
    let bad_vk = VerifyingKey::<P>::try_from(bad_pk.as_slice()).unwrap();
    r.check(
        "wrong pk_root",
        bad_vk.slh_verify_internal(&[MSG], &sig).is_err(),
    );

    // -- 5. fault-hardened path --------------------------------------------
    #[cfg(feature = "hardened-verify")]
    {
        r.section("[5] fault-hardened verification path");

        for mask in [0x0000_0000u32, 0xFFFF_FFFF, 0x5A5A_A5A5, 0x0000_0001] {
            let out = vk.slh_verify_hardened(&[MSG], &sig, mask);
            r.check(
                &format!("hardened accept with mask {mask:#010x}"),
                unmask_compare(&out, mask),
            );
        }

        let mut m = MSG.to_vec();
        m[MSG.len() - 1] ^= 0x01;
        let out = vk.slh_verify_hardened(&[m.as_slice()], &sig, 0xC0FF_EE00);
        r.check(
            "hardened rejects a tampered message",
            !unmask_compare(&out, 0xC0FF_EE00),
        );

        // Cross-check against the non-hardened path: both must agree.
        let out = vk.slh_verify_hardened(&[MSG], &sig, 0x1234_5678);
        r.check(
            "hardened and internal paths agree on the good signature",
            unmask_compare(&out, 0x1234_5678) == verified,
        );
    }

    #[cfg(not(feature = "hardened-verify"))]
    {
        r.section("[5] fault-hardened verification path");
        println!("  skip  built without the hardened-verify feature");
    }

    // -- 6. keygen (slow, opt-in) ------------------------------------------
    if !do_keygen {
        r.section("[6] key generation from the known seeds");
        println!("  skip  not requested (pass --keygen)");
        r.section("[7] signing with the known opt_rand");
        println!("  skip  not requested (pass --sign)");
        finish(
            &r,
            "Verification path only: key generation and signing were not exercised.\n             Re-run with --keygen / --sign to cover them (slow: one 2^22-leaf tree per pass).",
        );
    }

    r.section("[6] key generation from the known seeds");
    println!("        building one height-22 XMSS tree (2^22 WOTS+ leaves)...");
    let t0 = Instant::now();
    let sk = SigningKey::<P>::slh_keygen_internal(&ramp(0x00, N), &ramp(0x40, N), &ramp(0x80, N));
    let dt = t0.elapsed();
    println!("        keygen took {dt:?}");

    r.check_eq(
        "slh_keygen_internal reproduces the reference public key",
        sk.verifying_key().to_bytes().as_slice(),
        &pk_bytes,
    );

    // The serialized secret key must be sk_seed || sk_prf || pk_seed || pk_root.
    let skb = sk.to_bytes();
    r.check(
        &format!("serialized signing key is {} bytes (got {})", 4 * N, skb.as_slice().len()),
        skb.as_slice().len() == 4 * N,
    );
    r.check_eq("  ...bytes 0..16 are sk_seed", &skb[..N], &ramp(0x00, N));
    r.check_eq("  ...bytes 16..32 are sk_prf", &skb[N..2 * N], &ramp(0x40, N));
    r.check_eq("  ...bytes 32..64 are the public key", &skb[2 * N..], &pk_bytes);

    // -- 7. signing (slower, opt-in) ---------------------------------------
    if !do_sign {
        r.section("[7] signing with the known opt_rand");
        println!("  skip  not requested (pass --sign)");
        finish(&r, "Signing was not exercised. Re-run with --sign to cover it.");
    }

    r.section("[7] signing with the known opt_rand");
    let t0 = Instant::now();
    let fresh = sk.slh_sign_internal(&[MSG], Some(&ramp(0xC0, N)));
    let dt = t0.elapsed();
    println!("        signing took {dt:?}");

    let fresh_bytes = fresh.to_bytes();
    r.check_eq(
        "randomizer R matches the vector",
        &fresh_bytes[..N],
        &want_randomizer,
    );
    r.check_eq(
        "full 3856-byte signature matches the C reference",
        fresh_bytes.as_slice(),
        &sig_bytes,
    );
    r.check(
        "the freshly produced signature verifies",
        vk.slh_verify_internal(&[MSG], &fresh).is_ok(),
    );

    finish(&r, "");
}

/// Recommended caller-side comparison for the hardened path: unmask and
/// compare one byte at a time, with an independent branch per byte and a
/// loop-completion cross-check, so a single glitch cannot force an accept.
#[cfg(feature = "hardened-verify")]
fn unmask_compare(out: &HardenedVerifyOutput<P>, mask: u32) -> bool {
    let (mr, er) = (out.masked_root(), out.expected_root());
    if mr.len() != er.len() {
        return false;
    }
    let mut matched: usize = 0;
    for i in 0..mr.len() {
        let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8); // 0x00 or 0xFF
        let cand = core::hint::black_box(mr[i]) ^ unmask;
        if cand != er[i] {
            return false;
        }
        matched += 1;
    }
    matched == mr.len()
}

fn finish(r: &Report, note: &str) -> ! {
    println!("\n----------------------------------------");
    if r.failed == 0 {
        println!("ALL CHECKS PASSED  ({} checks)", r.passed);
        if !note.is_empty() {
            println!("{note}");
        }
        exit(0)
    } else {
        println!("FAILED  ({} passed, {} failed)", r.passed, r.failed);
        exit(1)
    }
}
