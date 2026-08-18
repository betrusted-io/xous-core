<!--
SPDX-License-Identifier: Apache-2.0
-->

# Debugging USB CCID Enumeration Issues on BAO1x

**Status:** Community reference document — not an official support channel.

**Scope:** `services/usb-bao1x/` CCID transport, `feature/usb-bao1x-ccid-openpgp`
branch and successors.

## WARNING: AI-generated content disclaimer

This document was produced with the help of an AI assistant, working from the
project's own source code, git history, and CI configuration at the time of
writing. It is **not** written or reviewed by the BAO1x maintainers, and
debugging or fixing CCID/USB issues is **not** their responsibility to triage
on your behalf just because this document exists.

Treat every code citation, line number, and claim below as a starting point to
verify yourself against the current source, not as ground truth. Line numbers
drift as the code changes; behavior described here reflects one point-in-time
investigation, not a guarantee about any future commit.

If you find something wrong or outdated, prefer fixing it locally over trusting
it blindly — and don't file it as an official bug report without confirming it
against current HEAD first.

This guide exists so that curious contributors have a map of how to investigate
this class of problem and why one specific instance of it happened — not to
offload debugging work onto the core team, and not as a substitute for reading
the actual driver code.

Related maps (also verify against HEAD):

- [`docs/CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md) — protocol, images, HIL
- [`docs/code_map.md`](code_map.md) — symptom-to-source navigation
- Local EP arithmetic: `tools/check_ep_budget.py`, `tools/test_ep_budget_cumulative.py`
- Cumulative guard: `services/usb-bao1x/src/ep_budget.rs`

---

## 1. Why this guide exists

USB composite device bring-up on constrained microcontrollers is a class of
bug that's easy to introduce and hard to diagnose from symptoms alone,
because the failure mode is usually silent from the host's point of
view: the device just never enumerates, and there is no host-side error
message that says why. The actual cause is almost always on the device side,
often inside a `panic!`, `expect()`, or silent early-return in embedded code
that a host-side USB stack never gets to see.

This guide walks through:

- The general failure signature of USB enumeration problems on BAO1x
- A worked example: a real endpoint-budget overflow found in the CCID/OpenPGP
  transport branch, from symptom to root cause to fix to regression test
- A reusable diagnostic method for the next enumeration bug, whatever
  specifically causes it
- What tooling exists locally vs. what requires hardware-in-the-loop (HIL)

It assumes you can read Rust and are comfortable with cargo, but does not
assume prior familiarity with the Corigine USB controller, Xous, or this
codebase's internal structure.

---

## 2. What "enumeration failure" looks like on this platform

When a USB device fails to enumerate, the host side symptoms are
frustratingly generic:

- Device shows up briefly then disappears (`dmesg` / Device Manager churn)
- Device never appears at all
- `lsusb` / `system_profiler` shows nothing, or shows the device stuck at
  "unknown" with no interface descriptors
- Host-side tooling (`tools/ccid_hil/*.py`, pyusb) times out waiting for
  the device, or raises a generic "device not found" error

None of these tell you why. On BAO1x, the actual cause is almost always
one of:

| Symptom class | Likely cause | Where to look first |
|---------------|--------------|---------------------|
| Device never appears at all | Panic during gadget construction, before the controller is even initialized | Boot log / UART output during device init |
| Device appears then vanishes | Panic or reset after partial enumeration (e.g. mid-SET_CONFIGURATION) | `composite_handler`, endpoint I/O paths |
| Device enumerates but a specific interface misbehaves | Class-level bug (framing, buffer overflow) rather than a transport-level one | The specific class's `UsbClass` impl (e.g. `ccid_transport.rs`) |
| Enumerates fine on one image, not another | Feature-flag-dependent difference in which classes/endpoints are built | `xtask/src/main.rs` target definitions, `hw.rs` construction order |

The single most useful first step is always: **get the device-side log.**
Everything else in this guide is secondary to that, because on embedded
targets the panic message usually says exactly what went wrong — the
difficulty is almost never finding the message once you have console
access, it's getting console access in the first place.

On Persona A CCID images there is **no** USB CDC debug serial; use UART /
`xous-log` (DUART).

---

## 3. Worked example: the CCID endpoint-budget overflow

This section documents one real bug, found and fixed on the
`feature/usb-bao1x-ccid-openpgp` branch, as a concrete illustration of the
method in section 4. Treat the line numbers as illustrative, not current.

### 3.1 The symptom

A hardware tester reported that a CCID-enabled build (`baosec-ccid`) simply
did not enumerate on a host machine — no interface, no error, nothing
actionable from the host side. Standard USB CCID + OpenPGP smartcard tooling
would never even see a device.

### 3.2 Why static analysis, not live debugging, came first

No hardware was available at investigation time. Rather than wait, the
question "why doesn't this enumerate?" was answered entirely by reading
code — specifically, by treating USB endpoint allocation as arithmetic that
can be checked without a device:

1. Every USB class that gets built into a composite device claims some
   number of endpoints, each with a direction (IN/OUT) and a type (bulk,
   interrupt, control).

2. The Corigine controller used on BAO1x has a fixed, compile-time
   endpoint budget — not a runtime-configurable one:

   ```rust
   // libs/bao1x-hal/src/usb/driver.rs
   pub const CRG_EP_NUM: usize = 8;
   ```

   This tracks unidirectional non-EP0 direction slots (8 total). EP0
   (control) is handled separately and does not consume this budget.

3. Every class that's compiled into a given build claims some of those 8
   slots at construction time, before the device ever tries to talk to a
   host. If the sum of all claims exceeds 8, the allocator (`alloc_ep`)
   returns an error — and the calling code upgrades that error into a hard
   failure, because endpoint allocation failure is considered unrecoverable
   at that layer.

So the question "does this enumerate?" reduces to: add up every
endpoint claim for this specific build, and check whether it's ≤ 8.

### 3.3 Doing the arithmetic

For the stock (non-CCID) `baosec` build:

| Class | Endpoints claimed | Direction |
|-------|-------------------|-------------|
| FIDO (`RawFidoConfig`) | 2 | Interrupt IN, Interrupt OUT |
| NKRO keyboard | 2 | Interrupt IN, Interrupt OUT |
| Debug CDC serial | 3 | Interrupt IN (comm), Bulk OUT, Bulk IN |
| **Total** | **7** | fits in 8, 1 slot spare |

For the CCID-enabled `baosec-ccid` build, as originally written:

| Class | Endpoints claimed |
|-------|-------------------|
| CCID transport (bulk OUT, bulk IN, interrupt IN) | 3 |
| FIDO | 2 |
| NKRO | 2 |
| Debug CDC serial | 3 |
| **Total** | **10 — over budget by 2** |

If the optional provisioning CDC serial port was also enabled (a second
serial device used to seed PIN data over USB), the total rose to **13**.

Both exceed the 8-slot ceiling. That's not a "might fail under load" bug —
it's a deterministic, every-single-boot failure, entirely independent of the
host, the cable, or the OS. The specific allocation call that fails first
depends on construction order (see the class registration order in
`Bao1xUsb::new`), but something is guaranteed to panic before the device
reaches enumeration.

### 3.4 The fix that was chosen (Persona A)

Several strategies were considered (see section 5 for the general decision
framework). The one implemented:

1. On CCID-enabled builds, drop debug CDC serial and provisioning CDC
   serial entirely. A later follow-up also **omitted the CCID interrupt IN**
   endpoint (Corigine `alloc_ep` pairing collided with NKRO and caused host
   `EPROTO` / `-71`). The shipping composite is CCID bulk IN/OUT + FIDO + NKRO
   — **6 of 8** slots (2 spare). Stock `baosec` remains FIDO+NKRO+debug CDC
   at **7 of 8**.

2. Debug output on CCID images moved to UART instead of USB CDC (reusing the
   project's existing UART / `xous-log` path — no new driver was written for
   this).

3. PIN provisioning over USB was removed from CCID images. Provisioning must
   now happen either (a) offline, before flashing, (b) via a separate
   non-CCID build if such a path is built later, or (c) is a no-op if the
   device's PDDB is already `OKV1`-provisioned. Unprovisioned CCID images
   log a warning on UART and continue without USB provisioning.

4. A **cumulative** endpoint-budget guard was added so this exact bug class
   can't silently reappear (see section 3.5).

### 3.5 Why per-class checks weren't enough, and what replaced them

An early version of the safety net checked each class's endpoint claim
independently — e.g. "CCID claims 3, is 3 ≤ 8? yes" and separately "HID
claims 4, is 4 ≤ 8? yes" — and considered the build safe if every individual
subtotal passed. This is a trap: 3 ≤ 8 and 4 ≤ 8 and 2 ≤ 8 can all be true
while 3 + 4 + 2 = 9 still overflows. Independent per-class checks cannot
catch a combined overflow, by construction.

The fix was a **cumulative ledger** (`services/usb-bao1x/src/ep_budget.rs`):

- Classes call `EpBudgetLedger::reserve_before_alloc` **before** their
  `alloc.*` constructors, with a running total checked against `CRG_EP_NUM`.
- Per-class `assert_class_ep_budget` is **kept** as a sanity check.
- A shared `CorigineWrapper::allocated_non_ep0` counter is incremented
  inside the real `alloc_ep` path; after construction the ledger
  `assert_matches_live` against that count so accounting cannot silently
  drift from the allocator.

A regression test proves the old logic's blind spot: adding one fake
endpoint-consuming class to a 6/8 CCID build — independent subtotals still
"pass", cumulative reserve must panic. See
`cargo test -p usb-bao1x --lib ep_budget` and
`python3 tools/test_ep_budget_cumulative.py`.

---

## 4. General diagnostic method for CCID/USB enumeration bugs

Use this order. Each step is cheap relative to the next one — don't skip to
hardware debugging if a static check would have answered the question.

### Step 1 — Read the actual diff/branch scope first

Before touching anything, know exactly what changed.
`git diff <base>...HEAD --stat`, grouped by directory, tells you whether
you're looking at a transport-layer change, a class-level change, or
unrelated noise merged in alongside it. Don't assume the branch name
accurately describes its contents — verify what's actually different from a
working baseline.

### Step 2 — Do the endpoint-budget arithmetic statically

For any change that adds, removes, or reconfigures a USB class:

1. Find the hardware's actual endpoint budget (`CRG_EP_NUM` in
   `libs/bao1x-hal/src/usb/driver.rs` — don't assume it matches another chip).
2. Enumerate every class in the composite for the specific build target
   (`xtask/src/main.rs`, `hw.rs` `cfg` gates).
3. For each class, find `alloc.bulk` / `alloc.interrupt` / `alloc.control`
   call sites and count exactly what it claims.
4. Sum them. Compare to the hardware limit.

Or run: `python3 tools/check_ep_budget.py`

This alone catches the entire class of bug described in section 3, with zero
hardware required.

### Step 3 — Get device-side console output

If the arithmetic in Step 2 doesn't explain the symptom, you need to see
what the device is actually doing. On this platform that generally means:

- UART/DUART output during boot and gadget construction (most reliable —
  works even if USB is completely broken)
- Any existing `log::*` calls already in the construction path
- Confirm whether panic messages are routed to the same console

### Step 4 — Check construction order, not just totals

If the total is near the budget (e.g. exactly at it, or one over), the
order in which classes are constructed determines which specific
allocation call fails first. Read the class registration order in
`Bao1xUsb::new` / `make_ccid_transport`, not just the class list.

### Step 5 — Check whether CI would have caught this

If this bug reached a branch/PR at all, ask: which CI job, if any, was
supposed to catch it?

- `cargo build` / `cargo check` will **not** catch a runtime panic like an
  endpoint overflow — compilation succeeds; the panic only happens when the
  code runs on the device (or under a real initiator that constructs the
  gadget).
- A job that boots real (or emulated) hardware and checks for enumeration
  would catch it — but only if that job actually executes (trigger +
  registered runner). A HIL workflow with no runner attached will never run.
- Watch for swallowed failures in host-side scripts (e.g. "device not found"
  treated as skip/pass).

### Step 6 — Fix, then add a guard that fires on the cumulative condition

Prefer a guard that:

- Checks the actual combined state (live counter tied to the real allocator),
  not only a hand-maintained per-class estimate
- Fires with a message specific enough to be useful (which classes, what
  total, what limit)
- Fires **before** the opaque low-level `alloc_ep` panic when planning the
  reservation
- Has a regression test that fails against the old (insufficient) logic and
  passes against the new one

---

## 5. Choosing a fix strategy: a framework, not a recipe

When a composite device is over its endpoint budget, there is rarely one
"correct" fix — it's a product/UX trade-off. Ask explicitly before picking:

1. **What is this image actually for?** A CCID/smartcard image and a
   general-purpose dev/debug image may drop different interfaces.
2. **Is dropped functionality gone, or relocated?** (e.g. debug CDC → UART)
3. **Does the fix cover every build target and optional/runtime branch?**
4. **Is there a smaller structural change?** (Only if the USB stack truly
   supports it — don't invent unsupported topology.)

Don't guess product requirements from code alone. If it's not resolvable
from source, docs, or comments, ask whoever owns the product decision.

---

## 6. Local tooling reference

| Tool | What it checks | Hardware? |
|------|----------------|-----------|
| `python3 tools/check_ep_budget.py` | Known targets vs `CRG_EP_NUM=8` | No |
| `python3 tools/test_ep_budget_cumulative.py` | Cumulative vs independent-subtotal gap | No |
| `cargo test -p usb-bao1x --lib ep_budget` | Ledger unit + fake-class regression | No |
| `python3 tools/sim_persona_a_composite.py` | Host-side Persona A layout asserts (mock) | No |
| `tools/ccid_hil/*.py` | Real enumeration / echo / Persona A CDC absence | Yes |

Board compile gate (needs `cargo xtask install-toolkit`):

```bash
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
```

If a local no-hardware check and a HIL test disagree, trust HIL for real
device behavior. Static checks prove what the code *says*; they do not
replace confirming a fix on silicon.

---

## 7. Open questions worth tracking (as of this writing)

Honestly unresolved or constrained items — so a future contributor doesn't
have to rediscover them:

1. **UART capture is not wired into the HIL harness.** Without it, no
   host-side automated test can confirm device-side log behavior (e.g.
   "warned and continued" for unprovisioned PDDB) when CDC was intentionally
   removed from USB.
2. **Shipping composites.** Stock `baosec` sits at **7/8 (FRAGILE)**. CCID
   images (`dabao-ccid` / `baosec-ccid` / `ccid-hil`) sit at **6/8** after
   the interrupt IN was omitted. Document any future USB class as needing an
   explicit exclusion or budget change.
3. **Board-target compile of the cumulative guard:** verified with
   `cargo check … --target riscv32imac-unknown-xous-elf` after
   `install-toolkit` (matched rustc / `betrusted-io/rust` toolkit). That is
   still **not** a substitute for HIL enumeration on real hardware.

---

## 8. Summary

Enumeration bugs on constrained USB controllers are, more often than not,
arithmetic bugs wearing a hardware disguise. The controller's endpoint
budget is fixed and checkable statically; a device that requests more than
the hardware has will fail identically every boot — which makes this one of
the more satisfying classes of embedded bug to diagnose when you actually
add up the numbers.

When arithmetic alone doesn't explain a symptom, the next cheapest tool is
device-side console (UART). Get that working before assuming you need a
full HIL setup.

And when you fix a bug like this, the guard you add afterward is only as
good as its regression test: if you can't demonstrate that the old logic
would have missed the bug and the new logic catches it, you don't actually
know whether you've closed the gap or just moved it.
