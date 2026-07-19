//! Minimal SLH-DSA (NIST SP 800-230, SLH-DSA-SHA2-128-24) signing tool.
//!
//!   slh-sign keygen <secret-key-out>
//!       Generate a fresh secret key and write it (raw binary) to <secret-key-out>.
//!       Layout is FIPS-205: sk_seed || sk_prf || pk_seed || pk_root (4*n = 64 bytes).
//!
//!   slh-sign sign <secret-key-in> <message-file> <signature-out> [tree-cache-in]
//!       Read the secret key, read the message file, and write the raw binary
//!       signature (3856 bytes) to <signature-out>. If a tree-cache file is
//!       given, it is validated against the key and used to skip rebuilding
//!       the (message-independent) XMSS tree — ~5x faster signing.
//!
//!   slh-sign cache <secret-key-in> <tree-cache-out>
//!       Build the message-independent XMSS tree cache for the key and write
//!       it to <tree-cache-out> (~128 KiB for SHA2-128-24: the top levels of
//!       the tree; a tiny per-leaf subtree is recomputed at each signature).
//!       One-time cost of one full tree build; every subsequent `sign` with
//!       it skips ~80% of the signing work.
//!
//! Signatures are standard FIPS-205 (empty context) and are deterministic, so
//! signing the same message with the same key twice yields identical bytes.

use std::convert::TryFrom;
use std::fs;
use std::process::exit;

use rand::RngCore; // rand 0.8.5: `fill_bytes`
use slh_dsa::signature::{Keypair, Signer}; // `verifying_key()` / `try_sign`
use slh_dsa::{Sha2_128_24, SigningKey, XmssTreeCache};

/// n (in bytes) for SHA2-128-24. The secret key seed material is 3*n bytes
/// (sk_seed || sk_prf || pk_seed); the serialized key is 4*n (adds pk_root).
const N: usize = 16;

/// The parameter set this tool uses. Swap for another set if desired.
type Params = Sha2_128_24;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("keygen") if args.len() == 3 => keygen(&args[2]),
        Some("sign") if args.len() == 5 => sign(&args[2], &args[3], &args[4], None),
        Some("sign") if args.len() == 6 => sign(&args[2], &args[3], &args[4], Some(&args[5])),
        Some("cache") if args.len() == 4 => cache(&args[2], &args[3]),
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

    // Deterministic, empty-context FIPS-205 signature; the cached and
    // uncached paths produce bit-identical output.
    let sig = match cache_in {
        Some(path) => {
            let bytes = fs::read(path).unwrap_or_else(|e| die(&format!("reading tree cache {path}: {e}")));
            let cache = XmssTreeCache::<Params>::from_bytes(&bytes)
                .unwrap_or_else(|e| die(&format!("parsing tree cache {path}: {e}")));
            if !cache.validate(&sk.verifying_key()) {
                die(&format!("tree cache {path} does not match this key (or is corrupt)"));
            }
            sk.try_sign_with_tree_cache(&msg, &cache).unwrap_or_else(|e| die(&format!("signing failed: {e}")))
        }
        None => sk.try_sign(&msg).unwrap_or_else(|e| die(&format!("signing failed: {e}"))),
    };

    let sig_bytes = sig.to_bytes();
    fs::write(sig_out, sig_bytes.as_slice())
        .unwrap_or_else(|e| die(&format!("writing signature to {sig_out}: {e}")));
    eprintln!("wrote {}-byte signature to {sig_out}", sig_bytes.len());
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
         slh-sign sign <secret-key-in> <message-file> <signature-out> [tree-cache-in]\n  \
         slh-sign cache <secret-key-in> <tree-cache-out>"
    );
    exit(2);
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}
