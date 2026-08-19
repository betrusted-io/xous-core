//! Theme `concur`: concurrency (std::thread over std::fs on the PDDB).
//!
//! std::thread works on xous (128 KiB default stacks; none of these tests need
//! more). See the `tests` module header
//! (services/pddb-fs-tests/src/tests/mod.rs) for authoring rules and
//! services/pddb-fs-tests/README.md for the PFC registry.
//!
//! KNOWN LANDMINE, source-confirmed (services/pddb/src/main.rs:601, 1467):
//! `fd_mapping` is keyed by `msg.sender.pid()` ALONE, not by (pid, tid). Every
//! thread of this process shares one PID, so PFC-4's "any successful close
//! drops the WHOLE per-process fd table" (`fd_mapping.remove(&pid)`) is not
//! just a same-thread, sequential-handles hazard (as smoke::two_files_close_one
//! and concur::four_handles_close_one pin it) -- it generalizes structurally
//! to CONCURRENT threads: if thread A closes any handle while thread B has a
//! different handle open (even on a totally unrelated dict), B's handle can go
//! dead. Tests below that run genuinely concurrent open/close cycles
//! (two_threads_separate_dicts_cycles, three_threads_same_dict_distinct_keys)
//! are therefore written to TOLERATE isolated I/O errors as already-known
//! PFC-4 noise (logged, counted, not asserted to be zero) while still
//! strictly asserting the property that actually matters and that PFC-4 does
//! NOT license: no thread may ever observe wrong/cross-contaminated DATA, and
//! at least some concurrent work must get through cleanly (a total wedge would
//! be a new, more severe bug). This is a deliberate design choice to keep the
//! suite deterministic (registering a genuinely racy test as a hard XFAIL
//! risks intermittent XPASS, which the driver treats as a failure).

#![allow(unused_imports)]
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::harness::{TmpDict, XorShift, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(f.read_to_end(&mut buf));
    buf
}

/// One thread's repeated create/write/read/delete cycle against its OWN key
/// in its OWN dict. Returns (successful_cycles, tolerated_errors). A cycle
/// error (e.g. a stale fd from the OTHER thread's concurrent close, PFC-4) is
/// tolerated and counted; a WRONG value read back never is -- that would be
/// actual cross-thread data corruption, not a clean I/O error, and is asserted
/// against unconditionally.
fn cycle_worker(thread_id: &'static str, cycles: usize) -> (usize, usize) {
    let tmp = TmpDict::new(&format!("two_threads_{}", thread_id));
    let path = tmp.path("cycle");
    let mut ok = 0usize;
    let mut errs = 0usize;
    for i in 0..cycles {
        let content = format!("{}-{}", thread_id, i);
        match fs::write(&path, content.as_bytes()) {
            Ok(()) => match File::open(&path).and_then(|mut f| {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map(|_| buf)
            }) {
                Ok(buf) => {
                    assert_eq!(
                        &buf[..],
                        content.as_bytes(),
                        "cross-talk or corruption: thread {} cycle {} read back {:?}, expected {:?}",
                        thread_id,
                        i,
                        buf,
                        content.as_bytes()
                    );
                    ok += 1;
                    // best-effort: a missing key here is itself PFC-4 noise
                    // (a concurrent close from the other thread can also
                    // invalidate the delete path's own fd-table lookups),
                    // not asserted.
                    let _ = fs::remove_file(&path);
                }
                Err(_) => errs += 1, // tolerated PFC-4 cross-thread noise (see module doc)
            },
            Err(_) => errs += 1, // tolerated PFC-4 cross-thread noise (see module doc)
        }
        if (i + 1) % 5 == 0 {
            log::info!(
                "two_threads_separate_dicts_cycles: {} completed {}/{} cycles ({} ok, {} tolerated errs)",
                thread_id,
                i + 1,
                cycles,
                ok,
                errs
            );
        }
    }
    (ok, errs)
}

/// (1) Two threads, each with its own TmpDict, each running 15 independent
/// create/write/read/delete cycles concurrently. Asserts no cross-talk: every
/// successful read-back must equal exactly what THAT thread itself wrote
/// (thread-tagged content makes any cross-contamination immediately visible),
/// and both threads must get at least some cycles through cleanly (a total
/// wedge would be a new, more severe bug than PFC-4's already-documented
/// noise). See the module doc for why isolated I/O errors are tolerated
/// rather than hard-asserted to never happen.
pub fn two_threads_separate_dicts_cycles() {
    let cycles = 15;
    let t1 = std::thread::spawn(move || cycle_worker("alpha", cycles));
    let t2 = std::thread::spawn(move || cycle_worker("beta", cycles));
    let (a_ok, a_err) = t1.join().expect("thread alpha panicked unexpectedly");
    let (b_ok, b_err) = t2.join().expect("thread beta panicked unexpectedly");
    log::info!(
        "two_threads_separate_dicts_cycles: done -- alpha ok={} err={}, beta ok={} err={}",
        a_ok,
        a_err,
        b_ok,
        b_err
    );
    assert!(a_ok > 0, "alpha thread never completed a single cycle successfully ({} errors)", a_err);
    assert!(b_ok > 0, "beta thread never completed a single cycle successfully ({} errors)", b_err);
}

/// (2) Three threads write three DISTINCT keys into the SAME dict
/// concurrently (single open/write/close per thread -- the smallest possible
/// exposure to the PFC-4 cross-thread landmine described in the module doc).
/// The main thread then read_dir's the dict and reads back every key whose
/// writer thread reported success. A reported success that turns out
/// unreadable or wrong is real corruption and is never tolerated; an
/// individual writer erroring out cleanly is tolerated PFC-4 noise, but not
/// all three failing (that would mean the dict itself is wedged).
pub fn three_threads_same_dict_distinct_keys() {
    let tmp = TmpDict::new("three_threads_same_dict_distinct_keys");
    let dict = tmp.dict().to_string();

    let mut handles = Vec::new();
    for i in 0..3 {
        let path = format!("{}/key{}", dict, i);
        handles.push(std::thread::spawn(move || {
            let content = format!("value-{}", i);
            fs::write(&path, content.as_bytes()).map(|()| (path, content))
        }));
    }

    let mut written = Vec::new();
    for h in handles {
        match h.join().expect("writer thread panicked unexpectedly") {
            Ok(pair) => written.push(pair),
            Err(e) => log::info!(
                "three_threads_same_dict_distinct_keys: a concurrent write errored ({}) -- \
                 tolerated as PFC-4 cross-thread fd-table-wipe noise, see module doc",
                e
            ),
        }
    }
    assert!(
        !written.is_empty(),
        "every concurrent writer failed -- the dict looks wedged, not merely PFC-4-noisy"
    );

    let entries = check!(fs::read_dir(&dict));
    let found: BTreeSet<String> =
        entries.map(|e| check!(e).file_name().into_string().expect("non-UTF8 name")).collect();

    for (path, content) in &written {
        let name = path.rsplit('/').next().unwrap().to_string();
        assert!(
            found.contains(&name),
            "read_dir did not list {} even though its writer thread reported success",
            name
        );
        let actual = check!(fs::read(path));
        assert_eq!(&actual[..], content.as_bytes(), "content mismatch for {} after concurrent write", name);
    }
}

/// (3) `read_dir` racing a writer thread that is still adding keys to the
/// same dict. Must never panic or hang. Tolerant snapshot semantics: the
/// writer here only ADDS keys (never removes/renames), so a `read_dir` that
/// lands mid-write can only ever observe a PREFIX of the eventual key set --
/// it is asserted to be a subset-or-equal of the final listing, but NOT
/// required to equal it (the whole point is that the race is real and there
/// is no product-side locking to prevent it; requiring equality would just be
/// asserting away real, expected timing nondeterminism).
pub fn read_dir_races_writer() {
    let tmp = TmpDict::new("read_dir_races_writer");
    let dict = tmp.dict().to_string();
    let num_keys = 12;

    let dict_for_writer = dict.clone();
    let writer = std::thread::spawn(move || {
        let mut names = Vec::new();
        for i in 0..num_keys {
            let name = format!("k{:02}", i);
            let path = format!("{}/{}", dict_for_writer, name);
            check!(fs::write(&path, format!("v{}", i).as_bytes()));
            names.push(name);
            if (i + 1) % 5 == 0 {
                log::info!("read_dir_races_writer: writer added {}/{} keys", i + 1, num_keys);
            }
        }
        names
    });

    // No synchronization on purpose: this IS the race under test.
    let racy = check!(fs::read_dir(&dict));
    let racy_names: BTreeSet<String> =
        racy.map(|e| check!(e).file_name().into_string().expect("non-UTF8 name")).collect();

    let writer_names: Vec<String> = writer.join().expect("writer thread panicked unexpectedly");
    let writer_record: BTreeSet<String> = writer_names.iter().cloned().collect();

    let final_listing = check!(fs::read_dir(&dict));
    let final_set: BTreeSet<String> =
        final_listing.map(|e| check!(e).file_name().into_string().expect("non-UTF8 name")).collect();
    assert_eq!(
        final_set, writer_record,
        "post-join read_dir does not match the writer thread's own record of what it wrote"
    );

    assert!(
        racy_names.is_subset(&final_set),
        "racy read_dir saw names absent from the final listing (not a valid prefix snapshot): {:?}",
        racy_names.difference(&final_set).collect::<Vec<_>>()
    );

    // Every write path must be verified by read-back:
    // the race itself is only about read_dir's ENUMERATION, but each key the
    // writer thread reported as written must still hold its exact content.
    for (i, name) in writer_names.iter().enumerate() {
        let path = format!("{}/{}", dict, name);
        let content = check!(fs::read(&path));
        assert_eq!(content, format!("v{}", i).as_bytes(), "key {} did not read back intact", name);
        if (i + 1) % 5 == 0 {
            log::info!("read_dir_races_writer: verified {}/{} keys by read-back", i + 1, num_keys);
        }
    }
}

/// (4) Sequential many-handles characterization, N=4 generalization of
/// smoke::two_files_close_one: open 4 handles on 4 distinct keys, close the
/// FIRST one, then probe the other three. Correct POSIX behavior: closing one
/// fd never affects any other fd, so all three keep working exactly as
/// before. XFAIL PFC-4: `CloseKeyStd`'s `fd_mapping.remove(&pid)` drops the
/// WHOLE per-process fd table on that one successful close, so all three
/// probes are expected to fail.
///
/// Ordering follows smoke::two_files_close_one's PFC-7 hazard rule: Results
/// are collected first and ALL still-live handles are dropped in a normal
/// (non-panicking) context before anything here is allowed to panic --
/// panicking while a dead-fd `File` is still in scope would drop it during
/// unwind, and a second panic there would abort the whole runner.
pub fn four_handles_close_one() {
    let tmp = TmpDict::new("four_handles_close_one");
    let paths: Vec<String> = (0..4).map(|i| tmp.path(&format!("h{}", i))).collect();
    for (i, p) in paths.iter().enumerate() {
        check!(fs::write(p, format!("init-{}", i).as_bytes()));
        // Every write path must be verified by read-back
        // -- including handle 0's, even though it is about to be closed
        // deliberately below and never probed again afterward.
        assert_eq!(
            &read_back(p)[..],
            format!("init-{}", i).as_bytes(),
            "initial write {} did not read back intact",
            i
        );
    }

    let mut handles: Vec<File> =
        paths.iter().map(|p| check!(OpenOptions::new().read(true).write(true).open(p))).collect();

    // Close the FIRST handle. POSIX: this must only affect that one fd.
    let doomed = handles.remove(0);
    drop(doomed);

    // Probe the remaining three WITHOUT panicking while any of them is still
    // open (see the PFC-7 hazard note above): collect every Result first.
    let mut seek_results = Vec::new();
    let mut read_results = Vec::new();
    for h in handles.iter_mut() {
        seek_results.push(h.seek(SeekFrom::Start(0)));
        let mut buf = Vec::new();
        read_results.push(h.read_to_end(&mut buf).map(|_| buf));
    }
    drop(handles); // all closes happen here, in a normal (non-panicking) context

    for (i, (seek_r, read_r)) in seek_results.into_iter().zip(read_results.into_iter()).enumerate() {
        let idx = i + 1; // handle 0 was removed above; these are handles 1..3
        check!(seek_r);
        let buf = check!(read_r);
        assert_eq!(
            &buf[..],
            format!("init-{}", idx).as_bytes(),
            "handle {} did not read back intact after handle 0 closed",
            idx
        );
    }

    for p in &paths {
        let _ = fs::remove_file(p);
    }
}

/// (5) Two independent handles opened on the SAME key, from two GENUINELY
/// concurrent std::thread workers. (The brief scopes item (4)'s many-handles
/// test explicitly to "sequential" characterization but says no such thing
/// here, and a same-key multi-handle test that never actually runs on two
/// threads would under-deliver the "concur" theme.) Each thread owns a
/// DISJOINT byte range of the shared 10-byte key (A: 0-1 then 2-3; B: 4-5
/// then 8-9), so the final layout is deterministic regardless of scheduling
/// order -- the point under test is whether the server's shared-buffer
/// writes stay non-overlapping-safe under real concurrent access, not
/// scheduler order itself (no seeks past the seeded length, so this also
/// stays outside PFC-5's stale-length territory).
///
/// Neither handle is closed until BOTH threads have finished all of their
/// writes and been joined back into the main thread: PFC-4 (`CloseKeyStd`'s
/// whole-process fd-table wipe, see the module doc) fires only on a
/// successful CLOSE, so deferring both closes this way keeps the test about
/// same-key write semantics rather than accidentally becoming another PFC-4
/// reproducer.
///
/// Documents xous's actual semantics: the server holds one shared in-memory/
/// on-disk copy of the key's bytes and each write opcode splices directly
/// into it at the given offset (backend/dictionary.rs key_update) -- there is
/// no per-handle private buffering, so two handles on the same key behave
/// like two independent POSIX opens of the same inode with disjoint-range
/// pwrite()s: whichever thread's writes the scheduler lands first, the final
/// byte layout is the same. This matches POSIX and is asserted as-is (no
/// XFAIL).
pub fn same_file_two_handles_interleaved_writes() {
    let tmp = TmpDict::new("same_file_two_handles_interleaved_writes");
    let path = tmp.path("shared");
    check!(fs::write(&path, &[0u8; 10])); // seed: 10 zero bytes, well under 4 KiB

    let path_a = path.clone();
    let ta = std::thread::spawn(move || -> File {
        let mut ha = check!(OpenOptions::new().read(true).write(true).open(&path_a));
        check!(ha.seek(SeekFrom::Start(0)));
        check!(ha.write_all(b"AA"));
        check!(ha.seek(SeekFrom::Start(2)));
        check!(ha.write_all(b"CC"));
        ha
    });
    let path_b = path.clone();
    let tb = std::thread::spawn(move || -> File {
        let mut hb = check!(OpenOptions::new().read(true).write(true).open(&path_b));
        check!(hb.seek(SeekFrom::Start(4)));
        check!(hb.write_all(b"BB"));
        check!(hb.seek(SeekFrom::Start(8)));
        check!(hb.write_all(b"DD"));
        hb
    });
    let ha = ta.join().expect("thread A panicked unexpectedly");
    let hb = tb.join().expect("thread B panicked unexpectedly");
    // Both threads finished ALL their writes before either handle is closed
    // here, in the main thread, in a normal (non-panicking) context.
    drop(ha);
    drop(hb);

    let content = read_back(&path);
    // A owns bytes 0-3 (AACC), B owns bytes 4-5 and 8-9 (BB..DD); bytes 6-7
    // stay the untouched 0x00 NUL bytes from the seed (NOT ASCII '0' -- the
    // suite cold run caught exactly that authoring slip) -- disjoint ranges
    // make this deterministic.
    assert_eq!(
        &content[..],
        b"AACCBB\x00\x00DD",
        "disjoint-range concurrent same-key writes through two threads' handles did not land byte-exact"
    );
    check!(fs::remove_file(&path));
}

/// (6) A thread panics mid-I/O via a deliberate wrong-value assert while the
/// main thread keeps doing normal fs ops concurrently. This validates HARNESS
/// isolation (a spawned thread's panic must not take down the runner process
/// or wedge the PDDB connection for other threads), not any PDDB behavior --
/// std::thread panics unwind normally on xous (panics unwind; catch_unwind
/// works), so the worker thread's death must be fully contained to that
/// thread.
///
/// PFC-4 EXPOSURE (empirically hit on the suite warm run): the worker's
/// `File` close -- explicit or via drop-during-unwind -- wipes this whole
/// process's fd table, so a main-thread op caught mid-cycle (fs::write holds
/// an fd between its open and close) can fail with a clean I/O error while
/// the worker unwinds. Per this theme's design (module doc), the CONCURRENT
/// phase therefore tolerates clean I/O errors as known PFC-4 noise (logged,
/// counted, never wrong-data), and the STRICT health proof runs after join,
/// when no concurrent closer can exist any more. The worker also drops its
/// handle in normal (non-unwind) context before panicking, per the PFC-7
/// drop-handles-before-panicking rule (so a close during unwind cannot
/// double-panic and abort the runner).
pub fn thread_panic_mid_io_isolation() {
    let tmp = TmpDict::new("thread_panic_mid_io_isolation");
    let victim_path = tmp.path("victim");
    check!(fs::write(&victim_path, b"expected"));
    assert_eq!(&read_back(&victim_path)[..], b"expected", "victim seed did not read back intact");

    let worker_path = victim_path.clone();
    let handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let read_res = File::open(&worker_path).and_then(|mut f| f.read_to_end(&mut buf).map(|_| ()));
        // The handle is dropped inside and_then, in normal (non-unwind)
        // context. Whichever arm runs, this thread panics ON PURPOSE: that
        // containment is the property under test. (The Err arm exists because
        // the main thread's concurrent closes can kill this thread's fd first
        // -- PFC-4 -- which must not turn into a different test outcome.)
        match read_res {
            Ok(()) => assert_eq!(
                &buf[..],
                b"WRONG-ON-PURPOSE",
                "deliberate panic to exercise harness thread isolation"
            ),
            Err(e) => {
                panic!("deliberate panic (read errored first: {} -- tolerated PFC-4 noise)", e)
            }
        }
    });

    // Main thread keeps doing normal fs ops WHILE the worker thread panics.
    // Clean I/O errors here are tolerated PFC-4 cross-thread noise (see the
    // doc comment above); wrong DATA on a successful read never is.
    let main_path = tmp.path("main");
    let mut ok = 0usize;
    let mut errs = 0usize;
    for i in 0..5 {
        let content = format!("main-{}", i);
        match fs::write(&main_path, content.as_bytes()) {
            Ok(()) => match fs::read(&main_path) {
                Ok(buf) => {
                    assert_eq!(
                        &buf[..],
                        content.as_bytes(),
                        "corruption: concurrent-phase read-back mismatch on cycle {}",
                        i
                    );
                    ok += 1;
                }
                Err(_) => errs += 1, // tolerated PFC-4 noise
            },
            Err(_) => errs += 1, // tolerated PFC-4 noise
        }
    }
    log::info!("thread_panic_mid_io_isolation: concurrent phase done ({} ok, {} tolerated errs)", ok, errs);

    let join_result = handle.join();
    assert!(
        join_result.is_err(),
        "worker thread was expected to panic on its deliberate wrong-value assert, but it did not"
    );

    // STRICT health proof, after join: the worker is gone, so no concurrent
    // fd-table wipe can occur -- the runner (and its PDDB session) must now
    // work cleanly, proving the child thread's panic was fully contained.
    check!(fs::write(&main_path, b"post-panic-health"));
    assert_eq!(
        &read_back(&main_path)[..],
        b"post-panic-health",
        "post-join write did not read back intact -- the panic was not contained"
    );
    check!(fs::remove_file(&main_path));
    assert!(File::open(&main_path).is_err(), "main_path should be gone after remove_file");
    check!(fs::remove_file(&victim_path));
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    ("concur::two_threads_separate_dicts_cycles", two_threads_separate_dicts_cycles as fn()),
    ("concur::three_threads_same_dict_distinct_keys", three_threads_same_dict_distinct_keys as fn()),
    ("concur::read_dir_races_writer", read_dir_races_writer as fn()),
    ("concur::four_handles_close_one", four_handles_close_one as fn()),
    ("concur::same_file_two_handles_interleaved_writes", same_file_two_handles_interleaved_writes as fn()),
    ("concur::thread_panic_mid_io_isolation", thread_panic_mid_io_isolation as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[("concur::four_handles_close_one", "PFC-4")];
