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
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp \
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

See [Testing guide](#testing-guide) above for step-by-step smoke and HIL
commands. Quick reference:

```bash
source ~/ccid-venv/bin/activate
python3 tools/ccid_smoke.py
tools/ccid_hil/run_all.sh
```

Logs: `/tmp/ccid-hil-out/`

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

See [Testing guide](#testing-guide) for commands. Overview:

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
