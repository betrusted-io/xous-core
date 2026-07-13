//! Theme: characterize the unsupported surface.
//!
//! Characterization facts: rename, set_len, fsync/datasync, File::flush,
//! permissions, canonicalize, and link all map to ErrorKind::Unsupported on
//! xous today -- this is documented client-side stub behavior (no server
//! opcode exists for any of them), not a PFC-tracked bug, so these tests
//! assert the Unsupported kind directly with no XFAIL entry. try_clone is
//! grouped with these in the forbidden-API list for the same reason and is
//! asserted the same way. fs::read_link (on a regular, non-symlink file) and
//! fs::set_permissions are NOT in that documented list -- research suggests
//! InvalidInput and a silent no-op respectively, but neither is confirmed, so
//! those two tests characterize the actual observed behavior instead of
//! asserting a specific kind (still requiring is_err()/data-intact as
//! appropriate) per the error-assertion rule.
//! Every test verifies pre-existing file data survives the failed call via
//! read-back, per the SERVER-CRASH HAZARD / read-back rules; all files here
//! are a few bytes, well under the 4 KiB large-pool truncate hazard, and each
//! is created exactly once (no truncating re-create).

#![allow(unused_imports)]
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::harness::{TmpDict, XorShift, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// fs::rename returns Unsupported and leaves both source and destination intact
/// (documented semantic: no rename opcode exists server-side at all). This
/// kills the temp-file+rename idiom for atomic writes on xous.
pub fn rename_unsupported() {
    let tmp = TmpDict::new("rename_unsupported");
    let src_path = tmp.path("source");
    let dst_path = tmp.path("dest");

    // Create source file with known data.
    {
        let mut f = check!(File::create(&src_path));
        check!(f.write_all(b"source data"));
    }
    let src_before = read_back(&src_path);

    // Try to rename; must fail with Unsupported.
    let rename_result = fs::rename(&src_path, &dst_path);
    assert!(rename_result.is_err(), "rename should not succeed");
    match rename_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "rename error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("rename unexpectedly succeeded"),
    }

    // Verify source file is still there and intact.
    let src_after = read_back(&src_path);
    assert_eq!(src_before, src_after, "source file data corrupted by failed rename");

    // Verify destination was not created.
    assert!(File::open(&dst_path).is_err(), "destination should not have been created");

    check!(fs::remove_file(&src_path));
}

/// File::set_len returns Unsupported and leaves file data intact (documented
/// semantic).
pub fn set_len_unsupported() {
    let tmp = TmpDict::new("set_len_unsupported");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"test data"));
    }
    let before = read_back(&path);

    // Try to set_len; must fail with Unsupported.
    let f = check!(File::open(&path));
    let set_len_result = f.set_len(100);
    assert!(set_len_result.is_err(), "set_len should not succeed");
    match set_len_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "set_len error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("set_len unexpectedly succeeded"),
    }
    drop(f);

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by failed set_len");

    check!(fs::remove_file(&path));
}

/// File::sync_all returns Unsupported and leaves file data intact (documented
/// semantic: fsync/datasync).
pub fn sync_all_unsupported() {
    let tmp = TmpDict::new("sync_all_unsupported");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"sync test"));
    }
    let before = read_back(&path);

    // Try to sync_all; must fail with Unsupported.
    let f = check!(File::open(&path));
    let sync_result = f.sync_all();
    assert!(sync_result.is_err(), "sync_all should not succeed");
    match sync_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "sync_all error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("sync_all unexpectedly succeeded"),
    }
    drop(f);

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by failed sync_all");

    check!(fs::remove_file(&path));
}

/// File::sync_data returns Unsupported and leaves file data intact (documented
/// semantic: fsync/datasync).
pub fn sync_data_unsupported() {
    let tmp = TmpDict::new("sync_data_unsupported");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"data sync"));
    }
    let before = read_back(&path);

    // Try to sync_data; must fail with Unsupported.
    let f = check!(File::open(&path));
    let sync_result = f.sync_data();
    assert!(sync_result.is_err(), "sync_data should not succeed");
    match sync_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "sync_data error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("sync_data unexpectedly succeeded"),
    }
    drop(f);

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by failed sync_data");

    check!(fs::remove_file(&path));
}

/// Write::flush on a File returns Unsupported (documented semantic), and
/// buffered data written before the failed flush is still readable afterwards.
pub fn flush_unsupported() {
    let tmp = TmpDict::new("flush_unsupported");
    let path = tmp.path("file");

    // Create file and write buffered data.
    let mut f = check!(File::create(&path));
    check!(f.write_all(b"buffered"));

    // Try to flush; must fail with Unsupported.
    let flush_result = f.flush();
    assert!(flush_result.is_err(), "flush should not succeed");
    match flush_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "flush error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("flush unexpectedly succeeded"),
    }
    drop(f);

    // Verify buffered data was still written and readable.
    let content = read_back(&path);
    assert_eq!(&content[..], b"buffered", "buffered data was not written before flush failure");

    check!(fs::remove_file(&path));
}

/// fs::canonicalize returns Unsupported and leaves file intact (documented
/// semantic).
pub fn canonicalize_unsupported() {
    let tmp = TmpDict::new("canonicalize_unsupported");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"real file"));
    }
    let before = read_back(&path);

    // Try to canonicalize; must fail with Unsupported.
    let canon_result = fs::canonicalize(&path);
    assert!(canon_result.is_err(), "canonicalize should not succeed");
    match canon_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "canonicalize error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("canonicalize unexpectedly succeeded"),
    }

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by failed canonicalize");

    check!(fs::remove_file(&path));
}

/// fs::hard_link returns Unsupported and leaves source file intact (documented
/// semantic).
pub fn hard_link_unsupported() {
    let tmp = TmpDict::new("hard_link_unsupported");
    let src_path = tmp.path("source");
    let link_path = tmp.path("link");

    // Create source file with known data.
    {
        let mut f = check!(File::create(&src_path));
        check!(f.write_all(b"link source"));
    }
    let src_before = read_back(&src_path);

    // Try to create hard link; must fail with Unsupported.
    let link_result = fs::hard_link(&src_path, &link_path);
    assert!(link_result.is_err(), "hard_link should not succeed");
    match link_result {
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "hard_link error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
        Ok(_) => panic!("hard_link unexpectedly succeeded"),
    }

    // Verify source file is still there and intact.
    let src_after = read_back(&src_path);
    assert_eq!(src_before, src_after, "source file data corrupted by failed hard_link");

    // Verify link was not created.
    assert!(File::open(&link_path).is_err(), "link file should not have been created");

    check!(fs::remove_file(&src_path));
}

/// fs::read_link on a regular (non-symlink) file returns an error. Research
/// suggests ErrorKind::InvalidInput (the POSIX EINVAL case), but this is NOT
/// in the documented-semantic list, so we only assert is_err() here and log
/// the observed kind for the record rather than binding the test to an
/// unconfirmed kind (only assert a specific ErrorKind for the confirmed
/// Unsupported-surface tests).
pub fn read_link_regular_file() {
    let tmp = TmpDict::new("read_link_regular_file");
    let path = tmp.path("file");

    // Create a regular file (not a symlink).
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"not a link"));
    }
    let before = read_back(&path);

    // Try to read_link on a regular file; must fail.
    let readlink_result = fs::read_link(&path);
    assert!(readlink_result.is_err(), "read_link on regular file should fail");
    match readlink_result {
        Err(e) => {
            // Research says InvalidInput, but document what we actually get.
            log::info!("read_link on regular file returned ErrorKind::{:?}", e.kind());
            // Do not assert a specific kind here; xous may differ from unix.
        }
        Ok(_) => panic!("read_link on regular file unexpectedly succeeded"),
    }

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by failed read_link");

    check!(fs::remove_file(&path));
}

/// fs::set_permissions on a file: research says readonly is a silent no-op,
/// but this is NOT in the documented-semantic list (only "permissions"
/// generally is, without specifying Ok-vs-Err), so this test
/// characterizes the actual xous behavior rather than asserting a kind: if
/// the call succeeds, assert the (no-op) call did not damage the file's data,
/// AND assert the readonly flag was not actually enforced (a further write
/// still succeeds) -- proving it really is a no-op rather than a silent
/// success that nonetheless changed enforcement; if it errors, assert the
/// file is still intact.
pub fn set_permissions_characterize() {
    let tmp = TmpDict::new("set_permissions_characterize");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"perm test"));
    }
    let before = read_back(&path);

    // Get current permissions and create a readonly version.
    let perms = check!(fs::metadata(&path)).permissions();
    let mut readonly_perms = perms.clone();
    readonly_perms.set_readonly(true);

    // Try to set permissions to readonly.
    let set_perm_result = fs::set_permissions(&path, readonly_perms.clone());

    match set_perm_result {
        Ok(()) => {
            // If it succeeds, verify it didn't corrupt the file.
            let after = read_back(&path);
            assert_eq!(before, after, "file data corrupted by set_permissions");
            log::info!("set_permissions returned Ok (characterizing whether it is a no-op)");

            // Prove it is really a no-op (research expectation) rather than a
            // silent success that also enforced readonly: a further write
            // must still succeed and land in the file.
            let mut f2 = check!(OpenOptions::new().append(true).open(&path));
            check!(f2.write_all(b"!"));
            drop(f2);
            let after_write = read_back(&path);
            let mut expected = after.clone();
            expected.push(b'!');
            assert_eq!(
                after_write, expected,
                "write after set_permissions(readonly) was blocked or corrupted \
                 the file -- readonly is NOT a no-op on xous, update the characterization"
            );
        }
        Err(e) => {
            // If it fails, characterize the error.
            log::info!("set_permissions returned Err: {:?}", e.kind());
            // Either way, file should be intact.
            let after = read_back(&path);
            assert_eq!(before, after, "file data corrupted by failed set_permissions");
        }
    }

    check!(fs::remove_file(&path));
}

/// File::try_clone returns Unsupported and leaves file data intact. try_clone
/// is grouped with the confirmed-Unsupported forbidden-API set
/// (rename/hard_link/permissions/canonicalize/locks/times), so -- unlike
/// read_link/set_permissions above -- this asserts the specific kind rather
/// than merely characterizing whichever result comes back.
pub fn try_clone_unsupported() {
    let tmp = TmpDict::new("try_clone_unsupported");
    let path = tmp.path("file");

    // Create file with known data.
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"clone test"));
    }
    let before = read_back(&path);

    // Try to clone a file handle; must fail with Unsupported.
    let f1 = check!(File::open(&path));
    let clone_result = f1.try_clone();
    match clone_result {
        Ok(_) => panic!(
            "try_clone unexpectedly succeeded -- update the characterization \
             (the forbidden-API list assumes it errors)"
        ),
        Err(e) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::Unsupported,
                "try_clone error should be Unsupported, got: {:?}",
                e.kind()
            );
        }
    }
    drop(f1);

    // Verify file data is intact.
    let after = read_back(&path);
    assert_eq!(before, after, "file data corrupted by try_clone");

    check!(fs::remove_file(&path));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("unsupported::rename_unsupported", rename_unsupported as fn()),
    ("unsupported::set_len_unsupported", set_len_unsupported as fn()),
    ("unsupported::sync_all_unsupported", sync_all_unsupported as fn()),
    ("unsupported::sync_data_unsupported", sync_data_unsupported as fn()),
    ("unsupported::flush_unsupported", flush_unsupported as fn()),
    ("unsupported::canonicalize_unsupported", canonicalize_unsupported as fn()),
    ("unsupported::hard_link_unsupported", hard_link_unsupported as fn()),
    ("unsupported::read_link_regular_file", read_link_regular_file as fn()),
    ("unsupported::set_permissions_characterize", set_permissions_characterize as fn()),
    ("unsupported::try_clone_unsupported", try_clone_unsupported as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[];
