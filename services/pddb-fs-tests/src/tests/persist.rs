//! Theme `persist`: durability of PDDB content across a full Renode restart.
//!
//! Mechanism: the CI validation sequence is a **cold run** (blank flash,
//! first-boot format) followed by a **warm run** (a brand-new Renode process
//! remounting the SAME flash file). Every test in this theme runs in BOTH
//! processes; this file cannot tell which run it's in except by probing
//! whether last run's data is still there.
//!
//! CONTRACT EXEMPTIONS (both deliberate, both documented here rather than in
//! the general rules):
//! - This theme uses a **fixed** dict name (`pddbtest.persist`, plus a fixed sibling `pddbtest.persist_sub`),
//!   NOT `TmpDict::new`. `TmpDict` mints a process-local counter suffix (`pddbtest.<name>.<n>`), so two
//!   separate Renode *processes* would each start their counter at 0 and silently collide/shadow each other's
//!   namespace instead of sharing it. A fixed name is required so the warm run's process looks at the exact
//!   same on-flash dict the cold run's process wrote.
//! - The data these tests write is **deliberately never cleaned up** (no `remove_file`/`remove_dir` of the
//!   markers or the subdict at the end of a run) -- that's the entire point; removing it would make the next
//!   process's verify pass vacuously instead of proving persistence. The one exception is the `ghost` key
//!   (see `delete_then_persist`), whose CORRECT behavior is precisely to stay deleted.
//!
//! Registry order is load-bearing: VERIFY-tests run first, WRITE-tests run
//! last, in every process. On the cold run, the verify tests see nothing yet
//! (fresh flash) and pass trivially by design; the write tests then lay down
//! the markers for the NEXT process to check. On the warm run, the verify
//! tests check what the cold run's writers left on flash, and the write
//! tests refresh/re-verify within-run and reset the ghost key for the run
//! after that (there is currently only cold->warm in the CI sequence, but the
//! contract holds for any number of chained restarts).
//!
//! Scope note: a same-run reopen-after-drop read (create, drop the handle,
//! reopen, read back -- all inside one process) is already covered
//! extensively elsewhere (e.g. smoke::create_write_read, rw::io_smoke_test).
//! This theme is exclusively about durability ACROSS a full process restart
//! on the same backing flash file; it does not re-test same-run reopen.

use std::fs;
use std::path::Path;

use crate::harness::{XorShift, check};

/// Fixed (not TmpDict) dict holding the two scalar markers and the `ghost` key.
const DICT: &str = "pddbtest.persist";
/// Fixed sibling dict holding the 5-key subdict used by `dict_survives`.
const SUBDICT: &str = "pddbtest.persist_sub";

const MARKER_SMALL_LEN: usize = 100;
const MARKER_MEDIUM_LEN: usize = 2 * 1024; // 2 KiB -- still under the 4 KiB truncate/re-create hazard line
const MARKER_SMALL_SEED: u32 = 0x5EED_0064;
const MARKER_MEDIUM_SEED: u32 = 0x5EED_0800;

const SUB_KEY_COUNT: usize = 5;
const SUB_KEY_LEN: usize = 64;
const SUB_KEY_SEED_BASE: u32 = 0x5EED_5000;

/// Deterministic content generator: same (len, seed) always yields the same
/// bytes, in this process or any other -- this is what makes cross-restart
/// byte-exact comparison possible without persisting the content itself.
fn gen_marker(len: usize, seed: u32) -> Vec<u8> {
    let mut rng = XorShift::new(seed);
    let mut buf = vec![0u8; len];
    rng.fill(&mut buf);
    buf
}

fn small_path() -> String { format!("{}/marker_small", DICT) }
fn medium_path() -> String { format!("{}/marker_medium", DICT) }
fn ghost_path() -> String { format!("{}/ghost", DICT) }
fn sub_path(i: usize) -> String { format!("{}/sub_{}", SUBDICT, i) }

/// VERIFY (runs first): if `marker_small` exists, this is a warm run
/// checking the previous process's writes -- confirm every marker is
/// byte-exact (100 B, 2 KiB, and the 5 subdict keys) and that the `ghost`
/// key, deliberately deleted by `delete_then_persist` last run, stayed
/// deleted. If `marker_small` is absent, this is the cold run (fresh flash):
/// log and pass -- there is nothing to verify yet.
pub fn verify_markers() {
    // The ghost check is unconditional: on a fresh flash it's trivially
    // absent (nothing ever created it), and on a warm run it must have
    // stayed absent since last run's delete_then_persist removed it (PFC-11
    // is about a stale-handle WRITE resurrecting a deleted key within the
    // SAME run/process; a brand-new process has no such stale handle, so a
    // clean disappearance across a restart is exactly the correct contract
    // to assert here).
    assert!(
        !Path::new(&ghost_path()).exists(),
        "ghost key is present at process start -- it should have stayed deleted across the restart"
    );

    if !Path::new(&small_path()).exists() {
        log::info!("persist::verify_markers: fresh flash, nothing to verify");
        return;
    }

    let small = check!(fs::read(small_path()));
    assert_eq!(
        small,
        gen_marker(MARKER_SMALL_LEN, MARKER_SMALL_SEED),
        "marker_small was not byte-exact after restart"
    );

    let medium = check!(fs::read(medium_path()));
    assert_eq!(
        medium,
        gen_marker(MARKER_MEDIUM_LEN, MARKER_MEDIUM_SEED),
        "marker_medium was not byte-exact after restart"
    );

    for i in 0..SUB_KEY_COUNT {
        let content = check!(fs::read(sub_path(i)));
        assert_eq!(
            content,
            gen_marker(SUB_KEY_LEN, SUB_KEY_SEED_BASE + i as u32),
            "sub_{} was not byte-exact after restart",
            i
        );
    }

    log::info!("persistence VERIFIED across restart");
}

/// VERIFY (runs second, before any writer in this process): the subdict
/// doubles as a readdir-after-remount check. On the cold run the subdict
/// doesn't exist yet (Path::exists() is unaffected by PFC-9 -- that bug is
/// specifically about fs::read_dir on a MISSING dict returning an empty Ok
/// instead of erroring, not about Path::exists()), so this logs and passes.
/// On the warm run the subdict must already hold exactly the 5 keys the
/// previous process's write_markers left behind -- neither more nor fewer.
pub fn dict_survives() {
    if !Path::new(SUBDICT).exists() {
        log::info!("persist::dict_survives: fresh flash, subdict absent, nothing to verify");
        return;
    }
    let mut names: Vec<String> = Vec::new();
    for entry in check!(fs::read_dir(SUBDICT)) {
        let entry = check!(entry);
        names.push(entry.file_name().into_string().expect("utf8 filename"));
    }
    assert_eq!(
        names.len(),
        SUB_KEY_COUNT,
        "expected exactly {} entries in {} after restart, found {}: {:?}",
        SUB_KEY_COUNT,
        SUBDICT,
        names.len(),
        names
    );
    log::info!(
        "persistence VERIFIED across restart: {} survived readdir with {} entries",
        SUBDICT,
        names.len()
    );
}

/// WRITE (runs after the verifiers): (re)write every marker deterministically
/// and read each back within this run. Every overwrite here targets a key
/// that (if it exists at all) is well under the 4 KiB large-pool hazard line
/// (100 B / 2 KiB / 64 B), so re-creating it is safe per the SERVER-CRASH
/// HAZARD rule.
pub fn write_markers() {
    // create_dir on an already-existing dict is a (silent, PFC-6) no-op
    // success on this backend, so calling it unconditionally on both the
    // cold and the warm run is safe and idempotent.
    check!(fs::create_dir(DICT));
    check!(fs::create_dir(SUBDICT));

    let small = gen_marker(MARKER_SMALL_LEN, MARKER_SMALL_SEED);
    check!(fs::write(small_path(), &small));
    assert_eq!(check!(fs::read(small_path())), small, "marker_small did not read back intact this run");

    let medium = gen_marker(MARKER_MEDIUM_LEN, MARKER_MEDIUM_SEED);
    assert!(medium.len() < 4096, "marker_medium must stay under the 4 KiB truncate hazard line");
    check!(fs::write(medium_path(), &medium));
    assert_eq!(check!(fs::read(medium_path())), medium, "marker_medium did not read back intact this run");

    for i in 0..SUB_KEY_COUNT {
        let content = gen_marker(SUB_KEY_LEN, SUB_KEY_SEED_BASE + i as u32);
        check!(fs::write(sub_path(i), &content));
        assert_eq!(check!(fs::read(sub_path(i))), content, "sub_{} did not read back intact this run", i);
        log::info!("persist::write_markers: sub_{} written+verified ({}/{})", i, i + 1, SUB_KEY_COUNT);
    }
}

/// WRITE (runs last): create the `ghost` key, verify it within-run, then
/// delete it -- so that the NEXT process's `verify_markers` can assert it
/// stayed deleted. This test only proves same-run delete works; the
/// cross-run half of the contract lives in `verify_markers`'s ghost check.
pub fn delete_then_persist() {
    let path = ghost_path();
    check!(fs::write(&path, b"boo"));
    assert_eq!(check!(fs::read(&path)), b"boo", "ghost key did not read back intact before deletion");
    check!(fs::remove_file(&path));
    assert!(!Path::new(&path).exists(), "ghost key still present immediately after remove_file");
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
/// Order is load-bearing: verifiers first, writers last (see file header).
pub const TESTS: &[(&str, fn())] = &[
    ("persist::verify_markers", verify_markers as fn()),
    ("persist::dict_survives", dict_survives as fn()),
    ("persist::write_markers", write_markers as fn()),
    ("persist::delete_then_persist", delete_then_persist as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[];
