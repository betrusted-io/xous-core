//! Shared plumbing for the pddb-fs-tests suite: per-test namespace isolation,
//! the upstream `check!`/`error_contains!` assertion macros, and a deterministic
//! RNG (OS randomness is unsupported on xous — do not use the `rand` crate).

use std::sync::atomic::{AtomicUsize, Ordering};

static DICT_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A uniquely-named dictionary in the default basis, removed (best-effort) on drop.
/// Every test allocates its namespace through this; see the `tests` module
/// header (services/pddb-fs-tests/src/tests/mod.rs).
pub struct TmpDict {
    dict: String,
}
impl TmpDict {
    pub fn new(name: &str) -> Self {
        let counter = DICT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dict = format!("pddbtest.{}.{}", name, counter);
        // std::fs semantics apply on xous too: the parent "directory" (the
        // dict) must exist before keys can be created in it — File::create
        // does NOT auto-create the dict (empirically verified in the harness
        // pilot; the server only adds a dict when the create_path flag is
        // set, which plain opens never set).
        if let Err(e) = std::fs::create_dir(&dict) {
            panic!("could not create test dict {}: {}", dict, e);
        }
        TmpDict { dict }
    }

    /// Path to `key` inside this dict (default basis). The dict/key join MUST
    /// be '/': std::path::MAIN_SEPARATOR is '/' on xous (SEPARATORS[0] of
    /// ['/', ':'] in the rust fork), and the pddb server splits the final
    /// dict/key pair with rsplit_once(MAIN_SEPARATOR) -- a ':' join makes
    /// every open fail with "no key was specified" (harness pilot, toolchain
    /// 1.96.1.1). ':' is only for basis prefixes (`:basis:dict/key`).
    pub fn path(&self, key: &str) -> String { format!("{}/{}", self.dict, key) }

    /// Get the dict name (path) itself, useful for read_dir and similar dict-level operations.
    pub fn dict(&self) -> &str { &self.dict }
}
impl Drop for TmpDict {
    fn drop(&mut self) {
        // best-effort cleanup: a test that panicked may leave keys open or
        // already-removed; never turn cleanup trouble into a second panic
        let _ = std::fs::remove_dir_all(&self.dict);
    }
}

/// Upstream idiom from rust's library/std/src/fs/tests.rs: unwrap a Result with
/// the failing expression in the panic message.
macro_rules! check {
    ($e:expr) => {
        match $e {
            Ok(t) => t,
            Err(e) => panic!("{} failed with: {e}", stringify!($e)),
        }
    };
}
pub(crate) use check;

/// Upstream idiom from rust's library/std/src/fs/tests.rs: assert that a Result
/// is an Err whose message contains `$s`.
#[allow(unused_macros)]
macro_rules! error_contains {
    ($e:expr, $s:expr) => {
        match $e {
            Ok(_) => panic!("Unexpected success. Should've been: {:?}", $s),
            Err(ref err) => {
                assert!(err.to_string().contains($s), "`{}` did not contain `{}`", err, $s)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use error_contains;

/// Deterministic xorshift32 RNG for generating test data.
pub struct XorShift(u32);
impl XorShift {
    pub fn new(seed: u32) -> Self { XorShift(if seed == 0 { 0xDEAD_BEEF } else { seed }) }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.next_u32() as u8;
        }
    }
}
