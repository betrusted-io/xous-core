//! Test registry, plus the rules for adding a test.
//!
//! Each theme module owns its own `TESTS` table; this module aggregates them
//! and holds the expected-failure registry `XFAILS`. A test is a `pub fn` that
//! panics to fail and returns to pass; register it in its theme's `TESTS`
//! table and, if it reproduces a known bug, add it to `XFAILS` against the
//! bug's ID, with the defect described in the commit that registers it. Never
//! weaken an assertion to make a test pass — assert the correct behavior and
//! register the XFAIL.
//!
//! xous/PDDB semantics that trip up std::fs habits:
//! - The dict/key separator is `/`, not `:`. `std::path::MAIN_SEPARATOR` on xous is `/`; the server splits
//!   the final dict/key pair on it, so `dict:key` fails every open. `:` is the basis prefix only
//!   (`:basis:dict/key`).
//! - `File::create` does NOT create the parent dict (the client sends `create_path=false`). Use
//!   `TmpDict::new`, which creates its dict first.
//! - Most failures map to `ErrorKind::Other`, not the POSIX-specific kind. Assert `is_err()`; only assert a
//!   specific kind for the `Unsupported` surface.
//! - A loop of more than ~10 fs ops must `log::info!` every ~5 iterations: a successful op emits no output,
//!   and a long silent loop trips the driver's inactivity reaper (which treats console silence as a dead
//!   server).
//!
//! Isolate every test with `TmpDict`, verify each write by reading it back, and
//! clean up what you create (the `persist` theme is the exception — it keeps its
//! data on purpose to verify durability across a restart).

pub mod concur;
pub mod content;
pub mod dirs;
pub mod errors;
pub mod openflags;
pub mod paths;
pub mod persist;
pub mod rw;
pub mod sizes;
pub mod smoke;
pub mod unsupported;

/// (unique `theme::snake_case` name, test fn). A test panics to fail and
/// returns normally to pass.
pub type TestEntry = (&'static str, fn());
/// (test name, bug ID). A listed test that fails reports XFAIL; one that
/// passes reports XPASS and fails the run until it is removed from `XFAILS`.
pub type XfailEntry = (&'static str, &'static str);

/// The known-bug registry.
pub const XFAILS: &[XfailEntry] = &[];

/// Every registered test, aggregated from the theme modules.
pub fn all_tests() -> Vec<TestEntry> {
    let mut v: Vec<TestEntry> = Vec::new();
    v.extend_from_slice(smoke::TESTS);
    v.extend_from_slice(rw::TESTS);
    v.extend_from_slice(openflags::TESTS);
    v.extend_from_slice(dirs::TESTS);
    v.extend_from_slice(errors::TESTS);
    v.extend_from_slice(content::TESTS);
    v.extend_from_slice(unsupported::TESTS);
    v.extend_from_slice(paths::TESTS);
    v.extend_from_slice(sizes::TESTS);
    v.extend_from_slice(concur::TESTS);
    v.extend_from_slice(persist::TESTS);
    v
}
