//! Seed tests exercising the harness machinery and pinning known PFC bugs.
//! All assertions state the CORRECT behavior; known failures are registered in
//! the XFAILS table in mod.rs — never weaken an assertion here.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::harness::{TmpDict, XorShift, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// Create a file, write, read back, remove, and verify it is gone.
pub fn create_write_read() {
    let tmp = TmpDict::new("create_write_read");
    let path = tmp.path("hello");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"hello pddb"));
    }
    assert_eq!(&read_back(&path)[..], b"hello pddb");
    check!(fs::remove_file(&path));
    assert!(File::open(&path).is_err(), "file was openable after remove_file");
}

/// `File::create` over an existing small-pool key must truncate: a shorter
/// second write leaves exactly the new content.
pub fn overwrite_shorter_small() {
    let tmp = TmpDict::new("overwrite_shorter_small");
    let path = tmp.path("small");
    overwrite_shorter_inner(&path, 100);
    check!(fs::remove_file(&path));
}

/// Same as overwrite_shorter_small, but the first write lands the key in the
/// large pool (>= ~4 KiB). PFC-1 territory (large-key truncate is a no-op on
/// length) -- but empirically (harness pilot) the truncating re-create doesn't
/// just leave stale data, it PANICS the pddb server: `Option::unwrap()` on
/// None at services/pddb/src/backend/types.rs:109 (`PageAlignedVa::from(0)`),
/// killing pddb's main thread (main.rs:528). DISABLED in tests::TESTS until
/// that code path is fixed; kept compiling so it is one line to re-register.
#[allow(dead_code)]
pub fn overwrite_shorter_large() {
    let tmp = TmpDict::new("overwrite_shorter_large");
    let path = tmp.path("large");
    overwrite_shorter_inner(&path, 8192);
    check!(fs::remove_file(&path));
}

fn overwrite_shorter_inner(path: &str, first_len: usize) {
    // Step logging (log::info is ignored by the sentinel parser): the large
    // variant crashes the WHOLE pddb server (PageAlignedVa::from(0) unwrap,
    // backend/types.rs:109) and these markers pin down the killing request.
    let mut rng = XorShift::new(first_len as u32);
    let mut first = vec![0u8; first_len];
    rng.fill(&mut first);
    {
        log::info!("overwrite_shorter({}): first create", first_len);
        let mut f = check!(File::create(path));
        log::info!("overwrite_shorter({}): first write", first_len);
        check!(f.write_all(&first));
    }
    log::info!("overwrite_shorter({}): first read_back", first_len);
    assert_eq!(read_back(path), first, "first write did not read back intact");

    // second write: 10 bytes, each guaranteed to differ from the old content
    let second: Vec<u8> = first[..10].iter().map(|&b| !b).collect();
    {
        log::info!("overwrite_shorter({}): truncating re-create", first_len);
        let mut f = check!(File::create(path)); // must truncate the existing key
        log::info!("overwrite_shorter({}): second write", first_len);
        check!(f.write_all(&second));
    }
    log::info!("overwrite_shorter({}): second read_back", first_len);
    let readback = read_back(path);
    assert_eq!(
        readback.len(),
        second.len(),
        "expected exactly {} bytes after truncating overwrite, got {}",
        second.len(),
        readback.len()
    );
    assert_eq!(readback, second, "truncating overwrite did not read back intact");
}

/// SeekFrom::Current with a negative offset must rewind. XFAIL PFC-3: the
/// server casts the offset with `as u64`, so any negative seek errors out.
pub fn seek_negative_current() {
    let tmp = TmpDict::new("seek_negative_current");
    let path = tmp.path("seek");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"0123456789"));
    }
    let mut f = check!(File::open(&path));
    assert_eq!(check!(f.seek(SeekFrom::Start(10))), 10);
    assert_eq!(check!(f.seek(SeekFrom::Current(-5))), 5);
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    assert_eq!(&buf[..], b"56789");
    drop(f);
    check!(fs::remove_file(&path));
}

/// Closing one file must not affect other open handles in the same process.
/// XFAIL PFC-4: CloseKeyStd drops the whole per-process fd table, killing B's
/// handle when A closes.
///
/// Ordering is load-bearing: all of B's I/O results are collected first and B
/// is dropped explicitly BEFORE any panic. With PFC-4 live, B's fd is dead
/// after A closes, and PFC-7 makes `File::drop` panic on a failed close -- if
/// that drop ran during panic-unwind (e.g. `check!` firing while B is still
/// in scope) it would be a fatal double panic that aborts the whole runner.
/// Dropped in a normal context, the drop panic is caught like any other.
pub fn two_files_close_one() {
    let tmp = TmpDict::new("two_files_close_one");
    let path_a = tmp.path("a");
    let path_b = tmp.path("b");
    let a = check!(File::create(&path_a));
    let mut b = check!(OpenOptions::new().read(true).write(true).create(true).open(&path_b));
    drop(a);
    let write_res = b.write_all(b"still alive");
    let seek_res = b.seek(SeekFrom::Start(0));
    let mut buf = Vec::new();
    let read_res = b.read_to_end(&mut buf);
    drop(b); // may panic itself (PFC-7 close unwrap); catchable here
    check!(write_res);
    assert_eq!(check!(seek_res), 0);
    check!(read_res);
    assert_eq!(&buf[..], b"still alive");
    check!(fs::remove_file(&path_a));
    check!(fs::remove_file(&path_b));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("smoke::create_write_read", create_write_read as fn()),
    ("smoke::overwrite_shorter_small", overwrite_shorter_small as fn()),
    ("smoke::seek_negative_current", seek_negative_current as fn()),
    ("smoke::two_files_close_one", two_files_close_one as fn()),
    ("smoke::create_new_existing", create_new_existing as fn()),
    // DISABLED, not XFAIL: smoke::overwrite_shorter_large PANICS THE PDDB
    // SERVER (PageAlignedVa::from(0) unwrap, services/pddb/src/backend/
    // types.rs:109, killing pddb's main thread) at the truncating re-create
    // of an existing 8 KiB key -- harness pilot, 2026-07-07. A dead server
    // hangs every subsequent test, so it cannot be in the table until the
    // PFC-1 code path is fixed; re-register it (as XFAIL first) with the fix.
];

pub const XFAILS: &[(&str, &str)] = &[
    // smoke::overwrite_shorter_large (PFC-1) is not here: it crashes the pddb
    // server outright and is disabled above.
    ("smoke::seek_negative_current", "PFC-3"),
    ("smoke::two_files_close_one", "PFC-4"),
];

/// `create_new` on an existing path must fail. Don't assert the ErrorKind: the
/// xous backend surfaces the collision as an internal DiskFull retcode that
/// maps to ErrorKind::Other.
pub fn create_new_existing() {
    let tmp = TmpDict::new("create_new_existing");
    let path = tmp.path("exists");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"x"));
    }
    assert_eq!(&read_back(&path)[..], b"x");
    let r = OpenOptions::new().create_new(true).write(true).open(&path);
    assert!(r.is_err(), "create_new on an existing path unexpectedly succeeded");
    check!(fs::remove_file(&path));
}
