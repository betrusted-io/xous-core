//! Theme: openflags -- the `OpenOptions` matrix (create / create_new / append /
//! truncate, crossed with path-missing / path-existing-small). This is the
//! highest-value theme: the maintainer's create/overwrite complaint lives here.
//!
//! Ground truth for the "what actually happens" characterization tests below
//! was read directly out of the server implementation in this checkout
//! (services/pddb/src/libstd/mod.rs `open_key`/`write_key`/`read_key`, and
//! `struct FileHandle` in services/pddb/src/main.rs), not guessed: the KyOQ
//! open request only ever carries `create_file`/`create_path`/`create_new`/
//! `append`/`truncate`/`alloc_hint`/`cb_sid` -- there is no read/write bit on
//! the wire at all, and `FileHandle` has no access-mode field for `write_key`/
//! `read_key` to consult. This corroborates the documented claim that
//! `OpenOptions::read()/write()` are silently dropped client-side.
//!
//! SERVER-CRASH HAZARD: every file below is small
//! (well under 4 KiB) even across repeated truncating re-creates.

#![allow(unused_imports)]
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};

use crate::harness::{TmpDict, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

// ---------------------------------------------------------------------------
// The matrix: {create, create_new, append, truncate} x {missing, existing-small}
// ---------------------------------------------------------------------------

/// `create(true)` alone on a missing path: POSIX O_CREAT makes a new empty
/// file, and the write below lands. (Access-mode enforcement is not what this
/// test is about -- see `write_succeeds_through_read_only_handle` for that.)
pub fn create_missing_creates_and_writes() {
    let tmp = TmpDict::new("create_missing");
    let path = tmp.path("f");
    {
        let mut f = check!(OpenOptions::new().create(true).open(&path));
        check!(f.write_all(b"created"));
    }
    assert_eq!(&read_back(&path)[..], b"created");
    check!(fs::remove_file(&path));
}

/// `create(true)` WITHOUT `truncate(true)` on an existing file must NOT clear
/// it -- POSIX: O_CREAT alone on an existing path is a no-op open, not O_TRUNC.
pub fn create_existing_preserves_content_without_truncate() {
    let tmp = TmpDict::new("create_existing_no_trunc");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"keepme"));
    }
    assert_eq!(&read_back(&path)[..], b"keepme");
    {
        let _f = check!(OpenOptions::new().create(true).open(&path));
    }
    assert_eq!(&read_back(&path)[..], b"keepme", "create(true) alone must not truncate an existing file");
    check!(fs::remove_file(&path));
}

/// `create_new(true)` on a missing path creates it (correct std semantics:
/// `create_new` implies creation, like O_CREAT|O_EXCL). XFAIL PFC-10,
/// empirically confirmed on the Renode image 2026-07-07: the rust fork
/// serializes `create_file` and `create_new` as independent booleans and never
/// combines them, while the server's key-creation branch only runs when
/// `create_file` is set -- so `create_new` WITHOUT `.create(true)` falls
/// through every basis and the open errors ("unable to find key ..."). See
/// PFC-10.
pub fn create_new_creates_missing() {
    let tmp = TmpDict::new("create_new_missing");
    let path = tmp.path("f");
    {
        let mut f = check!(OpenOptions::new().create_new(true).open(&path));
        check!(f.write_all(b"fresh"));
    }
    assert_eq!(&read_back(&path)[..], b"fresh");
    check!(fs::remove_file(&path));
}

/// `create_new(true)` on an existing path must fail and must not disturb the
/// existing content. Don't assert a specific ErrorKind: the collision surfaces
/// as an internal DiskFull retcode that the client maps to ErrorKind::Other.
pub fn create_new_fails_on_existing() {
    let tmp = TmpDict::new("create_new_existing");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"x"));
    }
    assert_eq!(&read_back(&path)[..], b"x");
    let r = OpenOptions::new().create_new(true).open(&path);
    assert!(r.is_err(), "create_new on an existing path unexpectedly succeeded");
    assert_eq!(&read_back(&path)[..], b"x", "a failed create_new must not disturb existing content");
    check!(fs::remove_file(&path));
}

/// `truncate(true)` on an existing file clears it before the subsequent write
/// lands -- verified by content read-back (never by metadata: PFC-5 makes
/// `metadata().len()` always 0, and separately the returned/open-time length
/// this server keeps is stale pre-truncate -- see PFC-2 below -- but the
/// *actual key bytes* are genuinely replaced, which is all a read-back checks).
pub fn truncate_flag_on_existing_clears_and_writes() {
    let tmp = TmpDict::new("truncate_existing");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"stale-content-here"));
    }
    assert_eq!(&read_back(&path)[..], b"stale-content-here");
    {
        let mut f = check!(OpenOptions::new().truncate(true).open(&path));
        check!(f.write_all(b"new"));
    }
    assert_eq!(&read_back(&path)[..], b"new", "truncate(true) must clear old content before the new write");
    check!(fs::remove_file(&path));
}

/// `truncate(true)` WITHOUT `create(true)` on a path that does not exist must
/// fail -- POSIX: O_TRUNC never implies O_CREAT, so there is nothing to
/// truncate. Server-side: `open_key` only auto-vivifies a missing key when
/// `create_file` (create/create_new) is set; a plain `truncate` bit does not
/// set it (services/pddb/src/libstd/mod.rs open_key ~319-346).
pub fn truncate_flag_alone_on_missing_fails() {
    let tmp = TmpDict::new("truncate_missing");
    let path = tmp.path("f");
    let r = OpenOptions::new().truncate(true).open(&path);
    assert!(r.is_err(), "truncate-only open of a missing path unexpectedly succeeded");
}

/// `append(true)` alone (no `create`) on a missing path must fail -- POSIX:
/// O_APPEND without O_CREAT on a nonexistent file is ENOENT. `append(true)`
/// WITH `create(true)` on a missing path creates it empty and the write lands
/// at offset 0 (a brand new key has no "existing content" to land after).
pub fn append_requires_create_to_make_new_file() {
    let tmp = TmpDict::new("append_requires_create");
    let path = tmp.path("f");
    let r = OpenOptions::new().append(true).open(&path);
    assert!(r.is_err(), "append-only open of a missing path unexpectedly succeeded");
    {
        let mut f = check!(OpenOptions::new().append(true).create(true).open(&path));
        check!(f.write_all(b"abc"));
    }
    assert_eq!(&read_back(&path)[..], b"abc");
    check!(fs::remove_file(&path));
}

/// `append(true)` on an existing file: the write must land after the existing
/// content, not clobber it.
pub fn append_on_existing_appends_after_content() {
    let tmp = TmpDict::new("append_on_existing");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"hello"));
    }
    assert_eq!(&read_back(&path)[..], b"hello");
    {
        let mut f = check!(OpenOptions::new().append(true).open(&path));
        check!(f.write_all(b" world"));
    }
    assert_eq!(&read_back(&path)[..], b"hello world");
    check!(fs::remove_file(&path));
}

/// `create(true).truncate(true)` on an existing file (the "create and
/// truncate" row of the upstream `open_flavors` matrix, and what
/// `File::create` itself does): must truncate and leave exactly the new
/// content.
pub fn create_and_truncate_on_existing() {
    let tmp = TmpDict::new("create_and_truncate");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"0123456789old"));
    }
    assert_eq!(&read_back(&path)[..], b"0123456789old");
    {
        let mut f = check!(OpenOptions::new().create(true).truncate(true).open(&path));
        check!(f.write_all(b"new-short"));
    }
    assert_eq!(&read_back(&path)[..], b"new-short");
    check!(fs::remove_file(&path));
}

// ---------------------------------------------------------------------------
// Divergence characterization: xous drops read()/write() client-side.
// Upstream's `open_flavors`/`test_open_options_invalid_combinations` validate
// access-mode vs. create/truncate combinations purely in platform sys code
// ("creating or truncating a file requires write or append access", "must
// specify at least one of read, write, or append access") before ever
// touching the OS. The xous fork's sys/fs implementation performs no such
// validation -- these are documented xous semantics (not bugs), asserted as
// what actually happens per the XFAIL discipline.
// ---------------------------------------------------------------------------

/// `truncate(true)` with only `read(true)` set (no `write`) is accepted and
/// genuinely truncates on xous, diverging from upstream's client-side
/// rejection. Documented xous semantic (OpenOptions read()/write() flags are
/// silently dropped client-side; corroborated by `open_key`'s wire format
/// carrying no read/write field at all).
pub fn truncate_without_write_flag_still_truncates() {
    let tmp = TmpDict::new("truncate_no_write");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"old-data"));
    }
    assert_eq!(&read_back(&path)[..], b"old-data");
    let f = check!(OpenOptions::new().truncate(true).read(true).open(&path));
    drop(f);
    assert_eq!(
        &read_back(&path)[..],
        b"",
        "truncate(true) with only read(true) set should still truncate on xous"
    );
    check!(fs::remove_file(&path));
}

/// Writing through a handle opened with only `read(true)` set succeeds on
/// xous: `FileHandle` (services/pddb/src/main.rs) carries no access-mode bit,
/// and `write_key`/`read_key` never consult one. Documented xous semantic, not
/// a bug -- diverges from POSIX where this would be EBADF.
pub fn write_succeeds_through_read_only_handle() {
    let tmp = TmpDict::new("write_through_read_only");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"orig"));
    }
    let mut f = check!(OpenOptions::new().read(true).open(&path));
    check!(f.write_all(b"changed"));
    drop(f);
    assert_eq!(
        &read_back(&path)[..],
        b"changed",
        "write through a read(true)-only handle should succeed on xous"
    );
    check!(fs::remove_file(&path));
}

/// `append(true).truncate(true)` together: upstream rejects this combination
/// client-side ("invalid_options") before ever reaching the OS; xous performs
/// no such validation and forwards both bits to the server. XFAIL PFC-2: in
/// `open_key`, the pre-truncate key length is captured into `len` (~mod.rs
/// L335-341) *before* the truncate branch physically empties the key
/// (~L377-399), and `FileHandle.offset` is then seeded from that stale `len`
/// because `append` is set (~L407). The follow-up write therefore lands at
/// the old (pre-truncate) offset instead of 0, leaving the physically-emptied
/// key's start zero-padded up to that stale offset -- POSIX-correct behavior
/// (truncate empties the file, so a single subsequent write is the entire new
/// content) is asserted here and is expected to fail until PFC-2 is fixed.
pub fn append_and_truncate_together_stale_offset() {
    let tmp = TmpDict::new("append_and_truncate_together");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"0123456789"));
    }
    assert_eq!(&read_back(&path)[..], b"0123456789");
    {
        let mut f = check!(OpenOptions::new().append(true).truncate(true).open(&path));
        check!(f.write_all(b"XY"));
    }
    assert_eq!(
        &read_back(&path)[..],
        b"XY",
        "truncate-then-write under append+truncate should leave exactly the new bytes"
    );
    check!(fs::remove_file(&path));
}

/// `create_new(true).create(true)` together: upstream docs say create_new
/// makes create/truncate moot. On a missing path the combo still creates the
/// file (create_new's own semantics); on an existing path create_new must
/// still win and fail even with create(true) also set (open_key checks
/// create_new -- and errors unconditionally when the key exists -- before it
/// ever looks at the truncate bit).
pub fn create_new_and_create_together() {
    let tmp = TmpDict::new("create_new_and_create");
    let missing = tmp.path("missing");
    let existing = tmp.path("existing");
    {
        let mut f = check!(File::create(&existing));
        check!(f.write_all(b"present"));
    }
    assert_eq!(&read_back(&existing)[..], b"present");

    {
        let mut f = check!(OpenOptions::new().create_new(true).create(true).open(&missing));
        check!(f.write_all(b"made"));
    }
    assert_eq!(&read_back(&missing)[..], b"made");

    let r = OpenOptions::new().create_new(true).create(true).open(&existing);
    assert!(r.is_err(), "create_new+create on an existing path unexpectedly succeeded");
    assert_eq!(
        &read_back(&existing)[..],
        b"present",
        "a failed create_new+create must not disturb existing content"
    );

    check!(fs::remove_file(&missing));
    check!(fs::remove_file(&existing));
}

/// Double (then triple) `File::create` on the same path in sequence: each
/// call must truncate whatever the previous call (or write) left behind,
/// including truncating to empty when the final create has no follow-up
/// write at all.
pub fn double_create_truncates_each_time() {
    let tmp = TmpDict::new("double_create");
    let path = tmp.path("f");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"first-content"));
    }
    assert_eq!(&read_back(&path)[..], b"first-content");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"second"));
    }
    assert_eq!(&read_back(&path)[..], b"second", "second File::create must truncate the first content");
    {
        let _f = check!(File::create(&path)); // no follow-up write this time
    }
    assert_eq!(&read_back(&path)[..], b"", "a third File::create with no write must leave the key empty");
    check!(fs::remove_file(&path));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("openflags::create_missing_creates_and_writes", create_missing_creates_and_writes as fn()),
    (
        "openflags::create_existing_preserves_content_without_truncate",
        create_existing_preserves_content_without_truncate as fn(),
    ),
    ("openflags::create_new_creates_missing", create_new_creates_missing as fn()),
    ("openflags::create_new_fails_on_existing", create_new_fails_on_existing as fn()),
    (
        "openflags::truncate_flag_on_existing_clears_and_writes",
        truncate_flag_on_existing_clears_and_writes as fn(),
    ),
    ("openflags::truncate_flag_alone_on_missing_fails", truncate_flag_alone_on_missing_fails as fn()),
    ("openflags::append_requires_create_to_make_new_file", append_requires_create_to_make_new_file as fn()),
    ("openflags::append_on_existing_appends_after_content", append_on_existing_appends_after_content as fn()),
    ("openflags::create_and_truncate_on_existing", create_and_truncate_on_existing as fn()),
    (
        "openflags::truncate_without_write_flag_still_truncates",
        truncate_without_write_flag_still_truncates as fn(),
    ),
    ("openflags::write_succeeds_through_read_only_handle", write_succeeds_through_read_only_handle as fn()),
    (
        "openflags::append_and_truncate_together_stale_offset",
        append_and_truncate_together_stale_offset as fn(),
    ),
    ("openflags::create_new_and_create_together", create_new_and_create_together as fn()),
    ("openflags::double_create_truncates_each_time", double_create_truncates_each_time as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    ("openflags::append_and_truncate_together_stale_offset", "PFC-2"),
    ("openflags::create_new_creates_missing", "PFC-10"),
];
