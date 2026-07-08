<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID HIL testing

Hardware-in-the-loop tests for the `usb-bao1x` CCID transport (`ccid-openpgp`
feature).

**Protocol reference and Raspberry Pi setup:** see
[`docs/CCID_PROTOCOL_AND_HIL.md`](../../docs/CCID_PROTOCOL_AND_HIL.md) (includes
full testing guide).

## Quick test commands

```bash
# Unit tests (no hardware)
cargo test -p usb-bao1x --lib ccid_framing

# USB smoke test (ccid-hil image required)
python3 tools/ccid_smoke.py

# Full HIL suite
tools/ccid_hil/run_all.sh
```

## Requirements

- Linux USB host (desktop or Raspberry Pi)
- baosec device with image built using `cargo xtask ccid-hil`
- Python 3 packages: `pyusb`, `pyserial`
- `lsusb` (usbutils)

```bash
pip install pyusb pyserial
```

On Linux you may need udev rules so the test user can access the device without
root (see the main doc).

## Quick smoke test (manual)

Flash a `ccid-hil` image, connect USB, then:

```bash
python3 tools/ccid_smoke.py
```

## Full HIL suite

```bash
chmod +x tools/ccid_hil/*.sh
CCID_HIL_PROVISION=1 tools/ccid_hil/run_all.sh   # optional provisioning test
tools/ccid_hil/run_all.sh                         # enumeration + echo + stress
```

Environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `CCID_VID` | `1d50` | USB vendor ID (hex, no prefix) |
| `CCID_PID` | `6198` | USB product ID (baosec) |
| `CCID_WAIT_TIMEOUT` | `60` | Seconds to wait for device |
| `CCID_HIL_PROVISION` | `0` | Set to `1` to run provisioning CDC test |
| `CCID_HIL_STRESS` | `100` | Random XfrBlock echo iterations |
| `CCID_HIL_OUT` | `/tmp/ccid-hil-out` | Log directory |

## CI

- `.github/workflows/ccid-ci.yml` — unit tests + compile gates (GitHub-hosted)
- `.github/workflows/ccid-hil.yml` — nightly HIL on self-hosted runner label `baosec-hil`

Build the HIL firmware image:

```bash
cargo xtask ccid-hil
```

The `ccid-echo` feature echoes `PC_to_RDR` frames on bulk IN so transport can be
tested without an OpenPGP handler service.
