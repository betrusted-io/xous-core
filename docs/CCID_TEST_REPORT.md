<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID smart-card transport verification report

This report records verification status for the `usb-bao1x` CCID transport
(`ccid-openpgp` feature) on branch `feature/usb-bao1x-ccid-openpgp`
([PR #890](https://github.com/betrusted-io/xous-core/pull/890)).

For protocol background, handler integration, Pi HIL setup, **security
considerations**, and source navigation, see [`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md)
and [`code_map.md`](code_map.md). Enumeration deep-dive (community):
[`CCID_USB_ENUMERATION_DEBUG.md`](CCID_USB_ENUMERATION_DEBUG.md).

## Hardware

| Item | Value |
|------|-------|
| Board | **Dabao** (Baochip-1x), developer mode |
| Boot1 | v0.10.0-61 |
| Build | `cargo xtask dabao-ccid --no-verify` |
| Features in binary | `board-dabao`, `ccid-openpgp` |

Also buildable as `cargo xtask dabao --feature ccid-openpgp --no-verify`.

## USB enumeration — confirmed on hardware

| Field | Observed |
|-------|----------|
| Product | `1d50:6197` Dabao |
| Speed | High-speed (480 Mbps) |
| IF0 | `bInterfaceClass 11` Chip/SmartCard (CCID) |
| IF1 | `bInterfaceClass 3` HID (generic / U2F) |
| IF2 | `bInterfaceClass 3` HID (Boot Keyboard) |
| CCID bulk `wMaxPacketSize` | `0x0200` (512 bytes) on EP 0x01 / 0x81 |
| `dwMaxCCIDMessageLength` | 271 (`0x10F`) — short APDU max |
| `dwFeatures` | `0x000400FE` (short APDU level exchange) |
| Stability | 5+ minutes connected; no disconnects; no `error -71` on enumeration |

Persona A composite: **no** CDC-ACM on dabao-ccid (no `ttyACM` after boot).

## Checks passing

| Check | Result |
|-------|--------|
| `cargo test -p usb-bao1x --lib ccid_framing` | **7/7** |
| `cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp` | Pass (hosted feature wiring activates `ux-api/hosted-baosec` → `blitstr2`) |
| `cargo xtask dabao-ccid --no-verify` | Succeeds |
| Host enumeration | No `invalid maxpacket 64` warning; no `error -71` |

Related unit / static gates (CI / local):

| Area | Method | Status |
|------|--------|--------|
| Cumulative EP budget (ledger) | `cargo test -p usb-bao1x --lib ep_budget` | Pass (CI): **4** tests |
| EP budget arithmetic | `python3 tools/check_ep_budget.py` | Pass (local) |
| Persona A layout mock | `python3 tools/sim_persona_a_composite.py` | Pass (local) |

## Known limitations / not yet tested

- GnuPG `scdaemon` APDU exchange (requires Galdralag / out-of-tree APDU handler)
- Provisioning flow (CDC-ACM provisioning is not present on dabao-ccid; Persona A)
- **Baosec** board target (this HIL used Dabao; baosec path not hardware-tested here)
- CCID interrupt insert/remove notifications
- Automated USB HIL in GitHub Actions (no self-hosted runner registered)

## Build notes

- `ccid-openpgp` does **not** depend on `pddb` (dabao has no SPI flash / gen2 PDDB path). Optional offline PDDB helpers are behind `ccid-pddb`.
- `CCID_BULK_MAX_PACKET = 512` for high-speed USB; `CCID_WIRE_MAX = 271` for CCID message framing (chunked across bulk packets).
- xtask recipe `dabao-ccid` added (same packages as `dabao` + `ccid-openpgp`).
- `baosec-ccid` / `ccid-hil` remain compile targets for baosec-shaped images; they were not the board under test for this report.

## Image targets

| `cargo xtask` target | CCID features | USB composite | Use |
|---------------------|---------------|---------------|-----|
| `dabao` | none | HID (+ CDC on stock dabao) | Default dabao |
| `dabao-ccid` | `ccid-openpgp` | CCID+FIDO+NKRO (7/8); no CDC | **Hardware-confirmed** CCID on Dabao |
| `baosec` | none | FIDO+NKRO+debug CDC (7/8) | Default / upstream-like |
| `baosec-ccid` | `ccid-openpgp` | CCID+FIDO+NKRO (7/8); UART debug | Baosec CCID transport (compile) |
| `ccid-hil` | `ccid-openpgp` + `ccid-echo` + `oem-baosec-lite` | Same as baosec-ccid + echo | USB HIL bench (baosec-shaped) |

## CI workflows

| Workflow | Runner | Hardware | Trigger |
|----------|--------|----------|-----------|
| `ccid-ci.yml` | GitHub-hosted Ubuntu | No (`install-toolkit` + check/xtask) | push/PR |
| `build.yml` (`baosec` matrix) | GitHub-hosted Ubuntu | No | push/PR |
| `ccid-hil.yml` | Self-hosted (`baosec-hil`) | Intended | nightly / manual (**no runner registered**) |

Fork CI: fetch annotated tags from `betrusted-io/xous-core` before swap signing.

## Historical note

An earlier draft referenced in-tree OpenPGP crates and USB provisioning CDC.
Current design: transport only on dabao-ccid; optional PDDB helpers via `ccid-pddb`
on baosec; Persona A drops all USB CDC on CCID images; OpenPGP stays out-of-tree
via `CcidRxDeferred` / `CcidTx`. Bulk MPS was briefly lowered to 64 during FS
debugging and restored to **512** once Dabao high-speed enumeration was confirmed.
