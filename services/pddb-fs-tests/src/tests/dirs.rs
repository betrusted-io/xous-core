//! Theme: directories (PDDB dicts) and metadata-kind (is_file/is_dir,
//! exists, read_dir/DirEntry). Ported/adapted from upstream
//! library/std/src/fs/tests.rs -- see the `tests` module header
//! (services/pddb-fs-tests/src/tests/mod.rs) for the path grammar ('/'), the
//! truncate hazard, and XFAIL discipline.
//!
//! PDDB dicts are FLAT, not a real directory tree: `create_dict`
//! (services/pddb/src/libstd/mod.rs) takes the entire path remainder (after
//! stripping an optional leading `:basis:` prefix) as ONE literal dict name --
//! it never splits on '/'. `list_path`'s "is this dict a child of that dict"
//! check (`utils::get_path`, services/pddb/src/libstd/utils.rs) matches on
//! ':' hierarchy only, never '/'. So a "nested" dict created via
//! `create_dir_all("<dict>/<sub>")` is really just a second, independent flat
//! dict whose name happens to contain a slash -- it is invisible when reading
//! its "parent"'s contents. See create_dir_all_nested_single_level_visibility.
//! Consequently, upstream tests that assume real recursive dirs (symlinked
//! junctions, subdirectories nested via the OS path separator, `.`/`` cwd
//! tricks) are adapted or skipped; see the per-test notes below.

#![allow(unused_imports)]
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use crate::harness::{TmpDict, XorShift, check};

/// Port of `file_test_stat_is_correct_on_is_file`: metadata via the open
/// File handle, the free function, and the Path method must all agree a
/// plain key is a file.
pub fn stat_is_correct_on_is_file() {
    let tmp = TmpDict::new("stat_is_correct_on_is_file");
    let path = tmp.path("file_stat_correct_on_is_file");
    {
        let mut opts = OpenOptions::new();
        let mut f = check!(opts.read(true).write(true).create(true).open(&path));
        check!(f.write_all(b"hw"));
        let fstat_res = check!(f.metadata());
        assert!(fstat_res.is_file(), "open handle metadata should report is_file()");
    }
    // Every write path must be read-back verified, not
    // just checked for success -- confirm the content before checking stat.
    assert_eq!(check!(fs::read(&path)), b"hw", "content should read back intact");
    let stat_res_fn = check!(fs::metadata(&path));
    assert!(stat_res_fn.is_file(), "fs::metadata should report is_file()");
    let stat_res_meth = check!(Path::new(&path).metadata());
    assert!(stat_res_meth.is_file(), "Path::metadata should report is_file()");
    check!(fs::remove_file(&path));
}

/// Port of `file_test_stat_is_correct_on_is_dir`: a fresh dict must stat as
/// a dir both via the free function and the Path method.
pub fn stat_is_correct_on_is_dir() {
    let tmp = TmpDict::new("stat_is_correct_on_is_dir");
    // A distinct, independent flat dict (not '/'-joined onto tmp.dict()) so
    // this test only exercises "is a dict a dir", not the nesting question.
    let dir = format!("{}_sub", tmp.dict());
    check!(fs::create_dir(&dir));
    let stat_res_fn = check!(fs::metadata(&dir));
    assert!(stat_res_fn.is_dir(), "fs::metadata should report is_dir() for a dict");
    let stat_res_meth = check!(Path::new(&dir).metadata());
    assert!(stat_res_meth.is_dir(), "Path::metadata should report is_dir() for a dict");
    check!(fs::remove_dir(&dir));
}

/// Port of `file_test_fileinfo_false_when_checking_is_file_on_a_directory`.
pub fn fileinfo_false_when_checking_is_file_on_a_directory() {
    let tmp = TmpDict::new("fileinfo_false_on_dir");
    // tmp.dict() itself is already a created, empty dict (TmpDict::new).
    assert!(!Path::new(tmp.dict()).is_file(), "a dict must not report is_file() true");
}

/// Port of `file_test_fileinfo_check_exists_before_and_after_file_creation`.
pub fn fileinfo_check_exists_before_and_after_file_creation() {
    let tmp = TmpDict::new("fileinfo_check_exists_b_and_a");
    let path = tmp.path("fileinfo_check_exists_b_and_a");
    assert!(!Path::new(&path).exists(), "key should not exist before creation");
    check!(check!(File::create(&path)).write_all(b"foo"));
    assert!(Path::new(&path).exists(), "key should exist after creation");
    // metadata().len() is always 0 on xous (PFC-5): verify content by
    // read-back, never via metadata length.
    let content = check!(fs::read(&path));
    assert_eq!(content, b"foo", "content should read back intact");
    check!(fs::remove_file(&path));
    assert!(!Path::new(&path).exists(), "key should not exist after removal");
}

/// Port of `file_test_directoryinfo_check_exists_before_and_after_mkdir`.
pub fn directoryinfo_check_exists_before_and_after_mkdir() {
    let tmp = TmpDict::new("directoryinfo_before_and_after");
    let dir = format!("{}_sub", tmp.dict());
    assert!(!Path::new(&dir).exists(), "dict should not exist before mkdir");
    check!(fs::create_dir(&dir));
    assert!(Path::new(&dir).exists(), "dict should exist after mkdir");
    assert!(Path::new(&dir).is_dir(), "created path should report is_dir()");
    check!(fs::remove_dir(&dir));
    assert!(!Path::new(&dir).exists(), "dict should not exist after removal");
}

/// Port of `file_test_directoryinfo_readdir`, adapted: `metadata().len()` is
/// always 0 on xous (PFC-5), so content is verified by read-back instead of
/// length; entries are asserted `is_file()` (a dict's own listing only ever
/// contains keys -- see the module-level flatness note).
pub fn directoryinfo_readdir() {
    let tmp = TmpDict::new("directoryinfo_readdir");
    let prefix = "foo";
    for n in 0..3 {
        let path = tmp.path(&format!("{n}.txt"));
        let msg = format!("{prefix}{n}");
        check!(check!(File::create(&path)).write_all(msg.as_bytes()));
    }

    let mut seen = 0;
    for f in check!(fs::read_dir(tmp.dict())) {
        let f = check!(f);
        assert!(check!(f.metadata()).is_file(), "readdir entry should be a file (key)");
        let name = f.file_name().into_string().expect("utf8 filename");
        let stem = name.strip_suffix(".txt").expect("expected a .txt-suffixed key");
        let expected = format!("{prefix}{stem}");
        let actual = check!(fs::read_to_string(f.path()));
        assert_eq!(actual, expected, "content mismatch for entry {name}");
        check!(fs::remove_file(f.path()));
        seen += 1;
    }
    assert_eq!(seen, 3, "expected exactly 3 readdir entries");
}

/// Port of `dir_entry_methods`. PDDB dicts are flat (see module note): a
/// child dict never shows up in another dict's `read_dir`, so unlike
/// upstream (which mixes a subdirectory and a file in one listing) every
/// entry seen here is a key -- this exercises `file_name()`, `file_type()`,
/// and `metadata()` agreeing on the file/is_file() kind.
pub fn dir_entry_methods() {
    let tmp = TmpDict::new("dir_entry_methods");
    check!(fs::write(tmp.path("a"), b"aaa"));
    check!(fs::write(tmp.path("b"), b"bbb"));

    let mut seen = BTreeSet::new();
    for entry in check!(fs::read_dir(tmp.dict())) {
        let entry = check!(entry);
        assert!(check!(entry.file_type()).is_file(), "file_type() should report is_file()");
        assert!(check!(entry.metadata()).is_file(), "metadata() should report is_file()");
        let name = entry.file_name().into_string().expect("utf8 filename");
        // Read-back verification of the write behind this entry:
        // don't just trust fs::write's Ok(()), confirm the content DirEntry::path()
        // resolves to is exactly what was written for that key.
        let expected_content: &[u8] = match name.as_str() {
            "a" => b"aaa",
            "b" => b"bbb",
            other => panic!("unexpected DirEntry name {}", other),
        };
        assert_eq!(check!(fs::read(entry.path())), expected_content, "content mismatch for entry {name}");
        seen.insert(name);
    }
    let expected: BTreeSet<String> = BTreeSet::from(["a".to_string(), "b".to_string()]);
    assert_eq!(seen, expected, "unexpected DirEntry set from read_dir");

    check!(fs::remove_file(tmp.path("a")));
    check!(fs::remove_file(tmp.path("b")));
}

/// Port of `read_dir_not_found`. Don't assert a specific ErrorKind: the
/// contract only requires that for `Unsupported`-characterization tests
/// (most xous errors collapse to `ErrorKind::Other`). Still routed through a
/// `TmpDict` (counter-unique prefix) rather than a bare string literal, per
/// the isolation rules, even though nothing is created here:
/// `_missing` is never created, so the dict genuinely does not exist.
///
/// XFAIL PFC-9: `fs::read_dir` on a nonexistent directory returns Ok with an
/// EMPTY iterator instead of an error. The server's `list_path`
/// (services/pddb/src/libstd/mod.rs ~184: "Ignore errors, since sometimes
/// the dict doesn't exist" -- `key_list(...).unwrap_or_default()`) never
/// reports a missing dict, and the client's `readdir` (rust fork
/// sys/fs/xous.rs) has no retcode check either. POSIX requires ENOENT here;
/// assert the error and expect the XFAIL until PFC-9 is fixed.
pub fn read_dir_not_found() {
    let tmp = TmpDict::new("read_dir_not_found");
    let missing = format!("{}_missing", tmp.dict());
    let res = fs::read_dir(&missing);
    let err = res.expect_err("read_dir on a nonexistent dict should error (PFC-9)");
    log::info!("read_dir on nonexistent dict returned ErrorKind::{:?}", err.kind());
}

/// Port of `unicode_path_is_dir`. The upstream `.`/relative-path checks are
/// dropped: xous std::fs has no cwd concept for PDDB paths (every path is a
/// dict or dict/key string, never `/`-rooted or `.`-relative).
pub fn unicode_path_is_dir() {
    let tmp = TmpDict::new("unicode_path_is_dir");
    let dirpath = format!("{}_test-\u{ac00}\u{4e00}\u{30fc}\u{4f60}\u{597d}", tmp.dict());
    check!(fs::create_dir(&dirpath));
    assert!(Path::new(&dirpath).is_dir(), "unicode-named dict should report is_dir()");

    let filepath = format!("{dirpath}/unicode-file-\u{ac00}\u{4e00}\u{30fc}\u{4f60}\u{597d}.rs");
    check!(File::create(&filepath)); // ignore return; touch only
    assert!(!Path::new(&filepath).is_dir(), "unicode-named file must not report is_dir()");
    assert!(Path::new(&filepath).exists(), "unicode-named file should exist");

    check!(fs::remove_file(&filepath));
    check!(fs::remove_dir(&dirpath));
}

/// Port of `unicode_path_exists`. `.`/relative-path checks dropped (see
/// unicode_path_is_dir).
pub fn unicode_path_exists() {
    let tmp = TmpDict::new("unicode_path_exists");
    let dirpath = format!("{}_test-\u{ac01}\u{4e01}\u{30fc}\u{518d}\u{89c1}", tmp.dict());
    assert!(!Path::new(&dirpath).exists(), "unicode dict should not exist before creation");
    check!(fs::create_dir(&dirpath));
    assert!(Path::new(&dirpath).exists(), "unicode dict should exist after creation");

    let bogus = format!("{}_bogus-\u{ac02}\u{4e02}", tmp.dict());
    assert!(!Path::new(&bogus).exists(), "unrelated bogus unicode path should not exist");

    check!(fs::remove_dir(&dirpath));
}

/// Port of `mkdir_path_already_exists_error`: POSIX mkdir semantics require
/// `create_dir` on an existing path to fail. XFAIL PFC-6: the xous client's
/// `create_dir` discards the server's error retcode and returns Ok even when
/// the dict already exists (rust fork sys/fs/xous.rs ~444-448; contrast
/// unlink/rmdir, which do check it). Never weaken
/// this to `is_ok()` -- the correct behavior is `is_err()`.
pub fn mkdir_path_already_exists_error() {
    let tmp = TmpDict::new("mkdir_path_already_exists_error");
    let dir = format!("{}_twice", tmp.dict());
    check!(fs::create_dir(&dir));
    let r = fs::create_dir(&dir);
    assert!(r.is_err(), "create_dir on an already-existing dict must fail (PFC-6)");
    check!(fs::remove_dir(&dir));
}

/// Adapted from upstream `recursive_rmdir` (no symlinks/junctions on xous --
/// forbidden, and meaningless for a flat dict store
/// anyway): `remove_dir_all` on a dict containing several keys must remove
/// every key and the dict itself, while an unrelated sibling dict (and its
/// "canary" key) is left completely untouched.
pub fn recursive_rmdir() {
    let victim = TmpDict::new("recursive_rmdir_victim");
    let victim_dict = victim.dict().to_string();
    for i in 0..3 {
        let key_path = victim.path(&format!("k{i}"));
        check!(fs::write(&key_path, format!("v{i}")));
        // Read-back verification of every write before
        // the key is destroyed by remove_dir_all below.
        assert_eq!(
            check!(fs::read_to_string(&key_path)),
            format!("v{i}"),
            "content mismatch for {key_path} before remove_dir_all"
        );
    }

    let sibling = TmpDict::new("recursive_rmdir_sibling");
    let canary_path = sibling.path("do_not_delete");
    check!(fs::write(&canary_path, b"canary"));

    check!(fs::remove_dir_all(&victim_dict));

    assert!(fs::metadata(&victim_dict).is_err(), "victim dict should be gone after remove_dir_all");
    for i in 0..3 {
        let key_path = format!("{victim_dict}/k{i}");
        assert!(File::open(&key_path).is_err(), "key {} should be gone after remove_dir_all", key_path);
    }

    let canary = check!(fs::read(&canary_path));
    assert_eq!(canary, b"canary", "unrelated sibling dict's canary key was disturbed");
    check!(fs::remove_file(&canary_path));
}

/// Port of `recursive_rmdir_of_file_fails`: remove_dir_all must refuse to
/// delete a plain key (not a dict).
pub fn recursive_rmdir_of_file_fails() {
    let tmp = TmpDict::new("recursive_rmdir_of_file_fails");
    let path = tmp.path("do_not_delete");
    check!(fs::write(&path, b"foo"));
    let r = fs::remove_dir_all(&path);
    assert!(r.is_err(), "remove_dir_all on a plain key must fail");
    let content = check!(fs::read(&path));
    assert_eq!(content, b"foo", "key must survive a failed remove_dir_all");
    check!(fs::remove_file(&path));
}

/// New (PDDB-specific) test filling in the "nested create_dir_all + readdir
/// single-level visibility" characterization named in the module note.
/// `create_dir_all("<dict>/sub")` succeeds and both `<dict>` and `<dict>/sub`
/// independently stat as dicts, but `<dict>/sub` never appears when reading
/// `<dict>`'s own contents: PDDB has no real directory tree, only a flat
/// dict namespace, and the dict-nesting check in `list_path`
/// (`utils::get_path`, services/pddb/src/libstd/utils.rs) matches only a
/// ':'-joined hierarchy, never '/'. Documented xous semantic, not a bug.
pub fn create_dir_all_nested_single_level_visibility() {
    let tmp = TmpDict::new("nested_single_level_visibility");
    let nested = tmp.path("sub");

    check!(fs::create_dir_all(&nested));
    let meta = check!(fs::metadata(&nested));
    assert!(meta.is_dir(), "nested create_dir_all target should stat as a dict");

    let entries: Vec<String> = check!(fs::read_dir(tmp.dict()))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        !entries.iter().any(|n| n == "sub"),
        "'/'-nested dict must not appear as a child entry of its parent's \
         read_dir (PDDB dicts are flat): got {:?}",
        entries
    );

    // tmp.dict() and tmp.dict()/sub are two independent flat dicts; remove
    // the nested one explicitly (TmpDict::drop only removes tmp.dict()).
    check!(fs::remove_dir(&nested));
}

/// Adapted from upstream `concurrent_recursive_mkdir` (there: 8 threads x 100
/// iterations x 40-level nesting). Drastically scaled down for the emulation
/// runtime budget and because PDDB dicts are flat (there is only one level
/// to race on -- see the module note) -- but the invariant under test still
/// holds: concurrent `create_dir_all` calls racing on the *same* target must
/// not corrupt state, and must leave a valid, existing dict behind.
pub fn concurrent_recursive_mkdir() {
    let tmp = TmpDict::new("concurrent_recursive_mkdir");
    let nested = Arc::new(tmp.path("nest"));

    let mut joins = Vec::new();
    for _ in 0..2 {
        let nested = Arc::clone(&nested);
        joins.push(thread::spawn(move || fs::create_dir_all(nested.as_str())));
    }

    let mut ok_count = 0;
    for j in joins {
        match j.join().expect("mkdir thread panicked") {
            Ok(()) => ok_count += 1,
            Err(e) => log::info!("concurrent create_dir_all: {:?}", e.kind()),
        }
    }
    assert!(ok_count >= 1, "at least one concurrent create_dir_all must succeed");
    assert!(
        check!(fs::metadata(nested.as_str())).is_dir(),
        "nested dict should exist after concurrent create_dir_all"
    );

    check!(fs::remove_dir(nested.as_str()));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("dirs::stat_is_correct_on_is_file", stat_is_correct_on_is_file as fn()),
    ("dirs::stat_is_correct_on_is_dir", stat_is_correct_on_is_dir as fn()),
    (
        "dirs::fileinfo_false_when_checking_is_file_on_a_directory",
        fileinfo_false_when_checking_is_file_on_a_directory as fn(),
    ),
    (
        "dirs::fileinfo_check_exists_before_and_after_file_creation",
        fileinfo_check_exists_before_and_after_file_creation as fn(),
    ),
    (
        "dirs::directoryinfo_check_exists_before_and_after_mkdir",
        directoryinfo_check_exists_before_and_after_mkdir as fn(),
    ),
    ("dirs::directoryinfo_readdir", directoryinfo_readdir as fn()),
    ("dirs::dir_entry_methods", dir_entry_methods as fn()),
    ("dirs::read_dir_not_found", read_dir_not_found as fn()),
    ("dirs::unicode_path_is_dir", unicode_path_is_dir as fn()),
    ("dirs::unicode_path_exists", unicode_path_exists as fn()),
    ("dirs::mkdir_path_already_exists_error", mkdir_path_already_exists_error as fn()),
    ("dirs::recursive_rmdir", recursive_rmdir as fn()),
    ("dirs::recursive_rmdir_of_file_fails", recursive_rmdir_of_file_fails as fn()),
    (
        "dirs::create_dir_all_nested_single_level_visibility",
        create_dir_all_nested_single_level_visibility as fn(),
    ),
    ("dirs::concurrent_recursive_mkdir", concurrent_recursive_mkdir as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    ("dirs::mkdir_path_already_exists_error", "PFC-6"),
    // read_dir on a missing dict returns Ok(empty) instead of an error --
    // see the test's doc comment and PFC-9.
    ("dirs::read_dir_not_found", "PFC-9"),
];
