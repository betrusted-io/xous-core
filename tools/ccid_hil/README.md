<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID HIL testing

Hardware-in-the-loop (HIL) tests for the `usb-bao1x` CCID smart-card
**transport** (`ccid-openpgp` feature). These tests confirm that the device
enumerates as a USB CCID reader and correctly moves `PC_to_RDR` / `RDR_to_PC`
frames on bulk endpoints. They do **not** test OpenPGP cryptography or APDU
semantics.

**Persona A:** CCID images are **CCID + FIDO + NKRO only** (7 of 8 Corigine
endpoint slots). There is **no** debug CDC and **no** provisioning CDC. Device
logs go to **UART / xous-log**, not USB serial. This harness does not capture
UART yet.

## Documentation

| Document | Contents |
|----------|----------|
| [`docs/CCID_PROTOCOL_AND_HIL.md`](../../docs/CCID_PROTOCOL_AND_HIL.md) | **Main reference** — smart-card/CCID background, architecture, IPC handler guide, security considerations, Pi setup, testing guide |
| [`docs/code_map.md`](../../docs/code_map.md) | **Code map** — symptom-to-source navigation for debugging and fixes |
| [`docs/CCID_TEST_REPORT.md`](../../docs/CCID_TEST_REPORT.md) | Recorded verification results and CI status |
| [`CCID_EP_BUDGET_AND_HIL_LOCAL.md`](../../CCID_EP_BUDGET_AND_HIL_LOCAL.md) | Local working EP-budget / HIL notes (uncommitted convention) |

## Quick start

```bash
# 0. Local EP budget arithmetic (no hardware)
python3 tools/check_ep_budget.py
python3 tools/sim_persona_a_composite.py

# 1. Build and flash HIL image (ccid-openpgp + ccid-echo)
cargo xtask ccid-hil

# 2. Cable device USB data port to Linux host
#    (optional separate: watch UART/DUART for firmware logs)

# 3. Unit tests (no hardware)
cargo test -p usb-bao1x --lib ccid_framing

# 4. Smoke test (~30 s)
pip install pyusb
python3 tools/ccid_smoke.py

# 5. Full suite (~2 min)
pip install pyusb
chmod +x tools/ccid_hil/*.sh
tools/ccid_hil/run_all.sh
```

## Requirements

- Linux USB host (desktop or Raspberry Pi)
- baosec flashed with `cargo xtask ccid-hil` image
- Python 3: `pyusb` (pyserial **not** required for HIL-02 under Persona A)
- `lsusb` (usbutils package)
- udev rules so the test user can access USB without root (see main doc)

## Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor ID (hex, no prefix) |
| `CCID_PID` | `6198` | USB product ID (baosec) |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | **Obsolete** under Persona A (ignored; HIL-02 always runs CDC-absence check) |
| `CCID_HIL_STRESS` | `100` | Random XfrBlock echo iterations |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

## Suite steps

| Script | Pass marker | Why |
|--------|-------------|-----|
| `wait_device.sh` | `Device 1d50:6198 present` | Gate before USB I/O |
| `test_enumerate.py` | `HIL-01 PASS` (+ Persona A composite) | Wrong image / EP or CDC drift |
| `test_provision.py` | `HIL-02 PASS (Persona A)` — **no CDC** | USB PIN path must stay gone |
| `test_echo.py` | `HIL-03 PASS` | Transport without OpenPGP |
| `test_echo.py --stress N` | `HIL-05 PASS` | Stress bulk / reassembly |

PIN provisioning over USB is **not** tested here. Seed PDDB offline / before
flash if product needs `OKV1`.

## CI

- `.github/workflows/ccid-ci.yml` — `ccid_framing` + `ep_budget` unit tests + compile gates
- `.github/workflows/ccid-hil.yml` — USB HIL on self-hosted runner (`baosec-hil`; no pyserial)

The `ccid-echo` feature echoes host frames on bulk IN so transport can be tested
without an OpenPGP handler service. **HIL images only — never ship `ccid-echo`
in production** (see [security boundary](../../docs/CCID_PROTOCOL_AND_HIL.md#production-vs-hil-ccid-echo-security-boundary) in the main doc).
