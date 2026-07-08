//! Theme `rw`: core read/write/seek behavior, ported from upstream rust's
//! library/std/src/fs/tests.rs (file_test_io_* family) plus xous-specific seek
//! coverage (Start-only matrix, negative Current/End, open-time-length
//! staleness, seek-past-EOF gaps, append mode, zero-length reads). See the
//! `tests` module header (services/pddb-fs-tests/src/tests/mod.rs) for the
//! authoring rules this file follows.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::harness::{TmpDict, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// Ported from upstream `file_test_io_smoke_test`: create+write, open+read,
/// verify content, remove.
pub fn io_smoke_test() {
    let tmp = TmpDict::new("io_smoke_test");
    let path = tmp.path("file");
    let message = "it's alright. have a good time";
    {
        let mut write_stream = check!(File::create(&path));
        check!(write_stream.write(message.as_bytes()));
    }
    {
        let mut read_stream = check!(File::open(&path));
        let mut read_buf = [0; 1028];
        let read_str = match check!(read_stream.read(&mut read_buf)) {
            0 => panic!("shouldn't happen"),
            n => std::str::from_utf8(&read_buf[..n]).unwrap().to_string(),
        };
        assert_eq!(read_str, message);
    }
    check!(fs::remove_file(&path));
}

/// Ported from upstream `file_test_io_non_positional_read`: two sequential
/// reads into disjoint halves of one buffer must land contiguously (no seeks
/// involved, so no PFC-3 exposure).
pub fn io_non_positional_read() {
    let tmp = TmpDict::new("io_non_positional_read");
    let path = tmp.path("file");
    let message: &str = "ten-four";
    let mut read_mem = [0; 8];
    {
        let mut rw_stream = check!(File::create(&path));
        check!(rw_stream.write(message.as_bytes()));
    }
    {
        let mut read_stream = check!(File::open(&path));
        {
            let read_buf = &mut read_mem[0..4];
            check!(read_stream.read(read_buf));
        }
        {
            let read_buf = &mut read_mem[4..8];
            check!(read_stream.read(read_buf));
        }
    }
    check!(fs::remove_file(&path));
    let read_str = std::str::from_utf8(&read_mem).unwrap();
    assert_eq!(read_str, message);
}

/// Ported from upstream `file_test_io_seek_and_tell_smoke_test`. Only a
/// forward `SeekFrom::Start` is used, so this stays outside PFC-3's blast
/// radius and must pass.
pub fn io_seek_and_tell_smoke_test() {
    let tmp = TmpDict::new("io_seek_and_tell_smoke_test");
    let path = tmp.path("file");
    let message = "ten-four";
    let mut read_mem = [0; 4];
    let set_cursor = 4u64;
    let tell_pos_pre_read;
    let tell_pos_post_read;
    {
        let mut rw_stream = check!(File::create(&path));
        check!(rw_stream.write(message.as_bytes()));
    }
    {
        let mut read_stream = check!(File::open(&path));
        check!(read_stream.seek(SeekFrom::Start(set_cursor)));
        tell_pos_pre_read = check!(read_stream.stream_position());
        check!(read_stream.read(&mut read_mem));
        tell_pos_post_read = check!(read_stream.stream_position());
    }
    check!(fs::remove_file(&path));
    let read_str = std::str::from_utf8(&read_mem).unwrap();
    assert_eq!(read_str, &message[4..8]);
    assert_eq!(tell_pos_pre_read, set_cursor);
    assert_eq!(tell_pos_post_read, message.len() as u64);
}

/// Ported faithfully from upstream `file_test_io_seek_and_write` -- this is
/// the maintainer's exact overwrite-idiom symptom report: write, seek back
/// into the middle, write again, and read the whole thing back through a
/// fresh handle. The in-place overwrite lands entirely within the existing
/// length (3 + 10 == 13 == the original length), so it never grows the key
/// and never re-creates it -- small-pool in-place update is the one path
/// PFC-1 does NOT afflict (backend/dictionary.rs ~748-751), so this must
/// pass.
pub fn io_seek_and_write() {
    let tmp = TmpDict::new("io_seek_and_write");
    let path = tmp.path("file");
    let initial_msg = "food-is-yummy";
    let overwrite_msg = "-the-bar!!";
    let final_msg = "foo-the-bar!!";
    let seek_idx = 3;
    let mut read_mem = [0; 13];
    {
        let mut rw_stream = check!(File::create(&path));
        check!(rw_stream.write(initial_msg.as_bytes()));
        check!(rw_stream.seek(SeekFrom::Start(seek_idx)));
        check!(rw_stream.write(overwrite_msg.as_bytes()));
    }
    {
        let mut read_stream = check!(File::open(&path));
        check!(read_stream.read(&mut read_mem));
    }
    check!(fs::remove_file(&path));
    let read_str = std::str::from_utf8(&read_mem).unwrap();
    assert_eq!(read_str, final_msg);
}

/// Ported from upstream `file_test_io_seek_shakedown`. Exercises negative
/// `SeekFrom::End`/`SeekFrom::Current` offsets, which is exactly PFC-3
/// territory: the server casts the offset with `as u64` before
/// `checked_sub`, so any negative seek errors out (services/pddb/src/
/// libstd/mod.rs seek_key/seek_from_point). XFAIL PFC-3.
pub fn io_seek_shakedown() {
    let tmp = TmpDict::new("io_seek_shakedown");
    let path = tmp.path("file");
    //                   01234567890123
    let initial_msg = "qwer-asdf-zxcv";
    let chunk_one: &str = "qwer";
    let chunk_two: &str = "asdf";
    let chunk_three: &str = "zxcv";
    let mut read_mem = [0; 4];
    {
        let mut rw_stream = check!(File::create(&path));
        check!(rw_stream.write(initial_msg.as_bytes()));
    }
    {
        let mut read_stream = check!(File::open(&path));

        check!(read_stream.seek(SeekFrom::End(-4)));
        check!(read_stream.read(&mut read_mem));
        assert_eq!(std::str::from_utf8(&read_mem).unwrap(), chunk_three);

        check!(read_stream.seek(SeekFrom::Current(-9)));
        check!(read_stream.read(&mut read_mem));
        assert_eq!(std::str::from_utf8(&read_mem).unwrap(), chunk_two);

        check!(read_stream.seek(SeekFrom::Start(0)));
        check!(read_stream.read(&mut read_mem));
        assert_eq!(std::str::from_utf8(&read_mem).unwrap(), chunk_one);
    }
    check!(fs::remove_file(&path));
}

/// Ported from upstream `file_test_io_eof`: a freshly-created, never-written
/// file reads back 0 bytes, repeatedly, at EOF. No seeks, no growth -- must
/// pass. Fixture deviation from upstream (empirical, cold run 2026-07-07):
/// `.create(true)` is added alongside `create_new` because on xous
/// `create_new` alone can never create a missing file (PFC-10; the open
/// errored and this test failed before reaching its EOF subject). PFC-10
/// itself is pinned by openflags::create_new_creates_missing.
pub fn io_eof() {
    let tmp = TmpDict::new("io_eof");
    let path = tmp.path("file");
    let mut buf = [0; 256];
    {
        let oo = OpenOptions::new().create(true).create_new(true).write(true).read(true).clone();
        let mut rw = check!(oo.open(&path));
        assert_eq!(check!(rw.read(&mut buf)), 0);
        assert_eq!(check!(rw.read(&mut buf)), 0);
    }
    check!(fs::remove_file(&path));
}

/// Xous-specific: a matrix of `SeekFrom::Start`-only seeks (0, mid, last
/// byte, exactly at EOF). Never goes through the `by < 0` branch of
/// seek_from_point, so this must pass regardless of PFC-3.
pub fn seek_start_matrix() {
    let tmp = TmpDict::new("seek_start_matrix");
    let path = tmp.path("file");
    let content = b"0123456789ABCDEF"; // 16 bytes, well under the 4 KiB hazard line
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(content));
    }
    let mut f = check!(File::open(&path));
    for &start in &[0u64, 1, 8, 15, 16] {
        assert_eq!(check!(f.seek(SeekFrom::Start(start))), start);
        assert_eq!(check!(f.stream_position()), start);
        let mut buf = [0u8; 4];
        let n = check!(f.read(&mut buf));
        let expect_n = (content.len() as u64 - start).min(4) as usize;
        assert_eq!(n, expect_n, "read length mismatch seeking to Start({start})");
        assert_eq!(&buf[..n], &content[start as usize..start as usize + n]);
    }
    drop(f);
    check!(fs::remove_file(&path));
}

/// Xous-specific: negative `SeekFrom::End`/`SeekFrom::Current` offsets in
/// isolation (End coverage that smoke::seek_negative_current doesn't
/// exercise). Correct POSIX behavior: `End(-3)` on a 10-byte file lands at 7;
/// a subsequent `Current(-5)` from 10 lands at 5. XFAIL PFC-3: the server's
/// `by as u64` cast before `checked_sub` makes every negative offset error.
pub fn seek_negative_offsets() {
    let tmp = TmpDict::new("seek_negative_offsets");
    let path = tmp.path("file");
    let content = b"abcdefghij"; // 10 bytes
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(content));
    }
    let mut f = check!(File::open(&path));
    let pos = check!(f.seek(SeekFrom::End(-3)));
    assert_eq!(pos, 7);
    let mut buf = [0u8; 3];
    check!(f.read_exact(&mut buf));
    assert_eq!(&buf, b"hij");

    let pos = check!(f.seek(SeekFrom::Current(-5)));
    assert_eq!(pos, 5);
    let mut buf2 = [0u8; 5];
    check!(f.read_exact(&mut buf2));
    assert_eq!(&buf2, b"fghij");
    drop(f);
    check!(fs::remove_file(&path));
}

/// Xous-specific: open-time-length staleness (PFC-5). Confirmed by reading
/// services/pddb/src/libstd/mod.rs: `write_key` only ever advances
/// `file.offset`, never `file.length`; `seek_key`'s `SeekFrom::End` branch
/// seeks from `file.length` (the length captured at *open* time). So on a
/// freshly-created (open-time length 0) handle, writing bytes through that
/// SAME handle and then asking `SeekFrom::End(0)` must -- per POSIX -- report
/// the file's current true end, but the server instead reports the stale
/// open-time value. XFAIL PFC-5.
pub fn seek_end_after_write_staleness() {
    let tmp = TmpDict::new("seek_end_after_write_staleness");
    let path = tmp.path("file");
    let mut f = check!(OpenOptions::new().read(true).write(true).create(true).open(&path));
    check!(f.write_all(b"hello")); // grows the key to 5 bytes on disk
    let pos = check!(f.seek(SeekFrom::End(0)));
    assert_eq!(
        pos, 5,
        "SeekFrom::End(0) must reflect the file's CURRENT end after writes \
         through this same handle, not the length captured when it was opened"
    );
    drop(f);
    // A freshly-opened handle picks up the true persisted length correctly --
    // this half is not the bug, and pins down that the data itself is intact.
    let mut f2 = check!(File::open(&path));
    assert_eq!(check!(f2.seek(SeekFrom::End(0))), 5);
    check!(f2.seek(SeekFrom::Start(0)));
    let mut buf = Vec::new();
    check!(f2.read_to_end(&mut buf));
    assert_eq!(&buf[..], b"hello");
    drop(f2);
    check!(fs::remove_file(&path));
}

/// Xous-specific: seek past the current EOF, write past the gap, and read
/// the whole thing back through a fresh handle. Confirmed by reading
/// backend/dictionary.rs `key_update`: extending a small-pool key zero-fills
/// the vector out to `offset` before splicing in the new bytes, so the gap
/// must read back as zeros (POSIX sparse-file semantics) -- and because this
/// growth's `kcache.len` update is the *grow* path (not the truncate path),
/// it is unaffected by PFC-1. Must pass.
pub fn seek_past_eof_write_gap() {
    let tmp = TmpDict::new("seek_past_eof_write_gap");
    let path = tmp.path("file");
    {
        let mut f = check!(File::create(&path)); // empty key, length 0
        check!(f.seek(SeekFrom::Start(10)));
        check!(f.write_all(b"end"));
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), 13, "expected a 10-byte gap plus 3 written bytes");
    assert_eq!(&readback[..10], &[0u8; 10], "the seeked-past gap must read back as zeros");
    assert_eq!(&readback[10..], b"end");
    check!(fs::remove_file(&path));
}

/// Xous-specific: append mode across multiple writes through one handle,
/// extending a pre-existing small file (never truncating it -- append-mode
/// open must not truncate; services/pddb/src/libstd/mod.rs open_key sets
/// `offset: if append { len } else { 0 }` and takes no truncate branch for a
/// plain append open). Total size stays well under 4 KiB.
pub fn append_mode_multi_write() {
    let tmp = TmpDict::new("append_mode_multi_write");
    let path = tmp.path("file");
    let base = b"BASE-";
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(base));
    }
    assert_eq!(&read_back(&path)[..], base);

    {
        let mut f = check!(OpenOptions::new().append(true).open(&path));
        check!(f.write_all(b"AAA"));
        check!(f.write_all(b"BBB"));
        check!(f.write_all(b"CCC"));
    }
    let expected = [base.as_slice(), b"AAA", b"BBB", b"CCC"].concat();
    assert!(expected.len() < 4096, "test data must stay under the 4 KiB large-pool hazard line");
    assert_eq!(read_back(&path), expected);
    check!(fs::remove_file(&path));
}

/// Xous-specific edge case: reading into a zero-length buffer must return
/// `Ok(0)` immediately (the `Read` trait's contract, independent of the
/// underlying file), and reading a genuinely empty file into a normal buffer
/// is plain EOF (`Ok(0)`), repeatably.
pub fn read_zero_length_buffer_and_empty_file() {
    let tmp = TmpDict::new("read_zero_length_buffer_and_empty_file");
    let path = tmp.path("file");
    {
        check!(File::create(&path)); // created, never written: a genuinely empty key
    }
    let mut f = check!(File::open(&path));
    let mut empty_buf: [u8; 0] = [];
    assert_eq!(check!(f.read(&mut empty_buf)), 0);
    let mut buf = [0u8; 16];
    assert_eq!(check!(f.read(&mut buf)), 0);
    assert_eq!(check!(f.read(&mut buf)), 0, "reading past EOF again must still be Ok(0), not an error");
    drop(f);
    check!(fs::remove_file(&path));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("rw::io_smoke_test", io_smoke_test as fn()),
    ("rw::io_non_positional_read", io_non_positional_read as fn()),
    ("rw::io_seek_and_tell_smoke_test", io_seek_and_tell_smoke_test as fn()),
    ("rw::io_seek_and_write", io_seek_and_write as fn()),
    ("rw::io_seek_shakedown", io_seek_shakedown as fn()),
    ("rw::io_eof", io_eof as fn()),
    ("rw::seek_start_matrix", seek_start_matrix as fn()),
    ("rw::seek_negative_offsets", seek_negative_offsets as fn()),
    ("rw::seek_end_after_write_staleness", seek_end_after_write_staleness as fn()),
    ("rw::seek_past_eof_write_gap", seek_past_eof_write_gap as fn()),
    ("rw::append_mode_multi_write", append_mode_multi_write as fn()),
    ("rw::read_zero_length_buffer_and_empty_file", read_zero_length_buffer_and_empty_file as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[
    ("rw::io_seek_shakedown", "PFC-3"),
    ("rw::seek_negative_offsets", "PFC-3"),
    ("rw::seek_end_after_write_staleness", "PFC-5"),
];
