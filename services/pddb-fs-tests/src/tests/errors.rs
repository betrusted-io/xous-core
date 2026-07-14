//! THEME errors: error paths and boundary conditions.
//!
//! All assertions state the CORRECT (or, where it is a documented xous
//! semantic rather than a bug, the documented) behavior; known bugs are pinned
//! in this theme's XFAILS table -- never weaken an assertion.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};

use crate::harness::{TmpDict, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// `File::open` on a dict/key path that was never created must fail. Prefer
/// `is_err()` here: the xous backend maps this to `ErrorKind::Other`, not
/// `NotFound` (see `open_missing_kind_characterization` below for the
/// specific-kind pin).
pub fn open_missing_is_err() {
    let tmp = TmpDict::new("open_missing_is_err");
    let path = tmp.path("never_created");
    assert!(File::open(&path).is_err(), "File::open on a never-created key unexpectedly succeeded");
}

/// Characterization, not a bug: xous collapses nearly all fs failures --
/// including open-on-missing-key -- to `ErrorKind::Other` rather than
/// `NotFound`. There is no BasisLost-specific
/// retcode-to-ErrorKind mapping upstream (services/pddb/src/libstd/mod.rs
/// `open_key` returns `PddbRetcode::BasisLost` when the dict/key doesn't
/// exist in any basis). Pinned here so a future retcode-mapping change shows
/// up as a loud XPASS-shaped surprise rather than silently drifting.
pub fn open_missing_kind_characterization() {
    let tmp = TmpDict::new("open_missing_kind_characterization");
    let path = tmp.path("never_created");
    let err = File::open(&path).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::Other,
        "expected xous's characteristic Other kind for a missing key, got {:?}",
        err.kind()
    );
}

/// `fs::remove_file` on a path whose key was never created must fail.
pub fn remove_missing_path() {
    let tmp = TmpDict::new("remove_missing_path");
    let path = tmp.path("never_created");
    assert!(fs::remove_file(&path).is_err(), "remove_file on a never-created key unexpectedly succeeded");
}

/// Removing the same key twice: the second `remove_file` must fail.
pub fn remove_file_twice() {
    let tmp = TmpDict::new("remove_file_twice");
    let path = tmp.path("gone");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"bye"));
    }
    assert_eq!(&read_back(&path)[..], b"bye");
    check!(fs::remove_file(&path));
    assert!(fs::remove_file(&path).is_err(), "second remove_file of the same key unexpectedly succeeded");
}

/// `fs::remove_dir` given a key (file) path must fail. The server does not
/// split a `remove_dir` path on '/' the way `open`/`remove_file` split their
/// final dict/key component (services/pddb/src/libstd/mod.rs `delete_dict`
/// takes the whole remainder as a single dict name); a `dict/key`-shaped
/// argument is therefore looked up as one nonexistent dict literally named
/// "dict/key" and errors on that basis. Either way the API-level contract --
/// remove_dir on a non-directory target must fail -- holds, so `is_err()` is
/// what we assert.
pub fn remove_dir_on_file() {
    let tmp = TmpDict::new("remove_dir_on_file");
    let path = tmp.path("a_file");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"x"));
    }
    assert_eq!(&read_back(&path)[..], b"x");
    assert!(fs::remove_dir(&path).is_err(), "remove_dir on a file (key) path unexpectedly succeeded");
    check!(fs::remove_file(&path));
}

/// `fs::remove_file` given a bare dict (directory) path -- no key component,
/// hence no '/' to split on -- must fail (services/pddb/src/libstd/mod.rs
/// `delete_key` requires `path.rsplit_once('/')` to succeed).
pub fn remove_file_on_dir() {
    let tmp = TmpDict::new("remove_file_on_dir");
    assert!(fs::remove_file(tmp.dict()).is_err(), "remove_file on a bare dict path unexpectedly succeeded");
}

/// `fs::metadata` of a path whose key was never created must fail.
pub fn stat_missing_path() {
    let tmp = TmpDict::new("stat_missing_path");
    let path = tmp.path("never_created");
    assert!(fs::metadata(&path).is_err(), "metadata on a never-created key unexpectedly succeeded");
}

/// `File::open("")` must fail: an empty path has no dict component at all, so
/// the server can never split off a key (services/pddb/src/libstd/mod.rs
/// `open_key`: `rsplit_once('/')` on an empty string is `None`).
pub fn open_empty_path() {
    assert!(File::open("").is_err(), "File::open(\"\") unexpectedly succeeded");
}

/// Dict-name length boundary. `DictName`'s on-disk struct is `{ len: u8, data:
/// [u8; DICT_NAME_LEN - 1] }` (services/pddb/src/backend/dictionary.rs);
/// `DICT_NAME_LEN` itself (111, services/pddb/src/api.rs) is the STRUCT size
/// *including* the length byte, so the longest usable dict name is
/// `DICT_NAME_LEN - 1` = 110 bytes -- `DictName::try_from_str` errors past
/// that (called synchronously from `dict_add` -> `dict_sync` on every
/// `create_dir`, so the error surfaces immediately, not asynchronously).
///
/// NOTE for the registry maintainer: the documented dict-name limit of
/// "<= 111 bytes" reads api.rs's `DICT_NAME_LEN` constant alone
/// without the backend struct's reserved length byte. This test asserts the
/// verified real boundary (110 ok / 111 errors) instead of weakening to match
/// the doc; flagged as an open question to reconcile the doc.
///
/// This needs a bare top-level dict name of an exact byte length -- `TmpDict`
/// always appends an unpredictable `.<counter>` suffix (`harness.rs`) -- so it
/// manages its own dict directly, with a `pddbtest.`-prefixed name for
/// identifiability and a defensive `remove_dir_all` up front in case a prior
/// aborted run left it behind, cleaning up as it goes (mirroring TmpDict's
/// Drop for the parts TmpDict itself can't be used for).
pub fn dict_name_length_boundary() {
    const DICT_MAX: usize = 110;
    let prefix = "pddbtest.errors_raw.dictlen.";
    assert!(prefix.len() < DICT_MAX, "test prefix grew past the boundary budget");
    let ok_name = format!("{prefix}{}", "x".repeat(DICT_MAX - prefix.len()));
    assert_eq!(ok_name.len(), DICT_MAX);
    let over_name = format!("{ok_name}x");
    assert_eq!(over_name.len(), DICT_MAX + 1);

    let _ = fs::remove_dir_all(&ok_name);
    let _ = fs::remove_dir_all(&over_name);

    // At the limit: create_dir + a key inside it must work normally.
    check!(fs::create_dir(&ok_name));
    let key_path = format!("{ok_name}/k");
    {
        let mut f = check!(File::create(&key_path));
        check!(f.write_all(b"ok"));
    }
    assert_eq!(&read_back(&key_path)[..], b"ok");
    check!(fs::remove_file(&key_path));
    check!(fs::remove_dir(&ok_name));

    // One byte over: must error cleanly (POSIX ENAMETOOLONG territory).
    // XFAIL PFC-6: the client's `DirBuilder::mkdir` (rust fork
    // sys/fs/xous.rs) never reads the server's reply retcode at all, so the
    // server-side InternalError from `DictName::try_from_str` is swallowed
    // and create_dir returns Ok. Same client bug as the already-exists case
    // pinned by dirs::mkdir_path_already_exists_error.
    let over_res = fs::create_dir(&over_name);
    // Cleanup BEFORE the assert (a failing assert must not strand state):
    // `dict_add` inserts the dict into the in-memory basis cache and bumps
    // num_dicts *before* the dict_sync that validates the name length
    // (services/pddb/src/backend/basis.rs ~362-365 vs ~1771), so the
    // rejected dict lingers poisoned in RAM (PFC-8 territory) unless it is
    // explicitly removed -- and a stranded entry drifts the basis's
    // num_dicts accounting for the rest of the boot, which the offline
    // pddbdbg audit flags as an ERROR.
    let _ = fs::remove_dir_all(&over_name);
    assert!(over_res.is_err(), "create_dir with a {}-byte dict name unexpectedly succeeded", over_name.len());

    // Verify the PDDB survives: a completely normal create/read in a fresh,
    // TmpDict-managed namespace.
    let tmp = TmpDict::new("dict_name_length_boundary_survives");
    let path = tmp.path("alive");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"still alive"));
    }
    assert_eq!(&read_back(&path)[..], b"still alive");
    check!(fs::remove_file(&path));
}

/// Key-name length boundary. `KeyName`'s on-disk struct is `{ len: u8, data:
/// [u8; KEY_NAME_LEN - 1] }` (services/pddb/src/backend/key.rs);
/// `KEY_NAME_LEN` itself (95, services/pddb/src/api.rs) is again the STRUCT
/// size including the length byte, so the longest usable key name is
/// `KEY_NAME_LEN - 1` = 94 bytes -- `KeyName::try_from_str` errors past that
/// (called synchronously off the `dict_sync` that every `key_update` --
/// including the create-file path -- triggers). Same off-by-one relative to
/// the documented "<= 95" as `dict_name_length_boundary` above.
///
/// XFAIL PFC-8: `key_update`'s new-key path inserts the KeyCacheEntry and
/// bumps key_count (backend/dictionary.rs ~1001/~1030) *before* the
/// `dict_sync` whose `KeyName::try_from_str` (backend/basis.rs ~1848)
/// rejects the over-length name, so the rejected key stays poisoned --
/// valid+dirty -- in the dict's key cache, and EVERY later `dict_sync` of
/// this dict re-fails on it. The over-length File::create itself correctly
/// errors (open checks the retcode), but the follow-up "alive" create in
/// the same dict then fails too, which is what this test pins. TmpDict's
/// Drop (remove_dir_all: per-key unlink, then rmdir) clears the poisoned
/// entry, so the damage does not outlive the test.
pub fn key_name_length_boundary() {
    const KEY_MAX: usize = 94;
    let tmp = TmpDict::new("key_name_length_boundary");
    let ok_key = "k".repeat(KEY_MAX);
    let over_key = "k".repeat(KEY_MAX + 1);
    let ok_path = tmp.path(&ok_key);
    let over_path = tmp.path(&over_key);

    // At the limit: create + write + read-back must work normally.
    {
        let mut f = check!(File::create(&ok_path));
        check!(f.write_all(b"boundary"));
    }
    assert_eq!(&read_back(&ok_path)[..], b"boundary");
    check!(fs::remove_file(&ok_path));

    // One byte over: must error cleanly.
    assert!(
        File::create(&over_path).is_err(),
        "File::create with a {}-byte key name unexpectedly succeeded",
        over_key.len()
    );

    // Verify the PDDB survives: another normal create/read in the same dict.
    let alive_path = tmp.path("alive");
    {
        let mut f = check!(File::create(&alive_path));
        check!(f.write_all(b"still alive"));
    }
    assert_eq!(&read_back(&alive_path)[..], b"still alive");
    check!(fs::remove_file(&alive_path));
}

/// `fs::metadata(path).len()` characterization. POSIX-correct behavior is
/// that the returned length matches the actual content length; xous instead
/// always reports 0 (PFC-5: services/pddb/src/libstd/mod.rs `stat_path`
/// writes a placeholder `0u64` length unconditionally). Assert the CORRECT
/// length and register the XFAIL -- never assert the buggy `0`.
pub fn metadata_len_characterization() {
    let tmp = TmpDict::new("metadata_len_characterization");
    let path = tmp.path("sized");
    let content = b"0123456789abcdef";
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(content));
    }
    assert_eq!(&read_back(&path)[..], &content[..]);
    let md = check!(fs::metadata(&path));
    assert_eq!(
        md.len(),
        content.len() as u64,
        "PFC-5: fs::metadata(..).len() should report the actual content length"
    );
    check!(fs::remove_file(&path));
}

/// Delete-while-open. services/pddb/src/libstd/mod.rs `delete_key` is
/// SUPPOSED to mark every currently-open `FileHandle` for the removed
/// dict/key `deleted`, and `get_fd` (used by `read_key`/`write_key`/
/// `seek_key`) then rejects all further I/O on it with `BasisLost` -- the
/// intended (POSIX-divergent but deliberate) xous behavior this test asserts.
///
/// XFAIL PFC-11 (empirically discovered 2026-07-07): the marking loop never
/// fires for ordinary std paths. `delete_key` compares `fd.basis == basis`
/// where `basis` comes from `split_basis_and_dict` of the unlink path --
/// `None` for any non-`:basis:`-prefixed path -- while `open_key` always
/// records `fd.basis = Some(<actual basis>)`. So the handle is never marked:
/// the read still errors (the key's data really is gone server-side), but
/// the write goes through `write_key` -> `key_update`, which silently
/// RE-CREATES the deleted key at the old path. See PFC-11.
pub fn delete_while_open() {
    let tmp = TmpDict::new("delete_while_open");
    let path = tmp.path("victim");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"still here"));
    }
    assert_eq!(&read_back(&path)[..], b"still here");

    let mut f = check!(OpenOptions::new().read(true).write(true).open(&path));
    // Unlink the key out from under the still-open handle `f`.
    check!(fs::remove_file(&path));

    let mut buf = [0u8; 4];
    assert!(
        f.read(&mut buf).is_err(),
        "read through a deleted-while-open handle unexpectedly succeeded (POSIX divergence; \
         xous errors here by design)"
    );
    assert!(
        f.write_all(b"x").is_err(),
        "write through a deleted-while-open handle unexpectedly succeeded (POSIX divergence; \
         xous errors here by design)"
    );
    drop(f); // close_key does not check the `deleted` flag; this does not hit PFC-7.
}

/// `File::create` of a key inside a dict that was never created must fail:
/// the fork sends `create_path=false`, so the server will not auto-create the
/// parent dict. This is root cause (b) of upstream issue #286 (mtxcli could
/// not create its storage dict) and the reason TmpDict pre-creates its dict.
pub fn create_in_missing_dict() {
    let tmp = TmpDict::new("create_in_missing_dict");
    // Derive a sibling dict name that was never fs::create_dir'd.
    let ghost_path = format!("{}.neverdict/key", tmp.dict());
    assert!(
        File::create(&ghost_path).is_err(),
        "File::create auto-created a key in a dict that was never created"
    );
    // The failed create must not have materialized the dict either.
    assert!(
        fs::metadata(format!("{}.neverdict", tmp.dict())).is_err(),
        "failed File::create left a phantom dict behind"
    );
}

/// Create/delete/create churn on one key, interleaved with a live second key.
/// Upstream issue #299's comment thread reported a server panic ("Double-free
/// error in free_keys()") under repeated deletion; the fix (2023-01-20) has no
/// pinning test upstream. Ten cycles with read-back each round.
pub fn churn_create_delete() {
    let tmp = TmpDict::new("churn_create_delete");
    let churn = tmp.path("churn");
    let anchor = tmp.path("anchor");
    {
        let mut f = check!(File::create(&anchor));
        check!(f.write_all(b"anchor"));
    }
    for i in 0..10u32 {
        if i % 5 == 0 {
            log::info!("churn_create_delete: cycle {}/10", i);
        }
        let body = [b'a' + (i as u8), b'0' + (i as u8)];
        {
            let mut f = check!(File::create(&churn));
            check!(f.write_all(&body));
        }
        let mut buf = Vec::new();
        check!(check!(File::open(&churn)).read_to_end(&mut buf));
        assert_eq!(buf, body, "cycle {} read-back mismatch", i);
        check!(fs::remove_file(&churn));
        assert!(File::open(&churn).is_err(), "cycle {}: key openable after delete", i);
    }
    // The anchor key must have survived the churn untouched.
    let mut buf = Vec::new();
    check!(check!(File::open(&anchor)).read_to_end(&mut buf));
    assert_eq!(&buf[..], b"anchor", "anchor key corrupted by neighbor churn");
    check!(fs::remove_file(&anchor));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("errors::open_missing_is_err", open_missing_is_err as fn()),
    ("errors::open_missing_kind_characterization", open_missing_kind_characterization as fn()),
    ("errors::remove_missing_path", remove_missing_path as fn()),
    ("errors::remove_file_twice", remove_file_twice as fn()),
    ("errors::remove_dir_on_file", remove_dir_on_file as fn()),
    ("errors::remove_file_on_dir", remove_file_on_dir as fn()),
    ("errors::stat_missing_path", stat_missing_path as fn()),
    ("errors::open_empty_path", open_empty_path as fn()),
    ("errors::dict_name_length_boundary", dict_name_length_boundary as fn()),
    ("errors::key_name_length_boundary", key_name_length_boundary as fn()),
    ("errors::metadata_len_characterization", metadata_len_characterization as fn()),
    ("errors::delete_while_open", delete_while_open as fn()),
    ("errors::create_in_missing_dict", create_in_missing_dict as fn()),
    ("errors::churn_create_delete", churn_create_delete as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    // create_dir swallows ALL server retcodes (fork mkdir never reads the
    // reply), so the over-length name "succeeds" -- see the test's comment.
    ("errors::dict_name_length_boundary", "PFC-6"),
    // rejected over-length key name poisons the dict's key cache -- see the
    // test's comment and PFC-8.
    ("errors::key_name_length_boundary", "PFC-8"),
    ("errors::metadata_len_characterization", "PFC-5"),
    // unlink never marks open handles `deleted` (basis-comparison dead code),
    // so a write through the stale handle re-creates the key -- see the
    // test's comment and PFC-11.
    ("errors::delete_while_open", "PFC-11"),
];
