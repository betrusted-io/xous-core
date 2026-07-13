# pddb-fs-tests

An on-target test suite for the `std::fs` implementation backed by the PDDB.

Unlike `cargo xtask pddb-ci` (which runs on the host and therefore links the
*host's* std), this service is baked into a Renode image and runs on the real
riscv32 xous libstd, so it exercises the whole chain: the `std::fs` client shim
→ senres IPC → the PDDB `libstd` glue (`services/pddb/src/libstd/`) → backend →
emulated SPI NOR.

`main()` waits for the PDDB to mount, runs every registered test under
`catch_unwind`, and prints one machine-parsable line per test on the log console
(`TEST <name> PASS|FAIL|XFAIL|XPASS`), ending with a `FS-TESTS DONE: ...` summary
and `CI done`. A Renode-side driver watches those lines. About half the tests are
ported/adapted from rust's own `library/std/src/fs/tests.rs`; the rest cover
PDDB-specific ground (path grammar, the ~4 KiB small/large key-pool boundary,
name-length limits, multi-handle and concurrency semantics, and markers verified
across a full emulator restart).

## Running

```
cargo xtask pddb-fs-ci --no-verify     # build the Renode image
python3 tools/pddb-fs-ci.py            # boot, format, run, audit (see tools/README.md)
```

`tools/pddb-fs-ci.py` drives the emulator end to end; `emulation/tests/pddb-fs.robot`
is the equivalent `renode-test` suite used by `.github/workflows/pddb-renode-ci.yml`.

## Known-good, known-broken

The suite is green while every open bug stays visible: a test that reproduces a
known defect asserts the *correct* behavior and is registered as an expected
failure (`XFAIL`) rather than being weakened. If a bug is fixed, its test flips to
`XPASS` and the run goes red until the registry is updated. One reproducer ships
disabled because it panics the pddb server outright (a truncating `File::create`
over an existing key ≥ 4 KiB — the same symptom as issue #297, whose 2023 fix
touched only the shell command, not the server path std::fs uses).

Each XFAIL test's doc comment states the defect, its mechanism, and the suspected
code path; grep the theme files under `src/tests/` for `XFAIL`.

## Adding a test

Write a `pub fn` in the appropriate `src/tests/<theme>.rs` (panic to fail, return
to pass), then add it to that file's `TESTS` table; `src/tests/mod.rs` aggregates
the per-theme tables. The important xous/PDDB gotchas — the `/` (not `:`) dict/key
separator, that `File::create` does not auto-create its parent dict, that
`metadata().len()` is always 0, and the ≥ 4 KiB truncate hazard — are documented
at the top of `src/tests/mod.rs` and in the existing tests. Use `TmpDict` for
isolation, verify every write by reading it back, and keep re-created files under
4 KiB.
