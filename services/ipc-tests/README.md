# ipc-tests

On-target regression runner for `xous-ipc` (`Buffer`, `into_buf`, `replace`,
`as_flat`/`to_original`, checked access, and the kernel lend paths), built for
Renode CI. Sentinel grammar and runner structure mirror
`services/net-tests` / `services/pddb-fs-tests`; the machine definition is
`emulation/tests/ipc-ci.resc` (headless SoC only -- this image has no COM
service, so unlike the net suites it needs no EC peer).

Build & run locally:

    cargo xtask ipc-ci --no-verify
    python3 tools/ipc-ci.py

CI: `.github/workflows/ipc-renode-ci.yml` drives `emulation/tests/ipc-ci.robot`
under `renode-test`. Expected steady state: `pass=11 fail=0 xfail=0 xpass=0
total=11`, then `CI done`.

## What is under test here that hosted CI cannot reach

- The riscv32 instantiation of the rkyv wire format (4-byte relative
  pointers, target-dependent padding). Growth-path tests assert
  `into_buf().used()` against an in-binary `IoWriter<io::Sink>` probe rather
  than hardcoded byte counts, which pins probe==real-size ON THIS TARGET
  without encoding host layout into the suite.
- Real page-backed kernel buffers: probe-sized multi-page allocation, the
  `used`/`capacity` contract, demand-paged zeroization costs, and a full
  MutableBorrow round-trip through `services/ipc-echo` (including the
  returned-offset update when the server's response is a different length).

## Authoring rules (also in tests.rs)

- Never nest heap types inside `Vec` elements (`Vec<String>` ...): the 256-byte
  `SubAllocator` scratch caps such nests at ~64 elements. Flat `Vec<POD>`,
  `String`, POD inline fields are safe.
- Sizes are asserted against the in-binary probe, never hardcoded; the suite
  must stay layout-portable.
- A `TEST <name> ...` sentinel is emitted from the runner only (never inside a
  test); a test signals failure by panicking, which the runner converts to a
  one-line FAIL reason. Never print `PANIC in PID` (the runner's panic hook
  prevents the default hook from firing -- do not install your own hook in a
  test).
- Tests run on worker threads with a 120 s verdict deadline; the echo tests
  are the only cross-process ones (a wedged `ipc-echo` wedges only those).
- `ipc-echo` must stay registered before the runner starts: the runner's
  readiness gate polls `xous-names` and emits a failing DONE line if it times
  out.
