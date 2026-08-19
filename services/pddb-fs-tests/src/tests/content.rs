//! Theme: data integrity and fs:: convenience APIs.
//! Tests cover fs::write, fs::read, fs::read_to_string, fs::copy, and read_dir enumeration.

#![allow(unused_imports)]
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};

use crate::harness::{TmpDict, XorShift, check, error_contains};

/// Helper function to read file content into a Vec.
fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// fs::write and fs::read convenience APIs: write bytes, read them back.
/// Also test fs::read_to_string with invalid UTF-8 error handling.
pub fn write_then_read() {
    let tmp = TmpDict::new("write_then_read");
    let path_bin = tmp.path("binary");
    let path_utf8 = tmp.path("utf8");
    let path_invalid = tmp.path("invalid_utf8");

    // Test 1: fs::write and fs::read with binary data
    let test_data = b"hello world\x00\x01\x02";
    check!(fs::write(&path_bin, test_data));
    let readback = check!(fs::read(&path_bin));
    assert_eq!(readback, test_data, "binary read did not match written data");

    // Test 2: fs::write and fs::read_to_string with valid UTF-8
    let utf8_str = "Hello, PDDB!";
    check!(fs::write(&path_utf8, utf8_str.as_bytes()));
    let readback_str = check!(fs::read_to_string(&path_utf8));
    assert_eq!(readback_str, utf8_str, "UTF-8 read did not match written string");

    // Test 3: fs::read_to_string with invalid UTF-8 must error. This message
    // is produced client-side by std's own UTF-8 validation of the bytes it
    // already read back (io::Read::read_to_string), not by the xous/PDDB
    // server, so (unlike most xous fs errors) the
    // exact upstream message is portable and safe to assert here.
    check!(fs::write(&path_invalid, &[0xFF, 0xFE, 0xFD]));
    error_contains!(fs::read_to_string(&path_invalid), "stream did not contain valid UTF-8");

    // Cleanup
    check!(fs::remove_file(&path_bin));
    check!(fs::remove_file(&path_utf8));
    check!(fs::remove_file(&path_invalid));
}

/// Binary file test: write XorShift-generated random content (~2 KiB),
/// read back and verify byte-exact match.
pub fn binary_file() {
    let tmp = TmpDict::new("binary_file");
    let path = tmp.path("data");

    // Generate 2 KiB of deterministic random data using XorShift
    let mut rng = XorShift::new(12345);
    let mut bytes = vec![0u8; 2048];
    rng.fill(&mut bytes);

    // Write the binary data
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&bytes));
    }

    // Read back and verify exact match
    let readback = read_back(&path);
    assert_eq!(readback.len(), bytes.len(), "read back length mismatch");
    assert_eq!(readback, bytes, "binary content did not match after read-back");

    // Cleanup
    check!(fs::remove_file(&path));
}

/// Test fs::copy with valid source and destination.
///
/// PASSES on target (empirically confirmed 2026-07-07; formerly registered
/// XFAIL PFC-4). The predicted close cascade half-happens: fs::copy holds
/// both files open, the writer drops first, and its successful CloseKeyStd
/// really does drop the WHOLE per-process fd table (PFC-4 is real -- see
/// smoke::two_files_close_one), so the reader's follow-up close is answered
/// with an error retcode via `return_scalar`. But the fork's
/// `blocking_scalar` (os/xous/ffi.rs) maps ANY Scalar1/Scalar2 reply to Ok,
/// so `File::drop`'s unwrap (PFC-7) never observes the retcode -- the close
/// error is silently discarded instead of panicking. `io::copy` completes
/// before either close, so the copy and its content survive intact.
pub fn copy_file_ok() {
    let tmp = TmpDict::new("copy_file_ok");
    let src = tmp.path("source");
    let dst = tmp.path("dest");

    // Create source file with content
    let content = b"copy this content";
    check!(fs::write(&src, content));

    // Copy the file
    check!(fs::copy(&src, &dst));

    // Verify destination has same content
    let dst_content = check!(fs::read(&dst));
    assert_eq!(dst_content, content, "copied file content mismatch");

    // Cleanup
    check!(fs::remove_file(&src));
    check!(fs::remove_file(&dst));
}

/// Test fs::copy when source does not exist must error.
pub fn copy_file_does_not_exist() {
    let tmp = TmpDict::new("copy_file_does_not_exist");
    let src = tmp.path("nonexistent");
    let dst = tmp.path("output");

    // Both source and destination don't exist
    let result = fs::copy(&src, &dst);
    assert!(result.is_err(), "copy of non-existent source should error");
    assert!(fs::metadata(&src).is_err(), "non-existent source should not exist");
    assert!(fs::metadata(&dst).is_err(), "output should not exist after failed copy");
}

/// Test fs::copy when source does not exist but destination exists.
/// The destination should remain unchanged.
pub fn copy_src_does_not_exist() {
    let tmp = TmpDict::new("copy_src_does_not_exist");
    let src = tmp.path("nonexistent");
    let dst = tmp.path("existing");

    // Create destination with initial content
    let initial_content = b"preserve me";
    check!(fs::write(&dst, initial_content));

    // Try to copy from non-existent source
    let result = fs::copy(&src, &dst);
    assert!(result.is_err(), "copy from non-existent source should error");

    // Verify destination was not modified
    let preserved = check!(fs::read(&dst));
    assert_eq!(preserved, initial_content, "destination was modified by failed copy");

    // Cleanup
    check!(fs::remove_file(&dst));
}

/// Test fs::copy when destination is a dictionary (not a file). Ported from
/// upstream `copy_file_dst_dir`, which targets `tmpdir.path()` itself (the
/// enclosing directory), not a freshly-nested subdirectory -- deliberately
/// mirrored here rather than building `tmp.dict()` + "/sub": per the dirs.rs
/// module note, PDDB dicts are FLAT, and `create_dir`/`fs::copy` never split
/// on '/' the way `open`/`remove_file` split their final dict/key component --
/// a `dict/sub` path created via `create_dir` would be a second, independent
/// flat dict, and `fs::copy(&src, "<dict>/sub")` would actually resolve (via
/// the open-side dict/key rsplit_once('/')) to a KEY named "sub" inside
/// `tmp.dict()`, not to that unrelated dict at all -- silently defeating the
/// intended "copy onto a directory" scenario. `tmp.dict()` bare (no '/') is
/// the one path shape that both (a) genuinely already stats as a dict and
/// (b) can never be split into a dict/key pair, so a copy onto it must error.
pub fn copy_file_dst_dir() {
    let tmp = TmpDict::new("copy_file_dst_dir");
    let src = tmp.path("source");
    let content = b"source content";
    check!(fs::write(&src, content));

    let result = fs::copy(&src, tmp.dict());
    assert!(result.is_err(), "copy onto an existing dict path should error");

    // The dict itself must survive, untouched, as a directory -- and `src`
    // must be unaffected by the failed copy.
    let meta = check!(fs::metadata(tmp.dict()));
    assert!(meta.is_dir(), "destination dict should still stat as a directory after failed copy");
    assert_eq!(&check!(fs::read(&src))[..], content, "src must be unaffected by a failed copy");

    check!(fs::remove_file(&src));
}

/// Test fs::copy when source is a dictionary (not a file). Ported from
/// upstream `copy_file_src_dir` (source = `tmpdir.path()` itself); see
/// copy_file_dst_dir's doc comment for why `tmp.dict()` bare -- not a
/// `/`-joined nested path -- is the correct way to address "an existing
/// dict" here.
pub fn copy_file_src_dir() {
    let tmp = TmpDict::new("copy_file_src_dir");
    let dst = tmp.path("output");

    let result = fs::copy(tmp.dict(), &dst);
    assert!(result.is_err(), "copy from a dict path should error");
    assert!(fs::metadata(&dst).is_err(), "destination should not be created for a failed copy");
}

/// Test fs::copy when destination file already exists (both files small < 4 KiB).
/// The copy should succeed and overwrite the destination with source content.
///
/// PASSES on target (empirically confirmed 2026-07-07; formerly registered
/// XFAIL PFC-4): same silently-discarded close cascade as copy_file_ok --
/// see its doc comment. (The truncating re-create of `dst` stays far under
/// the 4 KiB PFC-1 hazard line.)
pub fn copy_file_dst_exists() {
    let tmp = TmpDict::new("copy_file_dst_exists");
    let src = tmp.path("source");
    let dst = tmp.path("dest");

    // Create source file with content (keep small < 4 KiB)
    let src_content = b"new content from source";
    check!(fs::write(&src, src_content));

    // Create destination file with different content (keep small < 4 KiB)
    let dst_initial = b"old content at destination";
    check!(fs::write(&dst, dst_initial));

    // Copy source to destination (should overwrite)
    check!(fs::copy(&src, &dst));

    // Verify destination now has source's content
    let dst_final = check!(fs::read(&dst));
    assert_eq!(dst_final, src_content, "destination should be overwritten with source content");

    // Cleanup
    check!(fs::remove_file(&src));
    check!(fs::remove_file(&dst));
}

/// Multi-file test: create ~30 small keys in one dict, enumerate with read_dir,
/// verify all names are present, spot-check 3 contents, remove all, verify empty.
///
/// `readdir` is a single ListPathStd call with a 4096-byte senres reply, and
/// truncation behavior past that size
/// is unverified; this test's ~30 short names is deliberately in that
/// unexplored range. Assert the CORRECT (full-enumeration) behavior as-is --
/// if the reply truncates in practice, that is a new characterization to
/// register as a PFC/XFAIL, not a reason to shrink the assertion here.
pub fn read_dir_enumerate() {
    let tmp = TmpDict::new("read_dir_enumerate");

    // Create 30 small files with distinctive content
    let num_files = 30;
    let mut expected_names = BTreeSet::new();

    for i in 0..num_files {
        let name = format!("file_{:02}", i);
        let path = tmp.path(&name);
        let content = format!("content_{}", i).into_bytes();
        check!(fs::write(&path, &content));
        expected_names.insert(name);
        // Console liveness: a fresh key write emits NO console output and
        // costs ~4-8 s host under Renode, so 30 silent writes overran the
        // driver's 180 s inactivity reaper on the first suite cold run
        // (the system was healthy; the reaper presumed the server dead).
        // Any long fs-op loop must emit periodic diagnostics like this.
        if (i + 1) % 5 == 0 {
            log::info!("read_dir_enumerate: created {}/{} files", i + 1, num_files);
        }
    }

    // Enumerate directory with read_dir and collect all names
    let entries = check!(fs::read_dir(tmp.dict()));
    let mut found_names = BTreeSet::new();

    for entry_result in entries {
        let entry = check!(entry_result);
        let file_name = entry.file_name();
        let name_str = check!(file_name.into_string().or_else(|_| Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "filename not valid UTF-8"
        ))));
        found_names.insert(name_str);
    }

    // Verify all expected files were enumerated
    assert_eq!(found_names, expected_names, "enumerated files do not match created files");

    // Spot-check 3 files' contents: file_00, file_15, file_29
    let check_indices = [0, 15, 29];
    for &idx in &check_indices {
        let name = format!("file_{:02}", idx);
        let path = tmp.path(&name);
        let expected = format!("content_{}", idx).into_bytes();
        let actual = check!(fs::read(&path));
        assert_eq!(actual, expected, "spot-check failed for {}", name);
    }

    // Remove all files (removes do log server-side, but keep the same
    // liveness cadence as the create loop for slow-runner headroom)
    for i in 0..num_files {
        let name = format!("file_{:02}", i);
        let path = tmp.path(&name);
        check!(fs::remove_file(&path));
        if (i + 1) % 5 == 0 {
            log::info!("read_dir_enumerate: removed {}/{} files", i + 1, num_files);
        }
    }

    // Verify dict is now empty
    let entries_after = check!(fs::read_dir(tmp.dict()));
    let remaining_count = entries_after.count();
    assert_eq!(remaining_count, 0, "dict should be empty after removing all files");
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("content::write_then_read", write_then_read as fn()),
    ("content::binary_file", binary_file as fn()),
    ("content::copy_file_ok", copy_file_ok as fn()),
    ("content::copy_file_does_not_exist", copy_file_does_not_exist as fn()),
    ("content::copy_src_does_not_exist", copy_src_does_not_exist as fn()),
    ("content::copy_file_dst_dir", copy_file_dst_dir as fn()),
    ("content::copy_file_src_dir", copy_file_src_dir as fn()),
    ("content::copy_file_dst_exists", copy_file_dst_exists as fn()),
    ("content::read_dir_enumerate", read_dir_enumerate as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    // content::copy_file_ok / copy_file_dst_exists were registered XFAIL
    // PFC-4 on the theory that PFC-4's whole-fd-table drop on the first
    // close plus PFC-7's drop unwrap panics every fs::copy; both XPASSed on
    // target 2026-07-07 (the close-error retcode is silently discarded, not
    // unwrapped -- see the tests' doc comments and PFC-7), so they are now
    // expected to PASS.
];
