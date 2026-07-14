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

## Table of contents

1. [Background: smart cards, CCID, and OpenPGP](#background-smart-cards-ccid-and-openpgp)
2. [Scope: what xous-core does and does not do](#scope-what-xous-core-does-and-does-not-do)
3. [Architecture overview](#architecture-overview)
4. [USB composite device](#usb-composite-device)
5. [Feature flags and firmware images](#feature-flags-and-firmware-images)
6. [CCID USB interface](#ccid-usb-interface)
7. [Message framing on the wire](#message-framing-on-the-wire)
   - [Example CCID hex dumps](#example-ccid-hex-dumps)
8. [APDUs, T=1, and XfrBlock](#apdus-t1-and-xfrblock)
9. [Xous IPC API and handler integration](#xous-ipc-api-and-handler-integration)
   - [Handler skeleton (Rust)](#handler-skeleton-rust)
10. [First-boot provisioning (CDC serial)](#first-boot-provisioning-cdc-serial)
11. [HIL test personality (`ccid-echo`)](#hil-test-personality-ccid-echo)
12. [Host software path (pcscd / GnuPG)](#host-software-path-pcscd--gnupg)
13. [Testing guide](#testing-guide)
14. [Raspberry Pi HIL setup](#raspberry-pi-hil-setup)
15. [CI summary](#ci-summary)
16. [Related files](#related-files)

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

1. Host sends CCID bulk OUT data; `ccid_transport` reassembles a complete
   `PC_to_RDR` frame.
2. Frame is queued; `IrqCcidRx` notifies `usb-bao1x`.
3. A deferred listener (external handler) receives the raw frame bytes via
   `CcidRxDeferred`.
4. Handler parses the CCID message, runs APDU logic, builds an `RDR_to_PC`
   response frame.
5. Handler sends raw response bytes with `CcidTx`; transport chunks them on
   bulk IN.

---

## USB composite device

Baosec already exposes a composite USB gadget (HID keyboard, FIDO, debug
serial, etc.). With `ccid-openpgp` enabled, two more interfaces may appear:

| Interface | When present | Purpose |
|-----------|--------------|---------|
| CCID bulk (`0x0B`) | Always (feature enabled) | Smart-card transport |
| Provisioning CDC ACM | **Only until provisioned** | First-boot two-line PIN capture |

After provisioning succeeds (`usb.ccid` / `provisioned` = `OKV1`), the device
resets USB and re-enumerates **without** the provisioning serial port. The CCID
interface remains.

### USB identification

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
# Production baosec image (transport + provisioning; handler must be added separately)
cargo xtask baosec

# HIL test image (adds ccid-echo; no external handler needed for USB tests)
cargo xtask ccid-hil
```

Compile-only checks without flashing:

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
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
| Wire maximum | 530 bytes | 10-byte header + payload (`CCID_WIRE_MAX`) |

Endpoints:

- **Bulk OUT** — host sends `PC_to_RDR_*` frames (possibly split across 512-byte
  USB transactions)
- **Bulk IN** — device sends `RDR_to_PC_*` frames (chunked at 512 bytes)
- **Interrupt IN** — present in descriptor; notifications not implemented

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

Total size = 10 + `dwLength`, capped at 530 bytes.

The device **buffers partial bulk OUT packets** until a full frame is available,
then delivers the complete byte vector to software. Replies are queued as one
contiguous `RDR_to_PC` buffer and streamed on bulk IN.

### Common message types

Used in HIL tests (`tools/ccid_hil/ccid_usb.py`) and typical OpenPGP reader
traffic:

| Value | Name | Direction | Role |
|-------|------|-----------|------|
| 0x65 | PC_to_RDR_GetSlotStatus | Host to reader | Poll slot state |
| 0x6F | PC_to_RDR_XfrBlock | Host to reader | Carries T=1 / APDU payload |
| 0x80 | RDR_to_PC_DataBlock | Reader to host | Response data (often APDU response) |
| 0x81 | RDR_to_PC_SlotStatus | Reader to host | Slot status reply |

xous-core **does not interpret** these types. It forwards complete frames over
IPC. Only the external handler (or the `ccid-echo` test personality) inspects
`bMessageType`.

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

On a `ccid-echo` HIL image, bulk IN returns the **same 10 bytes** unchanged.

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

Replies are streamed in 512-byte chunks. A 530-byte response (maximum wire
size) uses two IN transactions:

```
Chunk 1: 512 bytes (bytes 0..511 of the RDR_to_PC frame)
Chunk 2:  18 bytes (bytes 512..529)
```

Capture with `usbmon` or pyusb read loops if debugging partial reads on the host.

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
| `IrqProvSerialRx` | 771 | Provisioning CDC line ready |

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

Provisioning lines are **not** delivered over CCID. They arrive on the separate
CDC serial interface during first boot (see below). The OpenPGP handler reads
stored lines from PDDB if needed.

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
        0x65 => {
            // PC_to_RDR_GetSlotStatus -> RDR_to_PC_SlotStatus (see hex dumps above)
            Some(vec![0x81, 0, 0, 0, 0, slot, seq, 0, 0, 0])
        }
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

## First-boot provisioning (CDC serial)

Before OpenPGP operation, the device may need two opaque **PIN lines**
(user and admin). xous-core only **stores** them; it does not validate format,
derive keys, or interpret content.

### When it runs

At boot, `usb-bao1x` checks PDDB dict `usb.ccid` / key `provisioned`. If the
value is not `OKV1`, a **second CDC ACM serial port** is exposed alongside CCID.

### Protocol

1. Host (or factory tool) opens the provisioning serial port.
2. Send **first line** + newline (`\r` or `\n`) — user PIN line (opaque).
3. Send **second line** + newline — admin PIN line (opaque).
4. Device writes PDDB, logs success, and **resets USB** (PMIC unplug on baosec).

After re-enumeration, the provisioning port is gone; CCID remains.

### PDDB keys

| PDDB key | Content |
|----------|---------|
| `usb.ccid` / `user_pin_line` | First line (opaque bytes) |
| `usb.ccid` / `admin_pin_line` | Second line (opaque bytes) |
| `usb.ccid` / `provisioned` | Marker `OKV1` |

Printable bytes (>= 0x20) are accepted on the wire. Maximum line length is
governed by PDDB key size (256 bytes per key in `ccid_store.rs`).

### Factory reset

To re-run provisioning HIL tests, the device must be factory-reset or flashed
fresh so `provisioned` is not `OKV1`.

---

## HIL test personality (`ccid-echo`)

For transport testing **without** an OpenPGP handler, build with `ccid-echo`
(included in `cargo xtask ccid-hil`):

```
Host bulk OUT  --->  device assembles frame  --->  bulk IN echoes same bytes
```

This validates USB descriptors, endpoint pairing, multi-packet reassembly, and
TX chunking. It does **not** validate APDU semantics or crypto.

---

## Host software path (pcscd / GnuPG)

Once an OpenPGP handler service ships on the device, the expected Linux path is:

1. Plug in Baosec; kernel loads generic CCID support or a project-specific udev
   rule binds the interface.
2. `pcscd` detects the reader (`pcsc_scan`).
3. `gpg --card-status` issues CCID exchanges that become APDUs on the device.

That end-to-end path is **out of scope** for xous-core CI today. This repository
tests:

- Framing logic (unit tests)
- USB enumeration and bulk echo (`ccid-echo` image + Python HIL)

Full `gpg --card-status` regression belongs in the OpenPGP handler repository
once it exists.

---

## Testing guide

How to run every tier: developer machine, Linux USB host, and CI.

### Prerequisites by test type

| Test | Device image | Host | Required tools |
|------|--------------|------|----------------|
| Unit tests | none | Linux/macOS + Rust | `cargo` |
| Compile gates | none | Linux (CI: Ubuntu) | `cargo xtask install-toolkit` for board target |
| Smoke test | `ccid-hil` (`ccid-echo`) | Linux USB host | `pyusb` |
| HIL suite | `ccid-hil` | Linux USB host | `pyusb`, `pyserial`, `lsusb` |
| Provisioning HIL | unprovisioned device | Linux USB host | `pyserial` + above |
| Production CCID | `baosec` + `ccid-openpgp` | Linux + handler | OpenPGP service (out of tree) |

Build the HIL test image:

```bash
cargo xtask ccid-hil
```

Flash the image, connect the device **data** USB port to the host, then run
tests below.

### Tier 1: Unit tests (no hardware)

```bash
cargo test -p usb-bao1x --lib ccid_framing
```

Expected: **7 passed**. Covers partial-frame handling, oversize rejection,
multi-packet reassembly, TX chunking, and PDDB marker `OKV1`.

### Tier 2: Compile gates (no hardware)

```bash
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp

cargo xtask install-toolkit --force --no-verify
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf

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
pip install pyusb pyserial
chmod +x tools/ccid_hil/*.sh
tools/ccid_hil/run_all.sh
```

| Step | Script | Checks | Pass line |
|------|--------|--------|-----------|
| 00 | `wait_device.sh` | Device `1d50:6198` present | `Device 1d50:6198 present` |
| 01 | `test_enumerate.py` | CCID class 0x0B, descriptor fields | `HIL-01 PASS` |
| 02 | `test_provision.py` | Optional (`CCID_HIL_PROVISION=1`) | `HIL-02 PASS` |
| 03 | `test_echo.py` | GetSlotStatus round-trip | `HIL-03 PASS` |
| 04 | `test_echo.py --stress N` | Random XfrBlock frames | `HIL-05 PASS` |

Environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor (hex, no `0x` prefix) |
| `CCID_PID` | `6198` | USB product ID |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | Set `1` to run provisioning test |
| `CCID_HIL_STRESS` | `100` | Random echo iterations |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

Provisioning test (unprovisioned device only):

```bash
CCID_HIL_PROVISION=1 tools/ccid_hil/run_all.sh
python3 tools/ccid_hil/test_provision.py --port /dev/ttyACM1
```

### Tier 5: CI (automated)

| Workflow | Runner | Scope |
|----------|--------|-------|
| `ccid-ci.yml` | GitHub-hosted | Unit tests + compile gates |
| `build.yml` | GitHub-hosted | Full `baosec` image build (includes CCID) |
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
| Provisioning: no re-enumeration | Wrong serial port; device already provisioned |

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
pip install pyusb pyserial

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
| Unit tests | GitHub-hosted | `ccid_framing` (7 tests) |
| Compile + image | GitHub-hosted | `ccid-ci.yml`, `build.yml` / `baosec` |
| HIL transport | Pi self-hosted | Enumeration, echo, stress |
| OpenPGP E2E | Out of tree | `gpg --card-status` with handler service |

See also [`docs/CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md) for recorded
verification results.

---

## Related files

| Path | Purpose |
|------|---------|
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class driver |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire format helpers + unit tests |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning storage |
| `services/usb-bao1x/src/api.rs` | IPC opcodes and `CcidMsgIpc` |
| `services/usb-bao1x/src/main.rs` | Deferred listener, echo, provisioning |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts |
| `.github/workflows/ccid-ci.yml` | CI compile + unit tests |
| `.github/workflows/ccid-hil.yml` | Nightly Pi HIL |
