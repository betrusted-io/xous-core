//! Theme `sizes`: allocation-pool boundaries and size-driven paths.
//!
//! Ground truth (services/pddb/src/backend), all verified by reading the
//! actual source (not assumed):
//!
//! - `VPAGE_SIZE = PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<JournalType>()`
//!   (backend/hw.rs:60). `PAGE_SIZE = SPINOR_ERASE_SIZE = 0x1000 = 4096` (services/spinor/src/api.rs:6).
//!   `Nonce`/`Tag` are the AES-256-GCM-SIV crate's `GenericArray<u8, U12>` / `GenericArray<u8, U16>`
//!   (aes-gcm-siv 0.11.1 src/lib.rs:105,108,159,160 -- 12 and 16 bytes). `JournalType = u32`
//!   (backend/types.rs:62 -- 4 bytes). So VPAGE_SIZE = 4096 - 12 - 16 - 4 = **4064**.
//! - `SMALL_CAPACITY: usize = VPAGE_SIZE` (backend/basis.rs:160).
//! - The small/large pool decision for a *fresh* key (backend/dictionary.rs ~920-923, the `key_update` branch
//!   taken when the key does not already exist): `if ((data.len() + offset) < SMALL_CAPACITY) &&
//!   (alloc_hint... < SMALL_CAPACITY) { /* small pool */ } else { /* large pool */ }`. The comparison is a
//!   strict `<`, so a fresh key of exactly SMALL_CAPACITY (4064) bytes is NOT `< SMALL_CAPACITY` and lands in
//!   the **large** pool; 4063 bytes is the largest size that stays **small**.
//! - Growing an *existing* small-pool key past its reservation does not consult size at all -- `key_update`
//!   (backend/dictionary.rs ~595-644) detects `kcache.reserved < (data.len() + offset)`, extracts the key's
//!   full current content, removes it, and recurses with the complete data at offset 0, at which point the
//!   *fresh-key* branch above decides the pool again. This is the internal small->large "graduation" path
//!   exercised by several tests below; it is a `key_update` growth, not the user-level
//!   `File::create`/`.truncate(true)`-over-an-existing-key SERVER-CRASH HAZARD (PFC-1) -- no test here ever
//!   truncates an existing key in place.

#![allow(unused_imports)]
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::harness::{TmpDict, XorShift, check};

/// Small-pool / large-pool threshold for a fresh key, see the module doc
/// comment above for the derivation and exact source citations.
const SMALL_CAPACITY: usize = 4064;

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// Exact boundary probe: a fresh key of `SMALL_CAPACITY - 1` (4063) bytes --
/// the largest size that still lands in the small pool (`4063 <
/// SMALL_CAPACITY` is true). Fresh path, never re-created.
pub fn boundary_below_threshold() {
    let tmp = TmpDict::new("boundary_below_threshold");
    let path = tmp.path("below");
    let len = SMALL_CAPACITY - 1;
    let mut rng = XorShift::new(0xB0F0_0001);
    let mut content = vec![0u8; len];
    rng.fill(&mut content);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&content));
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), len, "small-pool boundary (threshold-1) length mismatch");
    assert_eq!(readback, content, "small-pool boundary (threshold-1) content mismatch");
    check!(fs::remove_file(&path));
}

/// Exact boundary probe: a fresh key of exactly `SMALL_CAPACITY` (4064)
/// bytes. `4064 < SMALL_CAPACITY` is false, so per dictionary.rs ~922 this is
/// the *first* size that lands in the large pool. Fresh path, never
/// re-created.
pub fn boundary_at_threshold() {
    let tmp = TmpDict::new("boundary_at_threshold");
    let path = tmp.path("at");
    let len = SMALL_CAPACITY;
    let mut rng = XorShift::new(0xB0F0_0002);
    let mut content = vec![0u8; len];
    rng.fill(&mut content);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&content));
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), len, "small-pool boundary (threshold) length mismatch");
    assert_eq!(readback, content, "small-pool boundary (threshold) content mismatch");
    check!(fs::remove_file(&path));
}

/// Exact boundary probe: a fresh key of `SMALL_CAPACITY + 1` (4065) bytes --
/// comfortably in the large pool. Fresh path, never re-created.
pub fn boundary_above_threshold() {
    let tmp = TmpDict::new("boundary_above_threshold");
    let path = tmp.path("above");
    let len = SMALL_CAPACITY + 1;
    let mut rng = XorShift::new(0xB0F0_0003);
    let mut content = vec![0u8; len];
    rng.fill(&mut content);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&content));
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), len, "small-pool boundary (threshold+1) length mismatch");
    assert_eq!(readback, content, "small-pool boundary (threshold+1) content mismatch");
    check!(fs::remove_file(&path));
}

/// A key created but never written must read back as a genuinely empty (0
/// byte) file, repeatably, and disappear cleanly on delete.
pub fn zero_byte_file() {
    let tmp = TmpDict::new("zero_byte_file");
    let path = tmp.path("empty");
    {
        check!(File::create(&path)); // create only, no write
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), 0, "freshly created, never-written key must read back empty");
    check!(fs::remove_file(&path));
    assert!(File::open(&path).is_err(), "file still openable after remove_file");
}

/// A single-byte file: smallest possible non-empty small-pool key.
pub fn one_byte_file() {
    let tmp = TmpDict::new("one_byte_file");
    let path = tmp.path("one");
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&[0x5Au8]));
    }
    let readback = read_back(&path);
    assert_eq!(readback, vec![0x5Au8], "single-byte file content mismatch");
    check!(fs::remove_file(&path));
}

/// Growth WITHIN one open handle from small to large pool via sequential
/// writes: 100 B then +8 KiB, both through the same still-open `File`. The
/// second write's cumulative size (100 + 8192 = 8292 B) exceeds
/// SMALL_CAPACITY, which `key_update` handles by extracting the small-pool
/// key's full content, removing it, and recursing with the complete data at
/// offset 0 -- landing in the large pool (backend/dictionary.rs ~595-644,
/// the "update/extend" path; see the module doc comment). This is the
/// internal small->large graduation, *not* the user-level truncate/re-create
/// SERVER-CRASH HAZARD (PFC-1) -- the handle here is never closed and
/// reopened with truncate semantics.
pub fn growth_small_to_large_same_handle() {
    let tmp = TmpDict::new("growth_small_to_large_same_handle");
    let path = tmp.path("grow");
    let mut rng = XorShift::new(0x6001);
    let mut first = vec![0u8; 100];
    rng.fill(&mut first);
    let mut second = vec![0u8; 8 * 1024];
    rng.fill(&mut second);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&first)); // 100 B -- small pool
        check!(f.write_all(&second)); // cumulative 8292 B -- crosses into the large pool
    }
    let expected: Vec<u8> = first.iter().chain(second.iter()).copied().collect();
    let readback = read_back(&path);
    assert_eq!(readback.len(), expected.len(), "post-growth length mismatch");
    assert_eq!(readback, expected, "content after small->large growth mismatch");
    check!(fs::remove_file(&path));
}

/// Growth across REOPENS in append mode: a small base file, closed, then
/// reopened with `.append(true)` and extended by 8 KiB. Append never
/// truncates -- services/pddb/src/libstd/mod.rs `open_key` sets `offset: if
/// append { len } else { 0 }` and takes no truncate branch for a plain
/// append open (see also rw::append_mode_multi_write) -- so re-opening the
/// path here is safe even though it already holds content, unlike a
/// `File::create`/`.truncate(true)` re-open of an existing >= 4 KiB key
/// (PFC-1).
pub fn growth_across_reopen_append() {
    let tmp = TmpDict::new("growth_across_reopen_append");
    let path = tmp.path("append_grow");
    let base = b"base-content-for-append-growth-test";
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(base));
    }
    assert_eq!(&read_back(&path)[..], base, "base content mismatch before append");

    let mut rng = XorShift::new(0x7001);
    let mut tail = vec![0u8; 8 * 1024];
    rng.fill(&mut tail);
    {
        let mut f = check!(OpenOptions::new().append(true).open(&path));
        check!(f.write_all(&tail));
    }
    let expected: Vec<u8> = base.iter().chain(tail.iter()).copied().collect();
    let readback = read_back(&path);
    assert_eq!(readback.len(), expected.len(), "post-append length mismatch");
    assert_eq!(readback, expected, "content after append-mode reopen growth mismatch");
    check!(fs::remove_file(&path));
}

/// Shrink via the SAFE pattern: `remove_file` (a full unlink -- never a
/// truncate of an existing key in place) followed by a fresh `File::create`
/// at a smaller size. This is the mandated workaround for
/// the SERVER-CRASH HAZARD (truncating re-create of an existing large-pool
/// key, PFC-1): `path` is fully unlinked before the smaller content is ever
/// written, so the final `File::create` targets a genuinely non-existent
/// key, not a truncate of an existing one.
pub fn shrink_safe_pattern() {
    let tmp = TmpDict::new("shrink_safe_pattern");
    let path = tmp.path("shrink");
    let mut rng = XorShift::new(0x8001);
    let mut big = vec![0u8; 5000]; // large pool (>= SMALL_CAPACITY)
    rng.fill(&mut big);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&big));
    }
    assert_eq!(read_back(&path), big, "initial large content mismatch");

    check!(fs::remove_file(&path)); // full unlink -- no truncate involved
    assert!(File::open(&path).is_err(), "file still openable after remove_file");

    let small = b"tiny-after-shrink"; // small pool, genuinely fresh key
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(small));
    }
    let readback = read_back(&path);
    assert_eq!(readback.len(), small.len(), "shrunk file must be exactly the new (smaller) size");
    assert_eq!(&readback[..], small, "no stale large-pool tail after safe shrink");
    check!(fs::remove_file(&path));
}

/// Shared body for the 64 KiB / 200 KiB size tests: write `total_len` bytes
/// of deterministic XorShift content in 4 KiB chunks, then read it back in
/// 4 KiB chunks, regenerating the expected bytes from a freshly re-seeded
/// XorShift (the generator is a deterministic pure function of its seed and
/// call count, so re-seeding reproduces the exact write sequence) and
/// comparing byte-for-byte against each chunk actually read. This is
/// strictly stronger than a folded checksum and never requires holding the
/// whole file in memory at once -- both the write and read loops only ever
/// hold one ~4 KiB chunk. Both sizes stay far under the 4 MiB smalldb image
/// and the per-test data budget.
fn xorshift_roundtrip(path: &str, total_len: usize, seed: u32, label: &str) {
    const CHUNK: usize = 4096;
    {
        let mut f = check!(File::create(path));
        let mut rng = XorShift::new(seed);
        let mut written = 0usize;
        let mut chunk_no = 0usize;
        while written < total_len {
            let n = CHUNK.min(total_len - written);
            let mut buf = vec![0u8; n];
            rng.fill(&mut buf);
            check!(f.write_all(&buf));
            written += n;
            chunk_no += 1;
            // Console liveness: this loop can run to ~50 iterations (200 KiB
            // / 4 KiB) -- well past the ~10-op threshold, so emit progress
            // every few chunks.
            if chunk_no % 5 == 0 {
                log::info!("{}: wrote {}/{} bytes", label, written, total_len);
            }
        }
    }
    {
        let mut f = check!(File::open(path));
        let mut rng = XorShift::new(seed);
        let mut total_read = 0usize;
        let mut chunk_no = 0usize;
        loop {
            let mut buf = vec![0u8; CHUNK];
            let n = check!(f.read(&mut buf));
            if n == 0 {
                break;
            }
            let mut expected = vec![0u8; n];
            rng.fill(&mut expected);
            assert_eq!(&buf[..n], &expected[..], "{}: chunk mismatch at byte offset {}", label, total_read);
            total_read += n;
            chunk_no += 1;
            if chunk_no % 5 == 0 {
                log::info!("{}: verified {}/{} bytes", label, total_read, total_len);
            }
        }
        assert_eq!(total_read, total_len, "{}: total bytes read back does not match total written", label);
    }
    check!(fs::remove_file(path));
}

/// A 64 KiB large-pool file: XorShift content, chunked write, chunked
/// byte-exact read-back verification, delete after.
pub fn large_file_64kib() {
    let tmp = TmpDict::new("large_file_64kib");
    let path = tmp.path("data64k");
    xorshift_roundtrip(&path, 64 * 1024, 0x0064_1000, "large_file_64kib");
}

/// A 200 KiB large-pool file: XorShift content, chunked write, chunked
/// byte-exact read-back verification, delete after. Stays well inside the
/// 4 MiB smalldb image (services/pddb/src/api.rs PDDB_A_LEN under
/// `pddb/smalldb`).
pub fn large_file_200kib() {
    let tmp = TmpDict::new("large_file_200kib");
    let path = tmp.path("data200k");
    xorshift_roundtrip(&path, 200 * 1024, 0x0200_1000, "large_file_200kib");
}

/// Seek + read at various offsets within a 32 KiB large-pool file, including
/// offsets that straddle VPAGE_SIZE (4064 B, backend/hw.rs:60) internal page
/// boundaries -- e.g. seeking to 4063 and reading 64 bytes touches both the
/// last byte of page 0 and the first 63 bytes of page 1 in a single `read`
/// call. Confirms `seek`/`read` are transparent across the ciphertext page
/// grid: the plaintext file content has no seams a caller can observe.
pub fn seek_read_page_spanning_32kib() {
    let tmp = TmpDict::new("seek_read_page_spanning_32kib");
    let path = tmp.path("pagespan");
    let total_len = 32 * 1024;
    let mut rng = XorShift::new(0x9001);
    let mut content = vec![0u8; total_len];
    rng.fill(&mut content);
    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(&content));
    }
    let mut f = check!(File::open(&path));
    // Offsets deliberately straddle VPAGE_SIZE (4064) multiples: just below,
    // exactly at, and just above the first two page boundaries, plus a
    // mid-file probe and two near-EOF probes.
    let offsets: [usize; 10] = [0, 4063, 4064, 4065, 8127, 8128, 8129, 16000, total_len - 4, total_len - 200];
    for &off in &offsets {
        let pos = check!(f.seek(SeekFrom::Start(off as u64)));
        assert_eq!(pos, off as u64, "seek did not land at the requested offset {off}");
        let mut buf = [0u8; 64];
        let n = check!(f.read(&mut buf));
        let expect_n = (total_len - off).min(64);
        assert_eq!(n, expect_n, "read length mismatch seeking to offset {off}");
        assert_eq!(&buf[..n], &content[off..off + n], "content mismatch seeking to offset {off}");
    }
    drop(f);
    check!(fs::remove_file(&path));
}

/// Write at an offset spanning the small/large pool boundary within ONE open
/// handle: create an empty key (small pool, reservation 1 byte -- see the
/// module doc comment), seek to `SMALL_CAPACITY - 64` (4000), and issue a
/// SINGLE `write` call whose byte range `[4000, 4128)` straddles
/// SMALL_CAPACITY (4064). Because this write's cumulative size (4000 + 128 =
/// 4128) exceeds the tiny reservation, `key_update`'s "update/extend" path
/// (backend/dictionary.rs ~595-644) evicts the small-pool key and recurses
/// with the full zero-padded content at offset 0, and 4128 is not
/// `< SMALL_CAPACITY`, so it lands in the large pool -- exercising the pool
/// transition via a single write call whose payload literally contains the
/// boundary byte, distinct from growth_small_to_large_same_handle's two
/// sequential whole-buffer writes.
pub fn write_offset_spanning_pool_boundary() {
    let tmp = TmpDict::new("write_offset_spanning_pool_boundary");
    let path = tmp.path("span");
    let write_offset = SMALL_CAPACITY - 64; // 4000
    let mut rng = XorShift::new(0xA001);
    let mut payload = vec![0u8; 128]; // [4000, 4128) -- straddles the 4064 boundary
    rng.fill(&mut payload);
    {
        let mut f = check!(File::create(&path)); // empty key, small pool
        check!(f.seek(SeekFrom::Start(write_offset as u64)));
        check!(f.write_all(&payload));
    }
    let readback = read_back(&path);
    let expected_len = write_offset + payload.len();
    assert_eq!(readback.len(), expected_len, "expected a zero-filled gap plus the payload");
    assert_eq!(
        &readback[..write_offset],
        &vec![0u8; write_offset][..],
        "the seeked-past gap must read back as zeros"
    );
    assert_eq!(
        &readback[write_offset..],
        &payload[..],
        "payload spanning the pool boundary must read back intact"
    );
    check!(fs::remove_file(&path));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("sizes::boundary_below_threshold", boundary_below_threshold as fn()),
    ("sizes::boundary_at_threshold", boundary_at_threshold as fn()),
    ("sizes::boundary_above_threshold", boundary_above_threshold as fn()),
    ("sizes::zero_byte_file", zero_byte_file as fn()),
    ("sizes::one_byte_file", one_byte_file as fn()),
    ("sizes::growth_small_to_large_same_handle", growth_small_to_large_same_handle as fn()),
    ("sizes::growth_across_reopen_append", growth_across_reopen_append as fn()),
    ("sizes::shrink_safe_pattern", shrink_safe_pattern as fn()),
    ("sizes::large_file_64kib", large_file_64kib as fn()),
    ("sizes::large_file_200kib", large_file_200kib as fn()),
    ("sizes::seek_read_page_spanning_32kib", seek_read_page_spanning_32kib as fn()),
    ("sizes::write_offset_spanning_pool_boundary", write_offset_spanning_pool_boundary as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[];
