# net-tests

An on-target test suite for the `std::net` implementation backed by the xous
`net` service.

Hosted-mode CI links the *host's* std, so it never touches our networking code.
This service is baked into a Renode image and runs on the real riscv32 xous
libstd, exercising the whole chain: the `std::net` client shim → the `net`
server (`services/net/`) → smoltcp → the emulated WF200 / COM link. Everything
runs against the device's own address and 127.0.0.1 (loopback through the real
stack), so no external network is involved.

`main()` joins the (emulated) wlan and waits for the `net` service to report an
IPv4 config, then runs every registered test under `catch_unwind` and prints one
machine-parsable line per test on the log console
(`TEST <name> PASS|FAIL|XFAIL|XPASS`), ending with a `NET-TESTS DONE: ...`
summary and `CI done`. A Renode-side driver watches those lines. The suite is 99
tests across eight themes (`smoke` 12, `tcp` 25, `udp` 14, `errors` 9,
`sockopts` 8, `dns` 10, `concur` 10, `timeouts` 11); many are ported/adapted
from rust's own `library/std/src/net/{tcp,udp}/tests.rs`, the rest cover
xous-specific ground: the small socket buffers and the 1530-byte MTU boundary,
the loopback path, error-kind decoding, DNS response parsing (against an
in-image fake resolver), timeout behavior under the emulator's virtual clock,
and multi-socket concurrency.

Because the image has no DHCP peer on the emulated switch, the `net` service —
only under its `renode-minimal` feature — seeds a static IPv4 config through the
same handler a real DHCP bind would use. That is the sole product-code change
this suite requires, and it is confined to that feature gate.

## Running

```
cargo xtask std-net-ci --no-verify     # build the Renode image
python3 tools/std-net-ci.py            # boot, wait for net-up, run (see tools/README.md)
```

`tools/std-net-ci.py` drives the emulator end to end; `emulation/tests/std-net.robot`
is the equivalent `renode-test` suite used by `.github/workflows/pddb-renode-ci.yml`.

## Cross-host (cross-host)

The suite above is hermetic: it loops back inside the DUT. A second suite,
built with the `cross-host` feature, exercises what loopback cannot — the DUT
exchanging real packets with a second Renode machine
(`emulation/linux-server.resc`, a busybox Linux) on the same switch as the EC's
WF200:

```
cargo xtask std-net-cross-host-ci --no-verify   # build the cross-host image
python3 tools/std-net-cross-host-ci.py          # start peer + DUT, provision peer, run
```

The two suites are mutually exclusive — the cross-host image registers only the
`cross-host` theme (`src/tests/cross-host.rs`), because its real resolver is incompatible
with loopback's self-hosted fake DNS. Cross-host covers: a **real DHCP** lease from the
peer's `udhcpd` (the image is built with `net/renode-peer`, which skips the
loopback static seed); **cross-host TCP/UDP** echo against the peer's `nc` servers,
including an 8 KiB transfer that exercises real over-the-wire segmentation; a
**real RST** on connect-to-a-dead-port; and **real DNS** resolved by the peer's
`dnsd`. `tools/std-net-cross-host-ci.py` drives the peer's serial console (on a pty)
to a passwordless root shell and brings its services up before the DUT takes its
lease; the peer contract (IP, ports, DNS records) is duplicated in the driver
and `src/tests/cross-host.rs` and must stay in lockstep. Scope limits follow the
peer: IPv4 only, `dnsd` static A records only (no CNAME chains — loopback pins
those), clock at 1970 (no TLS).

## Known-good, known-broken

The suite is green while every open bug stays visible: a test that reproduces a
known defect asserts the *correct* behavior and is registered as an expected
failure (`XFAIL`) rather than being weakened. If a bug is fixed, its test flips
to `XPASS` and the run goes red until the registry is updated. Error *kinds* and
some behaviors are a property of the `(rustc, xous-core)` pair, so the workflow
pins both; each XFAIL's doc comment records the mechanism and suspected code
path (grep the theme files under `src/tests/` for `XFAIL`).

A few reproducers ship **disabled** (`#[allow(dead_code)]`, not registered)
because they crash or wedge the `net` service, which would hang the rest of the
run: binding/connecting an IPv6 address panics smoltcp on the v4-only interface,
and a connect timeout that lingers as a session timeout aborts an established
connection whose parked writer is then never woken. Their doc comments explain
how to reproduce them by hand.

## Adding a test

Write a `pub fn` in the appropriate `src/tests/<theme>.rs` (panic to fail,
return to pass), then add it to that file's `TESTS` table; `src/tests/mod.rs`
aggregates the per-theme tables and states the authoring rules. The important
hazards are documented there and worth repeating: allocate every port through
`harness::next_port` and never reuse one; route any call that a bug can park
forever through `harness::bounded` (a wedge counts as a hard failure even for an
XFAIL) and release TCP sockets with `harness::discard` rather than dropping them
on the test thread; target the device's own IP for UDP (127.0.0.1 does not loop
back for UDP); log every few iterations in long loops so the driver's inactivity
reaper does not mistake a busy test for a dead server; and use only the
deterministic `harness::XorShift`, never the `rand` crate.
