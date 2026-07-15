<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID HIL testing

Hardware-in-the-loop (HIL) tests for the `usb-bao1x` CCID smart-card
**transport** (`ccid-openpgp` feature). These tests confirm that the device
enumerates as a USB CCID reader and correctly moves `PC_to_RDR` / `RDR_to_PC`
frames on bulk endpoints. They do **not** test OpenPGP cryptography or APDU
semantics.

## Documentation

| Document | Contents |
|----------|----------|
| [`docs/CCID_PROTOCOL_AND_HIL.md`](../../docs/CCID_PROTOCOL_AND_HIL.md) | **Main reference** — smart-card/CCID background, architecture, IPC handler guide, security considerations, Pi setup, testing guide |
| [`docs/code_map.md`](../../docs/code_map.md) | **Code map** — symptom-to-source navigation for debugging and fixes |
| [`docs/CCID_TEST_REPORT.md`](../../docs/CCID_TEST_REPORT.md) | Recorded verification results and CI status |

## Quick start

```bash
# 1. Build and flash HIL image (ccid-openpgp + ccid-echo)
cargo xtask ccid-hil

# 2. Cable device USB data port to Linux host

# 3. Unit tests (no hardware)
cargo test -p usb-bao1x --lib ccid_framing

# 4. Smoke test (~30 s)
pip install pyusb
python3 tools/ccid_smoke.py

# 5. Full suite (~2 min)
pip install pyusb pyserial
chmod +x tools/ccid_hil/*.sh
tools/ccid_hil/run_all.sh
```

## Requirements

- Linux USB host (desktop or Raspberry Pi)
- baosec flashed with `cargo xtask ccid-hil` image
- Python 3: `pyusb`, `pyserial`
- `lsusb` (usbutils package)
- udev rules so the test user can access USB without root (see main doc)

## Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor ID (hex, no prefix) |
| `CCID_PID` | `6198` | USB product ID (baosec) |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | Set `1` to run provisioning CDC test |
| `CCID_HIL_STRESS` | `100` | Random XfrBlock echo iterations |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

## Suite steps

| Script | Pass marker |
|--------|-------------|
| `wait_device.sh` | `Device 1d50:6198 present` |
| `test_enumerate.py` | `HIL-01 PASS` |
| `test_provision.py` | `HIL-02 PASS` (optional) |
| `test_echo.py` | `HIL-03 PASS` |
| `test_echo.py --stress N` | `HIL-05 PASS` |

Provisioning test requires an **unprovisioned** device (PDDB `usb.ccid` /
`provisioned` not `OKV1`):

```bash
CCID_HIL_PROVISION=1 tools/ccid_hil/run_all.sh
```

## CI

- `.github/workflows/ccid-ci.yml` — unit tests + compile gates (GitHub-hosted)
- `.github/workflows/ccid-hil.yml` — USB HIL on self-hosted runner (`baosec-hil`)

The `ccid-echo` feature echoes host frames on bulk IN so transport can be tested
without an OpenPGP handler service. **HIL images only — never ship `ccid-echo`
in production** (see [security boundary](../../docs/CCID_PROTOCOL_AND_HIL.md#production-vs-hil-ccid-echo-security-boundary) in the main doc).
