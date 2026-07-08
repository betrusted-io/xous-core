# CCID protocol and Raspberry Pi HIL setup

This document describes the USB CCID transport implemented in `usb-bao1x`
(`ccid-openpgp` feature), how it relates to host-side tools such as `pcscd` /
GnuPG, and how to configure a Raspberry Pi as a hardware-in-the-loop (HIL) test
host.

OpenPGP / smart-card cryptography is intentionally **not** implemented in
xous-core. This repository provides USB framing and first-boot provisioning only;
an external Xous service is expected to handle APDUs via IPC.

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
| Unit tests | GitHub-hosted (`ccid-ci.yml`) | `ccid_framing` tests, compile gates |
| HIL transport | Pi self-hosted (`ccid-hil.yml`) | Enumeration, echo, stress |
| OpenPGP E2E | Out of tree | `gpg --card-status` once handler service exists |

## Related files

| Path | Purpose |
|------|---------|
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class driver |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire format helpers + unit tests |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning storage |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts and suite |
| `.github/workflows/ccid-ci.yml` | CI compile + unit tests |
| `.github/workflows/ccid-hil.yml` | Nightly Pi HIL |
