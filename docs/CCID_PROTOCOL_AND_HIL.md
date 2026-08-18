<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID smart-card transport and Raspberry Pi HIL setup

This document is the main reference for USB CCID support in `usb-bao1x`
([PR #890](https://github.com/betrusted-io/xous-core/pull/890)). It explains
what a smart card is in this context, what xous-core implements, how an external
OpenPGP handler plugs in, and how to run automated transport tests on a
Raspberry Pi or desktop Linux host.

**Audience:** reviewers who are not CCID experts, firmware developers wiring an
APDU handler, and anyone setting up hardware-in-the-loop (HIL) regression tests.

**Code navigation:** [`docs/code_map.md`](code_map.md) — debug decision tree,
USB enumeration flow, symptom-to-source map, and host `lsusb` checks (start here
for "device not visible" or CCID issues).

**Enumeration deep-dive (community):** [`docs/CCID_USB_ENUMERATION_DEBUG.md`](CCID_USB_ENUMERATION_DEBUG.md)
— worked example of Corigine endpoint-budget overflow, static diagnostic method,
Persona A trade-offs; not official support.

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
11. [PIN provisioning (offline / non-USB on CCID images)](#pin-provisioning-offline--non-usb-on-ccid-images)
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
| Persist / read opaque PIN lines in PDDB (`OKV1`) | Provisioning storage | **Optional** (`ccid-pddb` / `ccid_store.rs`); **no USB CDC capture**; not called at boot |
| Parse most `PC_to_RDR_*` message types | Protocol | **No** (exceptions: GetSlotStatus and IccPowerOn answered inline) |
| `PC_to_RDR_GetSlotStatus` → `RDR_to_PC_SlotStatus` | Transport | **Yes** (IRQ path; see framing / CreateChannel note) |
| `PC_to_RDR_IccPowerOn` → `RDR_to_PC_DataBlock` + OpenPGP ATR | Transport | **Yes** (IRQ path; ATR bytes in `ccid_framing::OPENPGP_ATR`) |
| T=1 block protocol, APDU parsing | Card protocol | **No** |
| OpenPGP card emulation, key storage, crypto | Application | **No** (external service, e.g. `baochip-openpgp` / stub) |
| `pcscd` driver, GnuPG integration | Host stack | **No** |
| CCID interrupt notifications (insert/remove) | Transport | **No** (interrupt IN endpoint omitted) |

The Cargo feature is named `ccid-openpgp` for product alignment, but **no
OpenPGP crates** are linked into xous-core. All cryptography stays
out of tree behind IPC.

Everything is gated behind `ccid-openpgp` on Xous builds so default `baosec`
images are unaffected when the feature is disabled.

---

## Security considerations

This section is for merge review of [PR #890](https://github.com/betrusted-io/xous-core/pull/890).
The PR adds a **potentially security-sensitive USB surface** (CCID bulk transport)
plus PDDB helpers for opaque PIN blobs. It does **not** deliver OpenPGP security
by itself; it exposes transport and storage primitives that a handler and factory
process must use correctly.

### Threat model and non-goals

**In scope for this PR (xous-core):**

- Present a USB CCID bulk interface to a connected host.
- Reassemble and forward complete `PC_to_RDR` frames to one deferred IPC listener.
- Accept complete `RDR_to_PC` reply blobs from that listener and stream them on bulk IN.
- Optional PDDB helpers (`ccid-pddb` / `ccid_store.rs`) to store PIN lines remain for offline seeding. **No xtask image enables `ccid-pddb`**, and `main.rs` does **not** call PDDB at boot (that blocked USB init).
- **Persona A:** CCID images do **not** expose USB CDC for debug or PIN provisioning (Corigine 8-endpoint budget).

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
| `cargo xtask baosec` | none (default) | No CCID interface; unchanged vs upstream `dev` | Default baosec image |
| `cargo xtask dabao-ccid` | `ccid-openpgp` | Frames go to IPC handler only | **Hardware-confirmed** CCID on Dabao (no USB CDC) |
| `cargo xtask baosec-ccid` | `ccid-openpgp` | Frames go to IPC handler only | Baosec CCID transport (no USB CDC) |
| `cargo xtask ccid-hil` | `ccid-openpgp` + **`ccid-echo`** | IRQ path **echoes host frames on bulk IN** without handler | Lab / bench HIL only |

**`ccid-echo` must never ship in production images.**

With `ccid-echo` enabled, any host that can write bulk OUT receives the same
bytes back on bulk IN. That:

- Bypasses the OpenPGP handler entirely for CCID replies.
- Creates a trivial protocol oracle useful for transport testing but **unsafe**
  if mistaken for a smart-card implementation.
- Must not be combined with tools (`pcscd`, GnuPG) that interpret responses as
  genuine card replies.

Production CCID builds use `dabao-ccid` or `baosec-ccid`, which add
`ccid-openpgp` but **do not** add `ccid-echo`. Only `ccid-hil` enables echo.
Dabao (`1d50:6197`) is the hardware target confirmed for CCID enumeration in
[`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md).

**Release checklist:** verify the flashed image was built with `ccid-hil` only on
test benches; confirm `ccid-echo` is absent from production feature sets.

### Provisioning trust model (Persona A)

CCID firmware images **do not** expose a USB provisioning CDC port. An untrusted
USB host therefore **cannot** write PIN lines through `usb-bao1x` on these images.

**What is implemented:**

| Path | Status |
|------|--------|
| (a) Offline / pre-flash PDDB already containing `usb.ccid` keys + `OKV1` | Supported operationally; `usb-bao1x` does not read it at boot |
| (b) Separate non-CCID image that exposes USB PIN provisioning | **Not implemented** in this tree (stock `baosec` has debug CDC but no PIN provision CDC) |
| (c) Skip USB provision if PDDB is already `OKV1` | **Implemented** as a no-op: there is no USB provision path |
| Unprovisioned CCID image (`OKV1` missing) | USB still enumerates as CCID+FIDO+NKRO; no USB PIN capture |

`ccid_store::save_provisioned_pins` remains in-tree for factory tools that seed
PDDB offline; it is **not** wired to any USB RX path on CCID builds.

**Mitigation:** ship images with PDDB already provisioned, or seed PDDB before
enabling a CCID image. There is **no** USB path to overwrite `OKV1` after flash.

#### Why no format validation in xous-core?

PIN lines are **opaque blobs** to `usb-bao1x`:

- Format, entropy, and derivation are defined by the OpenPGP / product layer
  (out of tree), not the transport crate.
- Semantic validation belongs in the handler or factory tool that seeds PDDB.

#### What is stored in PDDB and who can read it later?

Written by `ccid_store.rs` into dictionary **`usb.ccid`** (typically offline /
factory seed; not via USB on Persona A images):

| Key | Max size | Content |
|-----|----------|---------|
| `user_pin_line` | 256 bytes | First provisioning line (opaque) |
| `admin_pin_line` | 256 bytes | Second provisioning line (opaque) |
| `provisioned` | 32 bytes | Marker `OKV1` when complete |

**Who can read:** any Xous process that can open these PDDB keys through the
normal PDDB API for the active basis. xous-core does not add a separate ACL on
top of PDDB; access follows [PDDB basis and dictionary
policy](https://betrusted.io/xous-book/ch09-00-pddb-overview.html).

**Who can write:** not over USB on CCID images. Further updates require factory
reset or a product-specific PDDB update path outside this PR.

**Host visibility:** lines are **not** exposed over CCID bulk.

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
  | OpenPGP handler service   |  (out of tree)
  |  APDU / card logic        |
  +---------------------------+
```

Data flow for a normal (non-echo) production image:

1. After USB `SET_ADDRESS`, Corigine bulk OUT endpoints are **primed** in
   `set_device_address` (first receive TRB after `ep_enable`). Without this,
   host `WriteUSB` times out and `pcscd` never finishes `RFAddReader`.
2. Host sends CCID bulk OUT data; `ccid_transport` reassembles a complete
   `PC_to_RDR` frame.
3. **`PC_to_RDR_GetSlotStatus` (0x65)** is answered **inline in the IRQ path**
   with a fixed `RDR_to_PC_SlotStatus` (does not wake the stub / handler).
   libccid CreateChannel issues two GetSlotStatus probes with a **100 ms**
   ReadUSB timeout (`readTimeout * 100 / DEFAULT_COM_READ_TIMEOUT`); IPC
   round-trips are too slow for that window.
4. **`PC_to_RDR_IccPowerOn` (0x62)** is also answered **inline** with
   `RDR_to_PC_DataBlock` carrying `OPENPGP_ATR` so `pcscd` sees a card present
   before the handler is ready.
5. Other complete frames are queued; `IrqCcidRx` notifies `usb-bao1x`.
6. A deferred listener (external handler / stub) receives the raw frame bytes
   via `CcidRxDeferred`.
7. Handler parses the CCID message, runs APDU logic, builds an `RDR_to_PC`
   response frame.
8. Handler sends raw response bytes with `CcidTx`; transport chunks them on
   bulk IN (main loop triggers a soft IRQ so `poll_bulk_in` runs promptly).

---

## USB composite device

Corigine UDC exposes **8 unidirectional non-EP0 endpoint slots** (`CRG_EP_NUM`).
That hardware budget is why CCID images drop USB CDC.

| Image / feature set | Classes | Unidirectional EPs | Fits? |
|---------------------|---------|--------------------|-------|
| `baosec` (no `ccid-openpgp`) | FIDO (2) + NKRO (2) + debug CDC (3) | **7 / 8** | yes |
| `baosec-ccid` / `ccid-hil` / `dabao-ccid` (`ccid-openpgp`) | CCID (2) + FIDO (2) + NKRO (2) | **6 / 8** | yes |
| (rejected) CCID interrupt + FIDO + NKRO | interrupt IN collided with NKRO | broke enum | — |
| (rejected) CCID + FIDO + NKRO + debug CDC | 2+2+2+3 | **9 / 8** | no |
| (rejected) above + provision CDC | +3 | **12 / 8** | no |

**Persona A (`ccid-openpgp`):** composite is **CCID + FIDO + NKRO only**. Debug
CDC and provisioning CDC are **not** allocated. Debug/`log` output uses the
existing **`xous-log` UART / DUART** path (`services/xous-log/.../bao1x`), same
as pre-USB-serial and non-CDC platforms — not a new UART driver in `usb-bao1x`.
`EpBudgetLedger` verifies the **cumulative** class total (with per-class sanity
checks kept) before each allocating constructor and against
`allocated_non_ep0` after build — see `ep_budget` tests /
`tools/test_ep_budget_cumulative.py`.

| Interface | When present | Purpose |
|-----------|--------------|---------|
| CCID bulk (`0x0B`) | `ccid-openpgp` | Smart-card transport |
| FIDO + NKRO HID | Always (baosec USB) | Existing HID |
| Debug CDC ACM | **Not** on CCID images | Use UART / `xous-log` instead |
| Provisioning CDC ACM | **Never** on CCID images | Offline PDDB seed only |

### USB identification

| Board   | VID    | PID    | Product string |
|---------|--------|--------|----------------|
| baosec  | 0x1d50 | 0x6198 | Baosec         |
| dabao   | 0x1d50 | 0x6197 | Dabao          |

Manufacturer string: `Baochip`.

On Linux, expect something like:

```bash
lsusb -d 1d50:6197   # dabao (hardware-confirmed CCID)
# or: lsusb -d 1d50:6198   # baosec
# class 0x0B CCID Interface on dabao-ccid / baosec-ccid / ccid-hil
# no CDC ACM for debug or provisioning on those images
# CCID bulk wMaxPacketSize = 512 (high-speed)
```

---

## Feature flags and firmware images

Defined in `services/usb-bao1x/Cargo.toml`:

| Feature | Depends on | Effect |
|---------|------------|--------|
| `ccid-openpgp` | (none) | CCID bulk transport; **no USB CDC** (Persona A); no `pddb` |
| `ccid-pddb` | `dep:pddb`, `ccid-openpgp` | Optional offline PDDB provisioning helpers (baosec) |
| `ccid-echo` | `ccid-openpgp` | Echo every received `PC_to_RDR` frame on bulk IN (HIL only) |

Build commands:

```bash
# Dabao CCID (hardware-confirmed: 1d50:6197, HS bulk MPS 512)
cargo xtask dabao-ccid

# Default baosec image (no CCID; FIDO+NKRO+debug CDC)
cargo xtask baosec

# Baosec CCID transport (handler must be added separately; UART debug)
cargo xtask baosec-ccid

# HIL test image (adds ccid-echo; no external handler needed for USB tests)
cargo xtask ccid-hil
```

Compile-only checks without flashing:

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
# Include modals so ux-api default-widgets match the baosec image (CI does this).
cargo check -p usb-bao1x -p modals --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
cargo xtask baosec-ccid --no-verify
cargo xtask dabao-ccid --no-verify
```

---

## CCID USB interface

Implementation: `services/usb-bao1x/src/ccid_transport.rs` (CCID 1.1-style
descriptors).

| Field | Value | Notes |
|-------|-------|-------|
| Interface class | 0x0B | CCID |
| bcdCCID | 0x0110 | CCID 1.10 |
| bMaxSlotIndex | 0x00 | One slot (index 0) |
| dwProtocols | 0x00000002 | T=1 (protocol number 1 in bitfield) |
| dwMaxCCIDMessageLength | 0x10F (271) | Max payload in one CCID message |
| dwFeatures | 0x000400FE | Short APDU; character level; etc. |
| Bulk max packet | 512 bytes | High-speed |
| Wire maximum | 271 bytes (`0x10F`) | Short APDU `CCID_WIRE_MAX` |

Endpoints:

- **Bulk OUT** — host sends `PC_to_RDR_*` frames (possibly split across 512-byte
  USB transactions). Primed after `set_device_address` / `ep_enable`.
- **Bulk IN** — device sends `RDR_to_PC_*` frames (chunked at 512 bytes)
- **Interrupt IN** — **omitted** (Corigine `alloc_ep` pairing caused NKRO EP
  collision / host `EPROTO` when a lone CCID interrupt IN was allocated)

---

## Message framing on the wire

Every CCID bulk message:

```
 byte 0       : bMessageType
 bytes 1..4   : dwLength (little-endian payload length)
 byte 5       : bSlot
 byte 6       : bSeq
 bytes 7..9   : header padding / message-specific fields
 bytes 10..   : payload (dwLength bytes)
```

Total size = 10 + `dwLength`, capped at `CCID_WIRE_MAX` (271 bytes).

The device **buffers partial bulk OUT packets** until a full frame is available,
then delivers the complete byte vector to software. Replies are queued as one
contiguous `RDR_to_PC` buffer and streamed on bulk IN.

### Common message types

Used in HIL tests (`tools/ccid_hil/ccid_usb.py`) and typical OpenPGP reader
traffic:

| Value | Name | Direction | Role |
|-------|------|-----------|------|
| 0x62 | PC_to_RDR_IccPowerOn | Host to reader | Power-on; ATR returned **inline** |
| 0x65 | PC_to_RDR_GetSlotStatus | Host to reader | Poll slot state; answered **inline** |
| 0x6F | PC_to_RDR_XfrBlock | Host to reader | Carries T=1 / APDU payload |
| 0x80 | RDR_to_PC_DataBlock | Reader to host | Response data (often APDU response) |
| 0x81 | RDR_to_PC_SlotStatus | Reader to host | Slot status reply |

**Exceptions answered inline in the USB IRQ** (do **not** route to the
deferred stub/handler):

| Host message | Inline reply |
|--------------|--------------|
| `PC_to_RDR_GetSlotStatus` (0x65) | `rdr_to_pc_slot_status_ok` (10-byte `RDR_to_PC_SlotStatus`) |
| `PC_to_RDR_IccPowerOn` (0x62) | `rdr_to_pc_data_block_atr` (`RDR_to_PC_DataBlock` + `OPENPGP_ATR`) |

All other message types are forwarded as complete frames over IPC. Only the
external handler (or the `ccid-echo` test personality) interprets them.

#### Why GetSlotStatus is inline (100 ms libccid window)

On CreateChannel, libccid sends two GetSlotStatus resync probes and waits only
`readTimeout * 100 / DEFAULT_COM_READ_TIMEOUT` (typically **100 ms** when the
default read timeout is 3 s). Answering via `CcidRxDeferred` → stub → `CcidTx`
misses that window and used to make `RFAddReader` fail even when the rest of
the stack worked. Inline IRQ replies keep CreateChannel within budget so
`pcscd` can finish reader init and then exchange ATR / APDUs with the handler.

### Example CCID hex dumps

The examples below match what `tools/ccid_smoke.py` and `tools/ccid_hil/ccid_usb.py`
send on the wire. All multi-byte integers are **little-endian**. Offsets are
zero-based within each CCID message.

#### PC_to_RDR_GetSlotStatus (host to device)

Ten-byte header, no payload (`dwLength = 0`). Used by the smoke test with
`seq = 1`:

```
Offset  Field           Value
------  --------------  -----
0       bMessageType    0x65  (PC_to_RDR_GetSlotStatus)
1..4    dwLength        00 00 00 00
5       bSlot           0x00
6       bSeq            0x01
7..9    (reserved)      00 00 00

Full frame (10 bytes):
  65 00 00 00 00 00 01 00 00 00
```

On production / stub images, GetSlotStatus and IccPowerOn are answered inline
(see above). On a `ccid-echo` HIL image, other `PC_to_RDR` frames (for example
`XfrBlock`) are echoed unchanged on bulk IN.

#### PC_to_RDR_XfrBlock (host to device)

Header plus payload. Smoke test sends 32 payload bytes (`00`..`1f`) with
`seq = 2`:

```
Header (10 bytes):
  6f 20 00 00 00 00 02 00 00 00
  ^  ^-- dwLength = 0x20 (32)
  |-- PC_to_RDR_XfrBlock

Payload (32 bytes):
  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f
  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f

Full frame (42 bytes):
  6f 20 00 00 00 00 02 00 00 00
  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f
  10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f
```

#### Short APDU inside XfrBlock (illustrative)

When the host uses CCID short-APDU mode, the XfrBlock payload may be a raw
APDU. Example **SELECT** (`00 A4 04 00 00`), `seq = 3`:

```
  6f 05 00 00 00 00 03 00 00 00 00 a4 04 00 00
  ^  ^-- dwLength = 5              ^-- APDU bytes
  |-- XfrBlock
```

Parsing T=1 block framing (when the host wraps APDUs in T=1) is the handler's
responsibility; `usb-bao1x` delivers the full XfrBlock payload verbatim.

#### RDR_to_PC_SlotStatus (device to host)

Example reply a handler might build after `GetSlotStatus` (`slot = 0`,
`seq = 1`, success, ICC present/active):

```
  81 00 00 00 00 00 01 00 00 00
  ^  ^-- dwLength = 0
  |-- RDR_to_PC_SlotStatus
      bSlot=0, bSeq=1, bStatus=0, bError=0, bClockStatus=0
```

#### RDR_to_PC_DataBlock (device to host)

Example reply carrying a minimal successful APDU status word `90 00`
(`seq = 2`):

```
  80 02 00 00 00 00 02 00 00 00 90 00
  ^  ^-- dwLength = 2                    ^-- response body
  |-- RDR_to_PC_DataBlock
```

#### Multi-packet bulk OUT (host to device)

USB bulk max packet size is 512 bytes. The host may split one CCID frame across
several OUT transactions; `ccid_transport` reassembles before IPC delivery.

`GetSlotStatus` frame split after byte 4 (from unit test `split_bulk_out_reassembles`):

```
Transaction 1 (4 bytes):  65 00 00 00
Transaction 2 (6 bytes):  00 00 01 00 00 00
Assembled IPC frame:      65 00 00 00 00 00 01 00 00 00
```

#### Multi-packet bulk IN (device to host)

Replies are streamed in 512-byte chunks. Frames up to `CCID_WIRE_MAX` (271)
fit in one high-speed bulk packet; larger TX buffers (unit-test / stress)
still chunk at 512. Capture with `usbmon` or pyusb read loops if debugging
partial reads on the host.

---

## APDUs, T=1, and XfrBlock

For OpenPGP use, the host stack eventually sends **`PC_to_RDR_XfrBlock`**
(0x6F). The CCID payload contains a **T=1 transport block** (Baosec advertises
T=1 via `dwProtocols`), and inside that block is the **APDU** the card
application must handle.

Example path (conceptual; parsing is handler responsibility):

```
GnuPG  -->  pcscd  -->  PC_to_RDR_XfrBlock  -->  usb-bao1x  -->  handler
                                                                  |
                                                            parse CCID
                                                            parse T=1
                                                            handle APDU
                                                            build RDR_to_PC_DataBlock
```

The handler must produce a **complete CCID response frame** (header + payload)
before calling `CcidTx`. `usb-bao1x` does not add or strip CCID headers on
behalf of the handler.

---

## Xous IPC API and handler integration

Gated by `ccid-openpgp` and `target_os = "xous"`. Server name:
`_Xous USB device driver_` (must not change; log crate depends on it).

Types in `services/usb-bao1x/src/api.rs`:

```rust
pub struct CcidMsgIpc {
    pub data: Vec<u8>,   // raw CCID frame bytes
    pub code: CcidCode,
}

pub enum CcidCode {
    Tx, TxAck, RxWait, RxAck, RxTimeout, Hangup, Denied,
}
```

Opcodes:

| Opcode | ID | Purpose |
|--------|----|---------|
| `CcidRxDeferred` | 640 | Block until a complete `PC_to_RDR` frame is available |
| `CcidRxTimeout` | 641 | Timeout pump (reserved) |
| `CcidTx` | 642 | Enqueue raw `RDR_to_PC` bytes for bulk IN |
| `IrqCcidRx` | 770 | IRQ notification: frame ready |

Pattern mirrors the existing U2F deferred API (`U2fRxDeferred` / `U2fTx`).

### Handler integration sequence

Only **one** process may hold a deferred CCID receive at a time (same rule as
FIDO).

```
Handler                          usb-bao1x                    USB host
   |                                 |                            |
   |-- CcidRxDeferred (RxWait) ----->|                            |
   |   [blocks]                      |<------ bulk OUT chunks ----|
   |                                 | assemble PC_to_RDR frame   |
   |                                 |                            |
   |<-- IrqCcidRx (optional) --------|                            |
   |                                 |                            |
   |<-- RxAck + data (full frame) ---|                            |
   |                                 |                            |
   |  parse frame, run APDU logic    |                            |
   |                                 |                            |
   |-- CcidTx (Tx + RDR_to_PC) ----->|                            |
   |<-- TxAck -----------------------|                            |
   |                                 |------ bulk IN chunks ----->|
```

Steps for an external service:

1. Connect to `_Xous USB device driver_`.
2. Send `CcidRxDeferred` with `CcidMsgIpc { data: vec![], code: RxWait }` using
   the lend/deferred reply pattern (see U2F handler code in-tree for a template).
3. When a frame arrives, receive `CcidCode::RxAck` and the raw host frame in
   `data`.
4. Parse CCID/T=1/APDU, perform card operations, serialize a full `RDR_to_PC`
   response into `data`.
5. Send `CcidTx` with `CcidCode::Tx` and response bytes; wait for `TxAck`.
6. Loop to step 2.

Provisioning lines are **not** delivered over CCID or USB CDC on Persona A
images. The OpenPGP handler reads stored lines from PDDB if they were seeded
offline (see [PIN provisioning](#pin-provisioning-offline--non-usb-on-ccid-images)).

### Handler skeleton (Rust)

Minimal out-of-tree Xous service showing the deferred receive / send pattern.
This mirrors `UsbHid::u2f_wait_incoming` / `u2f_send` in
`services/usb-bao1x/src/lib.rs`, adapted for variable-length CCID frames.

**Cargo.toml** (handler crate):

```toml
[dependencies]
xous = "0.9.70"
xous-ipc = "0.10.10"
xous-names = { package = "xous-api-names", version = "0.9.71" }
log = "0.4.14"
log-server = { package = "xous-api-log", version = "0.1.69" }
num-traits = "0.2.14"
usb-bao1x = { path = "../xous-core/services/usb-bao1x", features = ["ccid-openpgp"] }
```

**main.rs** (illustrative — replace `handle_apdu` with real OpenPGP logic):

```rust
//! Out-of-tree CCID / OpenPGP handler (skeleton).
//! Requires baosec image built with ccid-openpgp (not ccid-echo).

use num_traits::ToPrimitive;
use usb_bao1x::{CcidCode, CcidMsgIpc, Opcode};
use xous_ipc::Buffer;

const USB_SERVER: &str = "_Xous USB device driver_";

fn connect_usb() -> xous::CID {
    xous_names::XousNames::new()
        .expect("xous-names")
        .request_connection_blocking(USB_SERVER)
        .expect("usb-bao1x not running or ccid-openpgp disabled")
}

/// Block until usb-bao1x delivers a complete PC_to_RDR frame.
fn ccid_wait_frame(conn: xous::CID) -> Result<Vec<u8>, xous::Error> {
    let req = CcidMsgIpc {
        data: Vec::new(),
        code: CcidCode::RxWait,
    };
    let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
    buf.lend_mut(conn, Opcode::CcidRxDeferred.to_u32().unwrap())
        .or(Err(xous::Error::InternalError))?;
    let ack = buf.to_original::<CcidMsgIpc, _>().unwrap();
    match ack.code {
        CcidCode::RxAck => Ok(ack.data),
        CcidCode::Hangup => Err(xous::Error::ProcessTerminated),
        _ => Err(xous::Error::InternalError),
    }
}

/// Enqueue a complete RDR_to_PC frame for bulk IN transmission.
fn ccid_send_frame(conn: xous::CID, frame: Vec<u8>) -> Result<(), xous::Error> {
    let req = CcidMsgIpc {
        data: frame,
        code: CcidCode::Tx,
    };
    let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
    buf.lend_mut(conn, Opcode::CcidTx.to_u32().unwrap())
        .or(Err(xous::Error::InternalError))?;
    match buf.to_original::<CcidMsgIpc, _>().unwrap().code {
        CcidCode::TxAck => Ok(()),
        CcidCode::Denied => Err(xous::Error::AccessDenied),
        CcidCode::Hangup => Err(xous::Error::ProcessTerminated),
        _ => Err(xous::Error::InternalError),
    }
}

/// Parse host frame and build a CCID response (replace bodies with real logic).
fn handle_ccid_request(host_frame: &[u8]) -> Option<Vec<u8>> {
    if host_frame.len() < 10 {
        return None;
    }
    let msg_type = host_frame[0];
    let slot = host_frame[5];
    let seq = host_frame[6];
    match msg_type {
        // 0x65 GetSlotStatus and 0x62 IccPowerOn are answered inline in usb-bao1x
        // and never reach this handler. Keep the XfrBlock path only.
        0x6F => {
            // PC_to_RDR_XfrBlock -> RDR_to_PC_DataBlock
            let dw_len = u32::from_le_bytes([
                host_frame[1],
                host_frame[2],
                host_frame[3],
                host_frame[4],
            ]) as usize;
            if host_frame.len() < 10 + dw_len {
                return None;
            }
            let payload = &host_frame[10..10 + dw_len];
            let apdu_response = handle_apdu(payload);
            let mut out = Vec::with_capacity(10 + apdu_response.len());
            out.push(0x80);
            out.extend_from_slice(&(apdu_response.len() as u32).to_le_bytes());
            out.push(slot);
            out.push(seq);
            out.extend_from_slice(&[0, 0, 0]); // bStatus, bError, RFU
            out.extend_from_slice(&apdu_response);
            Some(out)
        }
        _ => None,
    }
}

fn handle_apdu(_apdu_payload: &[u8]) -> Vec<u8> {
    // TODO: T=1 unwrap (if needed), OpenPGP command dispatch, crypto, etc.
    vec![0x90, 0x00]
}

fn main() -> ! {
    log_server::init_log("ccid-openpgp-handler").expect("log");
    let usb = connect_usb();
    log::info!("CCID handler connected to {}", USB_SERVER);

    loop {
        match ccid_wait_frame(usb) {
            Ok(host_frame) => {
                log::debug!("CCID RX {} bytes, type 0x{:02x}", host_frame.len(), host_frame[0]);
                if let Some(reply) = handle_ccid_request(&host_frame) {
                    if ccid_send_frame(usb, reply).is_err() {
                        log::warn!("CCID TX failed (USB not configured?)");
                    }
                }
            }
            Err(xous::Error::ProcessTerminated) => break,
            Err(e) => log::warn!("CCID wait error: {:?}", e),
        }
    }
    panic!("CCID handler exited");
}
```

**Integration checklist:**

1. Build `baosec` with `ccid-openpgp` only (do **not** enable `ccid-echo` in production).
2. Add the handler service to the Xous process table / `xous.toml` so it starts at boot.
3. Ensure only this service calls `CcidRxDeferred` (second listeners get `Denied`).
4. Read provisioning PIN lines from PDDB dict `usb.ccid` if the OpenPGP layer needs them.
5. Validate against `ccid-echo` HIL first, then against `pcscd` / GnuPG on Linux.

---

## PIN provisioning (offline / non-USB on CCID images)

See [Provisioning trust model](#provisioning-trust-model-persona-a).

Before OpenPGP operation, the device may need two opaque **PIN lines**
(user and admin) in PDDB. xous-core only **stores/reads** them; it does not
validate format, derive keys, or interpret content.

### Boot behavior (`ccid-openpgp`)

`usb-bao1x` does **not** open PDDB at boot. An earlier `Pddb::new()` call before
`cu.init()` blocked USB bring-up on first-boot format. USB composite is always
CCID+FIDO+NKRO on CCID images (no provision CDC).

`ccid_store` compiles only with feature `ccid-pddb`. No current `cargo xtask`
image enables that feature; factory tools that seed PDDB do so offline.

### PDDB keys

| PDDB key | Content |
|----------|---------|
| `usb.ccid` / `user_pin_line` | First line (opaque bytes) |
| `usb.ccid` / `admin_pin_line` | Second line (opaque bytes) |
| `usb.ccid` / `provisioned` | Marker `OKV1` |

Maximum key size is 256 bytes per PIN line in `ccid_store.rs`.
`save_provisioned_pins` can seed these offline; nothing in the USB IRQ/IPC path
calls it on Persona A images.

### HIL follow-up

`tools/ccid_hil/test_provision.py` (HIL-02) was **rewritten for Persona A**:
it asserts the device presents **no CDC ACM** interfaces (and the shared
Persona A composite checks). It does **not** send PIN lines over USB.
`--legacy-usb-provision` exits 2. `run_all.sh` always runs HIL-02.
`CCID_HIL_PROVISION` is ignored. There is no USB PIN path and no boot-time
OKV1 UART log; HIL-02 only proves CDC absence.

---

## HIL test personality (`ccid-echo`)

See [Production vs HIL: `ccid-echo` security boundary](#production-vs-hil-ccid-echo-security-boundary).
**Do not enable in production images.**

For transport testing **without** an OpenPGP handler, build with `ccid-echo`
(included in `cargo xtask ccid-hil`):

```
Host bulk OUT  --->  device assembles frame  --->  bulk IN echoes same bytes
```

This validates USB descriptors, endpoint pairing, multi-packet reassembly, and
TX chunking. It does **not** validate APDU semantics or crypto.

---

## Host software path (pcscd / GnuPG)

Expected Linux path (confirmed on Dabao with `dabao-ccid` + out-of-tree stub):

1. Plug in Dabao (`1d50:6197`); kernel binds the CCID interface.
2. `pcscd` runs `RFAddReader` successfully (CreateChannel GetSlotStatus probes
   answered inline; bulk OUT primed after address set).
3. `pcsc_scan` shows reader `Baochip Dabao CCID (HBZFHW)`, card inserted, ATR
   `3B DA 18 FF 81 B1 FE 75 1F 03 00 31 C5 73 C0 01 40 00 90 00 0C`, identified
   as **OpenPGP Card V2**.
4. Full `gpg --card-status` / production OpenPGP crypto still needs the real
   handler (stub proves ATR + SELECT only).

This repository's automated CI still covers:

- Framing logic (unit tests — **9/9** `ccid_framing`)
- USB enumeration and bulk echo (`ccid-echo` image + Python HIL)

Hardware pcscd / `pcsc_scan` results are recorded in
[`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md). Full GnuPG regression belongs in
the OpenPGP handler repository.

---

## Testing guide

How to run every tier: developer machine, Linux USB host, and CI.

### Prerequisites by test type

| Test | Device image | Host | Required tools |
|------|--------------|------|----------------|
| Unit tests (`ccid_framing`, `ep_budget`) | none | Linux/macOS + Rust | `cargo` |
| EP arithmetic / mock Persona A | none | Python 3 | `check_ep_budget.py`, `test_ep_budget_cumulative.py`, `sim_persona_a_composite.py` |
| Compile gates | none | Linux (CI: Ubuntu) | `cargo xtask install-toolkit` for board target |
| Smoke test | `ccid-hil` (`ccid-echo`) | Linux USB host | `pyusb` |
| HIL suite | `ccid-hil` | Linux USB host | `pyusb`, `lsusb` (no `pyserial`) |
| Production CCID | `baosec-ccid` + handler | Linux + handler | OpenPGP service (out of tree) |

Build the HIL test image:

```bash
cargo xtask ccid-hil
```

Flash the image, connect the device **data** USB port to the host, then run
tests below.

### Tier 1: Unit tests (no hardware)

```bash
cargo test -p usb-bao1x --lib ccid_framing
cargo test -p usb-bao1x --lib ep_budget
python3 tools/check_ep_budget.py
python3 tools/test_ep_budget_cumulative.py
python3 tools/sim_persona_a_composite.py
```

- `ccid_framing`: partial-frame handling, oversize rejection, reassembly, TX chunking, `OKV1` marker, GetSlotStatus / IccPowerOn helpers (**9/9**).
- `ep_budget`: cumulative ledger + proof that independent subtotals miss 7+2 overflow.
- Python scripts: static EP totals per xtask image; mock Persona A composite asserts.
### Tier 2: Compile gates (no hardware)

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp

cargo xtask install-toolkit --force --no-verify
cargo check -p usb-bao1x -p modals --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf

cargo xtask baosec-ccid --no-verify
cargo xtask ccid-hil --no-verify
```

Automated on push/PR via `.github/workflows/ccid-ci.yml` and the `baosec`
matrix in `.github/workflows/build.yml`.

**Fork note:** image-signing steps need annotated git tags. Fork CI workflows
fetch release tags from `betrusted-io/xous-core` before building so
`SemVer::from_git()` succeeds during swap signing.

### Tier 3: USB smoke test (~30 seconds)

Requires `ccid-hil` image flashed.

```bash
pip install pyusb
lsusb -d 1d50:6198
python3 tools/ccid_smoke.py
```

**Pass criteria:**

- `Device enumerated.`
- CCID descriptor: `bcd=0110`, `protocols=0x00000002`
- `GetSlotStatus echo OK.`
- `XfrBlock echo OK.`
- Final line: `PASS`

Flags:

```bash
python3 tools/ccid_smoke.py --timeout 120
python3 tools/ccid_smoke.py --skip-echo
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197   # dabao
```

### Tier 4: Full HIL suite (~2 minutes)

```bash
pip install pyusb
chmod +x tools/ccid_hil/*.sh
tools/ccid_hil/run_all.sh
```

| Step | Script | Checks | Pass line | Why |
|------|--------|--------|-----------|-----|
| 00 | `wait_device.sh` | Device `1d50:6198` present | `Device … present` | Gate before USB I/O |
| 01 | `test_enumerate.py` | CCID fields + Persona A composite | `HIL-01 PASS` | Wrong image / CDC or EP drift |
| 02 | `test_provision.py` | **Zero CDC**; no USB PIN path | `HIL-02 PASS (Persona A)` | Persona A surface; CDC return = fail |
| 03 | `test_echo.py` | GetSlotStatus echo | `HIL-03 PASS` | Transport without OpenPGP handler |
| 04 | `test_echo.py --stress N` | Random XfrBlock | `HIL-05 PASS` | Stress bulk / reassembly |

Environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor (hex, no `0x` prefix) |
| `CCID_PID` | `6198` | USB product ID |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | **Obsolete** — ignored; HIL-02 always runs CDC-absence check |
| `CCID_HIL_STRESS` | `100` | Random echo iterations |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

OPEN: suite does not capture UART (debug on CCID images is UART-only).
### Tier 5: CI (automated)

| Workflow | Runner | Scope |
|----------|--------|-------|
| `ccid-ci.yml` | GitHub-hosted | `ccid_framing` + `ep_budget` + `baosec-ccid` / `ccid-hil` compile |
| `build.yml` | GitHub-hosted | Full `baosec` image (stock, **no** CCID) |
| `ccid-hil.yml` | Self-hosted (`baosec-hil`) | USB HIL on Raspberry Pi |

Trigger Pi workflow manually: Actions -> CCID HIL -> Run workflow.

### Interpreting failures

| Failure | Likely cause |
|---------|----------------|
| `Timeout waiting for CCID device` | Cable, power, wrong image, check `lsusb` |
| `CCID interface not found` | Image missing `ccid-openpgp`; rebuild `ccid-hil` |
| `echo mismatch` | Missing `ccid-echo`; host sent before configured |
| `pyusb` permission error | udev rules / group membership |
| `Can't sign swap image` (CI on fork) | Missing upstream git tags; see fork note above |
| HIL-02 FAIL: CDC present | Persona A violated — debug/provision CDC must not be on CCID images |
| HIL-02 PASS but no UART OKV1 check | Expected — no boot PDDB log; harness has no UART capture |

---

## Raspberry Pi HIL setup

Self-contained Linux USB host for nightly transport regression (as suggested in
[PR #890 review](https://github.com/betrusted-io/xous-core/pull/890#issuecomment-4916206948)).

### Hardware

```
  +----------------+          USB-A
  | Raspberry Pi 4 |----------------------> baosec (data port)
  | (or Pi 5)      |
  +----------------+
        |
        optional: UART to device DUART for firmware logs
```

Use a **powered** hub if the Pi port is marginal; bulk tests are sensitive to
voltage droop.

### Operating system

1. Flash Raspberry Pi OS Lite (64-bit) or Ubuntu Server for Pi.
2. Enable SSH; hostname e.g. `baosec-hil`.
3. Install packages:

```bash
sudo apt update
sudo apt install -y git python3-pip python3-venv usbutils \
  libusb-1.0-0-dev build-essential pkg-config libxkbcommon-dev
```

### USB permissions (udev)

`/etc/udev/rules.d/99-baosec-ccid.rules`:

```
SUBSYSTEM=="usb", ATTR{idVendor}=="1d50", ATTR{idProduct}=="6198", MODE="0666", GROUP="plugdev"
SUBSYSTEM=="tty", ATTRS{idVendor}=="1d50", ATTRS{idProduct}=="6198", MODE="0666", GROUP="dialout"
```

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
sudo usermod -aG plugdev,dialout $USER
# log out and back in
```

### Toolchain and tests

```bash
git clone https://github.com/betrusted-io/xous-core.git
cd xous-core
cargo xtask install-toolkit --force --no-verify
cargo xtask ccid-hil    # build HIL image; flash separately

python3 -m venv ~/ccid-venv
source ~/ccid-venv/bin/activate
pip install pyusb

python3 tools/ccid_smoke.py
tools/ccid_hil/run_all.sh
```

Logs: `/tmp/ccid-hil-out/`

### GitHub Actions self-hosted runner

1. Register runner on `betrusted-io/xous-core` with labels `self-hosted`,
   `baosec-hil`.
2. Runner user in `plugdev` and `dialout`.
3. Flash `ccid-hil` image once on the bench (workflow does not flash today).
4. Nightly `ccid-hil.yml` runs `tools/ccid_hil/run_all.sh`.

---

## CI summary

| Tier | Where | What |
|------|-------|------|
| Unit tests | GitHub-hosted | `ccid_framing` + `ep_budget` (`ccid-ci.yml`) |
| EP arithmetic (optional local) | Developer machine | `check_ep_budget.py`, `test_ep_budget_cumulative.py`, `sim_persona_a_composite.py` |
| Compile + image | GitHub-hosted | `ccid-ci.yml` (`baosec-ccid` / `ccid-hil`); `build.yml` stock `baosec` (no CCID) |
| HIL transport | Pi self-hosted | Enum, no-CDC (HIL-02), echo, stress |
| OpenPGP E2E | Out of tree + Dabao HIL | Stub: `pcsc_scan` ATR / OpenPGP Card V2; full GnuPG = handler repo |

See also [`docs/CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md) for recorded
verification results and [`docs/code_map.md`](code_map.md) for source navigation.
