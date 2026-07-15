<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID protocol and Raspberry Pi HIL setup

This document describes the USB CCID transport implemented in `usb-bao1x`
(`ccid-openpgp` feature), how it relates to host-side tools such as `pcscd` /
GnuPG, and how to configure a Raspberry Pi as a hardware-in-the-loop (HIL) test
host.

**Audience:** reviewers who are not CCID experts, firmware developers wiring an
APDU handler, and anyone setting up hardware-in-the-loop (HIL) regression tests.

**Code navigation:** [`docs/code_map.md`](code_map.md) — symptom-to-source map
for debugging and fixing CCID/provisioning issues.

## Table of contents

1. [Background: smart cards, CCID, and OpenPGP](#background-smart-cards-ccid-and-openpgp)
2. [Scope: what xous-core does and does not do](#scope-what-xous-core-does-and-does-not-do)
3. [Security considerations](#security-considerations)
4. [Architecture overview](#architecture-overview)
5. [USB composite device](#usb-composite-device)
6. [Feature flags and firmware images](#feature-flags-and-firmware-images)
7. [CCID USB interface](#ccid-usb-interface)
8. [Message framing on the wire](#message-framing-on-the-wire)
   - [Example CCID hex dumps](#example-ccid-hex-dumps)
9. [APDUs, T=1, and XfrBlock](#apdus-t1-and-xfrblock)
10. [Xous IPC API and handler integration](#xous-ipc-api-and-handler-integration)
   - [Handler skeleton (Rust)](#handler-skeleton-rust)
11. [First-boot provisioning (CDC serial)](#first-boot-provisioning-cdc-serial)
12. [HIL test personality (`ccid-echo`)](#hil-test-personality-ccid-echo)
13. [Host software path (pcscd / GnuPG)](#host-software-path-pcscd--gnupg)
14. [Testing guide](#testing-guide)
15. [Raspberry Pi HIL setup](#raspberry-pi-hil-setup)
16. [CI summary](#ci-summary)

---

## Background: smart cards, CCID, and OpenPGP

### Smart cards

A **smart card** (or secure element acting as one) exposes a command/response
protocol called **APDU** (Application Protocol Data Unit). Typical exchanges
look like: host sends `SELECT`, `GET DATA`, `SIGN`, and so on; the card returns
status bytes (`SW1-SW2`, e.g. `90 00` for success) plus optional response data.

OpenPGP hardware tokens (YubiKey OpenPGP, Nitrokey, etc.) implement the
[OpenPGP card specification](https://gnupg.org/ftp/specs/OpenPGP-card-3.4.pdf)
on top of that APDU layer.

### CCID (Chip Card Interface Device)

**CCID** is a USB device class (interface class `0x0B`) defined for card
*readers*. The host does not send raw APDUs on USB directly; it wraps them in
**CCID bulk messages**:

- **`PC_to_RDR_*`** — host to reader (request)
- **`RDR_to_PC_*`** — reader to host (response)

Linux routes these through **`pcscd`** (PC/SC daemon). User tools such as
**GnuPG** (`gpg --card-status`, `gpg --sign`) talk to `pcscd`, which talks
CCID to the USB device.

Reference: [USB CCID 1.1 specification](https://www.usb.org/sites/default/files/DWG_SmartCard_CCID_V1.1.pdf).

### Where Baosec fits

From the host's point of view, a Baosec running this firmware looks like a
**USB CCID reader** with one slot. The "card" is not a physical insert; the
OpenPGP application logic runs in a **separate Xous service** on the device.
`usb-bao1x` is the USB plumbing between the Linux host and that service.

```
  gpg / OpenSC          pcscd              pyusb (HIL tests)
       |                  |                        |
       +------------------+------------------------+
                          |
                    CCID bulk USB
                          |
                    usb-bao1x  -------- IPC ------>  OpenPGP handler
                   (framing only)                  (APDU + crypto)
```

---

## Scope: what xous-core does and does not do

| Layer | Responsibility | In xous-core? |
|-------|----------------|---------------|
| USB CCID descriptors, bulk IN/OUT, frame assembly | Transport | **Yes** (`ccid_transport.rs`, `ccid_framing.rs`) |
| Deferred IPC for complete host frames | Transport API | **Yes** (`CcidRxDeferred` / `CcidTx`) |
| First-boot capture of two opaque PIN lines into PDDB | Provisioning | **Yes** (`ccid_store.rs`, provisioning CDC) |
| Parse `PC_to_RDR_*` message types | Protocol | **No** |
| T=1 block protocol, APDU parsing | Card protocol | **No** |
| OpenPGP card emulation, key storage, crypto | Application | **No** (external service, e.g. `baochip-openpgp`) |
| `pcscd` driver, GnuPG integration | Host stack | **No** |
| CCID interrupt notifications (insert/remove) | Transport | **No** (stub endpoint only) |

The Cargo feature is named `ccid-openpgp` for product alignment, but **no
OpenPGP or Galdralag crates** are linked into xous-core. All cryptography stays
out of tree behind IPC.

Everything is gated behind `ccid-openpgp` on Xous builds so default `baosec`
images are unaffected when the feature is disabled.

---

## Security considerations

This section is for merge review of [PR #890](https://github.com/betrusted-io/xous-core/pull/890).
The PR adds a **potentially security-sensitive USB surface** (CCID + first-boot
provisioning). It does **not** deliver OpenPGP security by itself; it exposes
transport and storage primitives that a handler and factory process must use
correctly.

### Threat model and non-goals

**In scope for this PR (xous-core):**

- Present a USB CCID bulk interface to a connected host.
- Reassemble and forward complete `PC_to_RDR` frames to one deferred IPC listener.
- Accept complete `RDR_to_PC` reply blobs from that listener and stream them on bulk IN.
- Optionally expose a one-time provisioning CDC port until PDDB marks provisioning complete.
- Persist two opaque byte lines and a completion marker in PDDB.

**Explicit non-goals (must be provided elsewhere):**

- OpenPGP card security, key generation, PIN verification, or cryptographic operations.
- Authentication of the USB host or provisioning tool.
- Rate limiting, intrusion detection, or audit logging beyond basic `log` lines.
- Validation of provisioning line format or semantic meaning.
- Protection against a compromised or malicious Xous process that already holds PDDB access.

**Security claim of this PR:** transport isolation and feature gating only. **End-user
OpenPGP security depends entirely on the out-of-tree handler, PDDB/key policy,
and factory provisioning procedures.**

### Trust boundaries

```
  [ USB host ]     untrusted; may send arbitrary CCID bytes
       |
  [ ccid_transport / usb-bao1x ]   trusted for framing only; no semantic checks
       |
  [ IPC: CcidRxDeferred / CcidTx ]   capability boundary; one listener PID
       |
  [ OpenPGP handler service ]   MUST enforce APDU policy, crypto, authorization
       |
  [ PDDB ]   persistence; access controlled by PDDB server + basis policy
```

| Layer | May assume | Must not assume |
|-------|------------|-----------------|
| **USB host** | Device speaks CCID 1.1 bulk framing | Device validates APDUs, PINs, or OpenPGP policy |
| **`usb-bao1x` transport** | Handler will parse frames; USB stack is configured | Frames are well-formed CCID commands; host is benign |
| **IPC (`CcidRxDeferred` / `CcidTx`)** | Only registered handler receives frames | Handler is always running; multiple handlers coordinate |
| **OpenPGP handler** | Transport delivers full raw frames | xous-core filtered dangerous APDUs; host is authenticated |
| **PDDB** | Keys exist after successful `save_provisioned_pins` | PIN lines are secret from other processes without PDDB access |

#### Single-listener `Denied` rule

`usb-bao1x` allows **one process** to hold a deferred `CcidRxDeferred` wait
(the first PID wins, same pattern as FIDO). A second process receives
`CcidCode::Denied`.

This matters because the handler receives **complete host-origin frames** that
may trigger signing, PIN prompts, or key operations. Allowing multiple
competing listeners would create ambiguous dispatch, possible double-processing,
or a confused-deputy path where the wrong service responds on bulk IN. The
handler process should be treated as part of the trusted computing base for
smart-card operations.

### Host attack surface

`usb-bao1x` **forwards host CCID frames blindly by design**. It does not:

- Reject unknown `bMessageType` values
- Cap command rates
- Inspect XfrBlock payloads for APDU content
- Enforce ordering beyond USB reassembly

Implications for the handler:

1. **Treat every received frame as hostile.** Parse strictly against CCID and
   APDU/T=1 rules; reject oversize, truncated, or nonsensical messages.
2. **Do not echo or reflect host bytes** in production (see `ccid-echo` below).
3. **Rate-limit expensive operations** (sign, decrypt, PIN verify) in the handler;
   the transport will keep delivering frames as fast as the host sends them.
4. **Never log secrets** from frame payloads at the transport layer; handler
   logging policy is handler-owned.
5. **USB disconnect** (`CcidCode::Hangup`) is signaled when the gadget is not
   configured; handler should drop partial transaction state.

A malicious host with physical USB access cannot directly read PDDB through this
interface, but it **can probe the handler** with arbitrary CCID/APDU traffic once
the handler is running.

### Production vs HIL: `ccid-echo` security boundary

| Build target | Features | CCID behavior | Intended use |
|--------------|----------|---------------|--------------|
| `cargo xtask baosec` | none (default) | No CCID interface; unchanged vs upstream `dev` | Default production image |
| `cargo xtask baosec-ccid` | `ccid-openpgp` | Frames go to IPC handler only | CCID transport + provisioning |
| `cargo xtask ccid-hil` | `ccid-openpgp` + **`ccid-echo`** | IRQ path **echoes host frames on bulk IN** without handler | Lab / bench HIL only |

**`ccid-echo` must never ship in production images.**

With `ccid-echo` enabled, any host that can write bulk OUT receives the same
bytes back on bulk IN. That:

- Bypasses the OpenPGP handler entirely for CCID replies.
- Creates a trivial protocol oracle useful for transport testing but **unsafe**
  if mistaken for a smart-card implementation.
- Must not be combined with tools (`pcscd`, GnuPG) that interpret responses as
  genuine card replies.

Production CCID builds use the dedicated `baosec-ccid` xtask target, which adds
`ccid-openpgp` but **does not** add `ccid-echo`. Only `ccid-hil` enables echo.

**Release checklist:** verify the flashed image was built with `ccid-hil` only on
test benches; confirm `ccid-echo` is absent from production feature sets.

### Provisioning trust model

First-boot provisioning exposes an **extra CDC ACM serial port** when PDDB
`usb.ccid` / `provisioned` is not `OKV1`.

#### Who may write the two PIN lines?

**Any party that can open the provisioning serial port on USB** while the device
is in the unprovisioned state. xous-core performs **no authentication** of the
host or tool:

- No pairing code
- No physical button confirmation in this layer
- No certificate or factory credential check

The intended trust model is **controlled factory or owner setup**:

- Device is provisioned in a trusted environment before untrusted USB exposure, **or**
- The first host to reach the provisioning port during the provisioning window
  defines the stored lines (similar to many "setup over USB" flows).

Operators should treat an **unprovisioned device on a hostile USB bus** as
vulnerable to provisioning capture: an attacker could write their own two lines
before the legitimate operator.

#### Attacker reaches provisioning CDC before factory setup

If an attacker connects first on an unprovisioned device:

1. They can send two lines and commit provisioning (`save_provisioned_pins`).
2. PDDB stores `user_pin_line`, `admin_pin_line`, and sets `provisioned = OKV1`.
3. USB resets; the provisioning port **disappears permanently** until factory reset.
4. The legitimate factory tool later sees an already-provisioned device and cannot
   overwrite lines through this USB path without reset.

**Mitigation is operational, not cryptographic in xous-core:** keep devices
unprovisioned only in trusted physical custody; use factory-reset before
re-provisioning; consider shipping pre-provisioned from factory.

There is **no rollback** of provisioning via the CCID/USB path once `OKV1` is written.

#### Why no format validation in xous-core?

PIN lines are **opaque blobs** to `usb-bao1x`:

- Format, entropy, and derivation are defined by the OpenPGP / product layer
  (out of tree), not the transport crate.
- xous-core only filters wire bytes to printable ASCII (`>= 0x20`) and line
  delimiters (`\r`/`\n`), rejecting control characters on the serial path.
- Semantic validation (length, KDF input structure, forbidden patterns) belongs
  in the handler or factory tool where the format is defined.

This keeps the USB layer small and avoids duplicating policy that the handler
must enforce anyway when reading PDDB.

#### What is stored in PDDB and who can read it later?

Written by `ccid_store.rs` into dictionary **`usb.ccid`**:

| Key | Max size | Content |
|-----|----------|---------|
| `user_pin_line` | 256 bytes | First provisioning line (opaque) |
| `admin_pin_line` | 256 bytes | Second provisioning line (opaque) |
| `provisioned` | 32 bytes | Marker `OKV1` when complete |

**Who can read:** any Xous process that can open these PDDB keys through the
normal PDDB API for the active basis. xous-core does not add a separate ACL on
top of PDDB; access follows [PDDB basis and dictionary
policy](https://betrusted.io/xous-book/ch09-00-pddb-overview.html). In
practice, the OpenPGP handler and other privileged services in the product TCB
should read these keys; unprivileged apps must not receive PDDB handles for
`usb.ccid`.

**Who can write after provisioning:** not via the provisioning CDC (port removed).
Further updates require factory reset or a product-specific PDDB update path
defined outside this PR.

**Host visibility:** lines are **not** exposed over CCID bulk. They travel only
on the provisioning CDC during the one-time window, then live in PDDB on device.

---

## Architecture overview

```
  Linux host (PC or Raspberry Pi)
  +---------------------------+
  | pyusb / pcscd / GnuPG     |
  |   PC_to_RDR / RDR_to_PC   |
  +-------------+-------------+
                | USB bulk IN/OUT (CCID class 0x0B)
                v
  +---------------------------+
  | usb-bao1x (xous-core)     |
  |  ccid_transport.rs        |  framing only
  +-------------+-------------+
                | IPC (CcidRxDeferred / CcidTx)
                v
  +---------------------------+
  | OpenPGP handler service   |  (out of tree, e.g. baochip-openpgp)
  |  APDU / card logic        |
  +---------------------------+
```

On the device, the composite USB gadget also exposes:

- HID keyboard + FIDO (existing personalities)
- Debug CDC serial (logging)
- **Provisioning CDC serial** (first-boot PIN lines, `ccid-openpgp` only)
- **CCID bulk interface** (`ccid-openpgp` only)

## USB identification

| Board   | VID    | PID    | Product string |
|---------|--------|--------|----------------|
| baosec  | 0x1d50 | 0x6198 | Baosec         |
| dabao   | 0x1d50 | 0x6197 | Dabao          |

Manufacturer string: `Baochip`.

On Linux, expect something like:

```bash
lsusb -d 1d50:6198
# ...
# iInterface 5 CCID Interface   # class 0x0B
# iInterface 6 ...             # provisioning CDC (if unprovisioned)
```

---

## Feature flags and firmware images

Defined in `services/usb-bao1x/Cargo.toml`:

| Feature | Depends on | Effect |
|---------|------------|--------|
| `ccid-openpgp` | `pddb` | CCID bulk transport + provisioning CDC + PDDB storage |
| `ccid-echo` | `ccid-openpgp` | Echo every received `PC_to_RDR` frame on bulk IN (HIL only) |

Build commands:

```bash
# Default baosec image (no CCID; matches upstream dev)
cargo xtask baosec

# Production CCID transport + provisioning (handler must be added separately)
cargo xtask baosec-ccid

# HIL test image (adds ccid-echo; no external handler needed for USB tests)
cargo xtask ccid-hil
```

When `ccid-openpgp` is enabled, the provisioning CDC interface is created only
while PDDB lacks `usb.ccid/provisioned=OKV1`. After provisioning, two bulk
endpoints are freed so the composite gadget stays within the Corigine endpoint
budget (`CRG_EP_NUM = 8`).

Compile-only checks without flashing:

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
```

---

## CCID USB interface

The CCID interface follows USB CCID 1.1-style descriptors as implemented in
`services/usb-bao1x/src/ccid_transport.rs`:

| Field | Value | Notes |
|-------|-------|-------|
| Interface class | 0x0B | CCID |
| bcdCCID | 0x0110 | CCID 1.10 |
| dwProtocols | 0x00000002 | T=1 |
| dwMaxCCIDMessageLength | 0x10F (271) | Max payload in one message |
| Bulk max packet | 512 bytes | High-speed |
| Wire maximum | 530 bytes | 10-byte header + payload |

Endpoints:

- Bulk OUT — host sends `PC_to_RDR_*` frames
- Bulk IN — device sends `RDR_to_PC_*` frames
- Interrupt IN — stub (notifications not implemented)

### Message framing

Every CCID bulk message on the wire is:

```
 byte 0       : bMessageType
 bytes 1..4   : dwLength (little-endian payload length)
 byte 5       : bSlot
 byte 6       : bSeq
 bytes 7..9   : header padding / fields (message-specific)
 bytes 10..   : payload (dwLength bytes)
```

Total message size = 10 + dwLength, capped at 530 bytes (`CCID_WIRE_MAX`).

The device assembles complete frames from one or more 512-byte bulk OUT
transactions before notifying software. Replies are queued as raw
`RDR_to_PC` bytes and chunked on bulk IN.

Common `bMessageType` values used in tests (`tools/ccid_hil/ccid_usb.py`):

| Value | Name | Direction |
|-------|------|-----------|
| 0x65 | PC_to_RDR_GetSlotStatus | Host to reader |
| 0x6F | PC_to_RDR_XfrBlock | Host to reader (carries APDU payload) |
| 0x81 | RDR_to_PC_DataBlock | Reader to host |
| 0x81 | RDR_to_PC_SlotStatus | Reader to host |

xous-core does **not** interpret these message types. It forwards complete
frames to an external handler over IPC, or (in HIL images with `ccid-echo`)
reflects the frame verbatim on bulk IN.

### What is not in xous-core

- Slot power management (`IccPowerOn` handling)
- APDU parsing or OpenPGP card emulation
- `pcscd` integration or IFD driver
- CCID interrupt notifications (card insert/remove)

Those belong in the OpenPGP handler service or host-side stack once the
transport layer is verified.

## Xous IPC API (device-side)

Gated by `ccid-openpgp` and `target_os = "xous"`. Defined in
`services/usb-bao1x/src/api.rs`.

| Opcode | ID | Purpose |
|--------|----|---------|
| `CcidRxDeferred` | 640 | Block until a complete `PC_to_RDR` frame is available |
| `CcidRxTimeout` | 641 | Timeout pump (reserved) |
| `CcidTx` | 642 | Enqueue raw `RDR_to_PC` bytes for bulk IN |
| `IrqCcidRx` | 770 | IRQ notification: frame ready |
| `IrqProvSerialRx` | 771 | Provisioning CDC line ready |

`CcidMsgIpc { data: Vec<u8>, code: CcidCode }` mirrors the existing U2F deferred
pattern (`U2fMsgIpc`).

An external service should:

1. Lend on `CcidRxDeferred` with `CcidCode::RxWait`
2. Receive `CcidCode::RxAck` and the raw host frame in `data`
3. Build a response frame and send it via `CcidTx` with `CcidCode::Tx`
4. Receive `CcidCode::TxAck`

## First-boot provisioning (CDC serial)

When PDDB dict `usb.ccid` / key `provisioned` is not set to `OKV1`, the device
opens a **second CDC ACM serial port** and captures two opaque lines:

1. First line (user PIN line) — stored temporarily
2. Second line (admin PIN line) — triggers PDDB write

On success, `ccid_store.rs` writes:

| PDDB key | Content |
|----------|---------|
| `usb.ccid` / `user_pin_line` | First line (opaque) |
| `usb.ccid` / `admin_pin_line` | Second line (opaque) |
| `usb.ccid` / `provisioned` | `OKV1` |

The USB stack then resets (PMIC unplug on baosec, or `force_reset` elsewhere)
and re-enumerates with provisioning CDC disabled.

Lines are terminated by `\r` or `\n`. Printable bytes (>= 0x20) are accepted;
no format validation is performed in xous-core.

## HIL test personality (`ccid-echo`)

For transport testing without an OpenPGP handler, build with the `ccid-echo`
feature (included in `cargo xtask ccid-hil`):

```
Host bulk OUT  --->  device assembles frame  --->  bulk IN echoes same bytes
```

This validates USB descriptors, bulk endpoints, and framing only.

## Testing guide

This section describes how to run every test tier: without hardware (developer
machine), on a Linux USB host (desktop or Pi), and in CI.

### Prerequisites by test type

| Test | Device image | Host | Required tools |
|------|--------------|------|----------------|
| Unit tests | none | any Linux/macOS with Rust | `cargo` |
| Compile gates | none | Linux (CI uses Ubuntu) | `cargo xtask install-toolkit` for board target |
| Smoke test | `ccid-hil` (has `ccid-echo`) | Linux USB host | `pyusb` |
| HIL suite | `ccid-hil` | Linux USB host | `pyusb`, `pyserial`, `lsusb` |
| Provisioning HIL | factory-reset device (unprovisioned) | Linux USB host | `pyserial` + above |
| Production CCID | `baosec` with `ccid-openpgp` | Linux + OpenPGP handler | handler service (out of tree) |

Build the HIL test image:

```bash
cargo xtask ccid-hil
```

Flash the resulting image to the device before any USB host test. Connect the
device USB **data** port to the host (Pi or PC).

### Tier 1: Unit tests (no hardware)

From the repository root:

```bash
cargo test -p usb-bao1x --lib ccid_framing
```

Expected output ends with `7 passed`. These tests cover:

- Partial-frame handling (no premature parse)
- Valid `GetSlotStatus` frame extraction
- Oversize `dwLength` rejection
- Multi-packet bulk OUT reassembly
- TX chunking at 512 bytes
- PDDB provisioning marker (`OKV1`)

### Tier 2: Compile gates (no hardware)

Hosted (fast sanity check):

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
```

Board target (requires Xous toolkit):

```bash
cargo xtask install-toolkit --force --no-verify
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
```

Full HIL image compile:

```bash
cargo xtask ccid-hil --no-verify
```

These run automatically on every PR via `.github/workflows/ccid-ci.yml`.

### Tier 3: USB smoke test (hardware, ~30 seconds)

Use this after flashing a `ccid-hil` image to confirm enumeration and bulk
echo in one step.

1. Install host dependencies:

```bash
pip install pyusb
# Linux: ensure user can access USB (see udev rules below)
```

2. Confirm the device is visible:

```bash
lsusb -d 1d50:6198
```

3. Run the smoke test from the repository root:

```bash
python3 tools/ccid_smoke.py
```

**Pass criteria:**

- Prints `Device enumerated.`
- CCID descriptor shows `bcd=0110` and `protocols=0x00000002`
- `GetSlotStatus echo OK.`
- `XfrBlock echo OK.`
- Final line: `PASS`

Useful flags:

```bash
python3 tools/ccid_smoke.py --timeout 120        # slow enumerators
python3 tools/ccid_smoke.py --skip-echo           # enumeration only
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197   # dabao
```

### Tier 4: Full HIL suite (hardware, ~2 minutes)

Runs individual tests in sequence and writes logs to `/tmp/ccid-hil-out/`.

```bash
pip install pyusb pyserial
chmod +x tools/ccid_hil/*.sh
tools/ccid_hil/run_all.sh
```

| Step | Script | What it checks | Pass line |
|------|--------|----------------|-----------|
| 00 | `wait_device.sh` | USB device `1d50:6198` appears | `Device 1d50:6198 present` |
| 01 | `test_enumerate.py` | CCID interface class 0x0B, descriptor fields | `HIL-01 PASS` |
| 02 | `test_provision.py` | skipped unless `CCID_HIL_PROVISION=1` | `HIL-02 PASS` |
| 03 | `test_echo.py` | GetSlotStatus bulk round-trip echo | `HIL-03 PASS` |
| 04 | `test_echo.py --stress N` | N random XfrBlock echo frames | `HIL-05 PASS` |

Run individual tests:

```bash
export PYTHONPATH=tools/ccid_hil

python3 tools/ccid_hil/test_enumerate.py
python3 tools/ccid_hil/test_echo.py
python3 tools/ccid_hil/test_echo.py --stress 100
```

Environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor (hex, no `0x` prefix) |
| `CCID_PID` | `6198` | USB product ID |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | Set to `1` to run provisioning test |
| `CCID_HIL_STRESS` | `100` | Random echo iterations in step 04 |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

#### Provisioning test (optional)

Only works on an **unprovisioned** device (PDDB `usb.ccid` / `provisioned` is not
`OKV1`). Factory-reset or use a fresh image, then:

```bash
CCID_HIL_PROVISION=1 tools/ccid_hil/run_all.sh
```

The test opens the provisioning CDC serial port, sends two lines, and waits for
USB re-enumeration. List serial ports if auto-detection fails:

```bash
python3 -m serial.tools.list_ports
python3 tools/ccid_hil/test_provision.py --port /dev/ttyACM1
```

### Tier 5: CI (automated)

**GitHub-hosted** (every push/PR to `main` or `dev`):

- Workflow: `.github/workflows/ccid-ci.yml`
- Runs unit tests + compile gates (no device attached)

**Self-hosted Raspberry Pi** (nightly or manual dispatch):

- Workflow: `.github/workflows/ccid-hil.yml`
- Requires runner labels: `self-hosted`, `baosec-hil`
- Device must be cabled to the Pi and flashed with a `ccid-hil` image
- Logs uploaded as `ccid-hil-logs` artifact

Trigger manually from GitHub: Actions -> CCID HIL -> Run workflow.

### End-to-end test workflow on Raspberry Pi

Typical bench session after initial Pi setup (see below):

```bash
cd ~/xous-core
git pull
source ~/ccid-venv/bin/activate

# 1. Build test image (or build on desktop and copy flash artifact)
cargo xtask ccid-hil

# 2. Flash image to device (use your normal baosec flash procedure)

# 3. Cable device USB to Pi, power on device

# 4. Quick check
lsusb -d 1d50:6198
python3 tools/ccid_smoke.py

# 5. Full regression
tools/ccid_hil/run_all.sh

# 6. Review logs if anything fails
ls -la /tmp/ccid-hil-out/
cat /tmp/ccid-hil-out/summary.log
```

### Interpreting failures

| Failure | Check |
|---------|-------|
| `Timeout waiting for CCID device` | Cable, power, image flashed, `lsusb` output |
| `CCID interface not found` | Image missing `ccid-openpgp`; rebuild with `ccid-hil` |
| `echo mismatch` | Image missing `ccid-echo`; host sent before device configured |
| `pyusb` permission error | udev rules, group membership, re-login |
| Provisioning: no re-enumeration | Wrong serial port; device already provisioned |
| Unit test / compile failure | Run the exact `cargo` command from CI log locally |

## Raspberry Pi HIL setup

The goal is a self-contained Linux USB host that can flash a test image, wait
for enumeration, and run the Python HIL suite — suitable as a GitHub Actions
self-hosted runner or a bench setup.

### Hardware

```
  +----------------+          USB-A
  | Raspberry Pi 4 |----------------------> baosec device
  | (or Pi 5)      |          (data port)
  +----------------+
        |
        optional: UART adapter to device DUART for serial logs
```

Recommendations:

- Pi 4 or 5 with a **USB-A host port** (or a **powered** USB hub; CCID bulk
  tests are sensitive to underpowered hubs)
- Short, data-rated USB cable to the baosec device
- Network connection for the Pi (runner registration, artifact fetch)

### Operating system

1. Flash **Raspberry Pi OS Lite (64-bit)** or Ubuntu Server for Raspberry Pi.
2. Enable SSH and set hostname, e.g. `baosec-hil`.
3. Update packages:

```bash
sudo apt update
sudo apt install -y git python3-pip python3-venv usbutils \
  libusb-1.0-0-dev build-essential pkg-config libxkbcommon-dev
```

### USB permissions (udev)

Create `/etc/udev/rules.d/99-baosec-ccid.rules`:

```
# baosec CCID + CDC interfaces
SUBSYSTEM=="usb", ATTR{idVendor}=="1d50", ATTR{idProduct}=="6198", MODE="0666", GROUP="plugdev"
SUBSYSTEM=="tty", ATTRS{idVendor}=="1d50", ATTRS{idProduct}=="6198", MODE="0666", GROUP="dialout"
```

Then:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
sudo usermod -aG plugdev,dialout $USER
# log out and back in
```

Verify:

```bash
lsusb -d 1d50:6198
```

### Clone xous-core and install toolchain

```bash
git clone https://github.com/betrusted-io/xous-core.git
cd xous-core
cargo xtask install-toolkit --force --no-verify
```

Install Python test dependencies:

```bash
python3 -m venv ~/ccid-venv
source ~/ccid-venv/bin/activate
pip install pyusb pyserial
```

### Build and flash the HIL firmware image

On a build machine (can be the Pi, but cross-build from a desktop is faster):

```bash
cargo xtask ccid-hil
```

This produces a baosec image with `ccid-openpgp` and `ccid-echo` enabled.
Flash using your normal baosec update path (USB boot loader, JTAG, or internal
update flow — follow existing betrusted flashing documentation for your hardware
revision).

After flash, connect the device USB data port to the Pi and confirm CCID
enumeration:

```bash
lsusb -d 1d50:6198 -v 2>/dev/null | grep -A2 "bInterfaceClass"
# Expect an interface with bInterfaceClass 11 (0x0B)
```

### Run tests manually on the Pi

From the repository root:

```bash
source ~/ccid-venv/bin/activate

# Quick smoke test
python3 tools/ccid_smoke.py

# Full HIL suite (enumeration, echo, stress)
tools/ccid_hil/run_all.sh

# Include provisioning test (factory-reset / unprovisioned device only)
CCID_HIL_PROVISION=1 tools/ccid_hil/run_all.sh
```

Logs are written to `/tmp/ccid-hil-out/` by default.

### GitHub Actions self-hosted runner (optional)

To run `.github/workflows/ccid-hil.yml` nightly on the Pi:

1. On the Pi, register a self-hosted runner for `betrusted-io/xous-core` with
   labels: `self-hosted`, `baosec-hil`.
2. Ensure the runner user is in `plugdev` and `dialout`.
3. Install Rust and the Xous toolkit on the runner (same as above).
4. The workflow builds `cargo xtask ccid-hil` and runs `tools/ccid_hil/run_all.sh`.

The workflow does not flash automatically today; flash the HIL image once on the
bench (or extend the runner script with your flashing command). After a
successful flash, nightly runs validate transport regressions.

### Troubleshooting

| Symptom | Likely cause |
|---------|----------------|
| `lsusb` shows device but pyusb fails | udev permissions; try `sudo` once to confirm |
| No CCID interface (class 0x0B) | Image built without `ccid-openpgp`; rebuild with `ccid-hil` |
| Echo test times out | Device not running `ccid-echo`; host not configured; bad cable |
| Provisioning test fails | Device already provisioned (`OKV1` in PDDB); factory reset required |
| `Resource busy` on serial port | Wrong CDC port; list ports with `python3 -m serial.tools.list_ports` |

## CI summary

| Tier | Where | What |
|------|-------|------|
| Unit tests | GitHub-hosted | `ccid_framing` (7 tests) |
| Compile + image | GitHub-hosted | `ccid-ci.yml`, `build.yml` / `baosec` |
| HIL transport | Pi self-hosted | Enumeration, echo, stress |
| OpenPGP E2E | Out of tree | `gpg --card-status` with handler service |

See also [`docs/CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md) for recorded
verification results and [`docs/code_map.md`](code_map.md) for source navigation.
