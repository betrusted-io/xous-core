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

## What is verified in xous-core

| Area | Method | What / why | Status |
|------|--------|------------|--------|
| CCID wire framing | `cargo test -p usb-bao1x --lib ccid_framing` | Pure-Rust frame assemble/chunk/overflow math — no USB needed | Pass (CI): **7** tests |
| Cumulative EP budget (ledger) | `cargo test -p usb-bao1x --lib ep_budget` | Proves independent subtotals miss overflow; cumulative reserve catches 7+2 | Pass (CI): **4** tests |
| EP budget arithmetic (all targets) | `python3 tools/check_ep_budget.py` | Static 7/8 for stock + Persona A; rejected 10/13 combos | Pass (local, verified) |
| Cumulative gap demo | `python3 tools/test_ep_budget_cumulative.py` | Documents old vs new guard semantics without rustc | Pass (local, verified) |
| Persona A layout mock | `python3 tools/sim_persona_a_composite.py` | Host-side asserts without pyusb/hardware | Pass (local, verified) |
| Hosted compile | `cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp` | Client + framing/API link on host | Pass (CI) |
| Board compile (hw.rs) | `cargo check … --target riscv32imac-unknown-xous-elf` | Links `EpBudgetLedger`, `Bao1xUsb::new`, Corigine path for Persona A | Pass (CI + local after `install-toolkit`) |
| HIL image build | `cargo xtask ccid-hil --no-verify` | Image with `ccid-echo` for bench | Pass (CI compile) |
| CCID image build | `cargo xtask baosec-ccid --no-verify` | Production CCID image compile | Pass (CI compile) |
| Default `baosec` | `cargo xtask baosec --no-verify` | No CCID; stock FIDO+NKRO+debug CDC | Pass (no CCID) |
| USB smoke (enum + echo) | `tools/ccid_smoke.py` | Host sees CCID; Persona A composite; `ccid-echo` round-trip | **Manual HIL** |
| HIL suite | `tools/ccid_hil/run_all.sh` | Enum + no-CDC + echo/stress on real USB | **Manual HIL** (no runner) |
| OpenPGP / APDU / GnuPG E2E | — | Out of tree | **Not in scope** |
| UART log capture in HIL | — | Would prove OKV1 warn vs continue | **OPEN** |

### HIL suite steps (what / why)

| Step | Script | What it tests | Why |
|------|--------|---------------|-----|
| 00 | `wait_device.sh` | VID:PID present | Gate: no point running USB tests without a device |
| 01 | `test_enumerate.py` | CCID descriptor fields + Persona A composite (0 CDC, ≥2 HID, 7 non-EP0) | Catch wrong image / EP-budget regressions that still compile |
| 02 | `test_provision.py` | **No** CDC ACM for VID:PID; refuses legacy USB PIN path | Persona A: USB PIN provision must stay gone; CDC reappearance is a fail |
| 03 | `test_echo.py` | GetSlotStatus + XfrBlock echo | Transport works without OpenPGP handler (`ccid-echo` image only) |
| 04 | `test_echo.py --stress` | Many random XfrBlocks | Stress reassembly / bulk path |

`CCID_HIL_PROVISION` is obsolete (ignored). PIN lines are offline/PDDB only on CCID images.

## CI workflows

| Workflow | Runner | Hardware | Trigger |
|----------|--------|----------|-----------|
| `ccid-ci.yml` | GitHub-hosted Ubuntu | No (`install-toolkit` + check/xtask) | push/PR |
| `build.yml` (`baosec` matrix) | GitHub-hosted Ubuntu | No | push/PR |
| `ccid-hil.yml` | Self-hosted (`baosec-hil`) | Intended | nightly / manual (**no runner registered**) |

Fork CI: fetch annotated tags from `betrusted-io/xous-core` before swap signing.

## Image targets

| `cargo xtask` target | CCID features | USB composite | Use |
|---------------------|---------------|---------------|-----|
| `baosec` | none | FIDO+NKRO+debug CDC (7/8) | Default / upstream-like |
| `baosec-ccid` | `ccid-openpgp` | CCID+FIDO+NKRO (7/8); UART debug | Production CCID transport |
| `ccid-hil` | `ccid-openpgp` + `ccid-echo` + `oem-baosec-lite` | Same as baosec-ccid + echo | USB HIL bench |

## Local verification notes

- Board target requires `cargo xtask install-toolkit` (custom libstd from
  `betrusted-io/rust`; host rustc version must match).
- Container/GitHub CI do **not** attach USB hardware.
- Python EP scripts (`check_ep_budget.py`, etc.) remain local fast gates;
  `ep_budget` Rust unit tests run in `ccid-ci.yml`.

## Explicitly not tested here

- Parsing `PC_to_RDR_XfrBlock` payloads into APDUs
- OpenPGP card commands / `pcscd` / `gpg --card-status` E2E
- CCID interrupt insert/remove notifications
- Automated USB HIL in GitHub Actions (no self-hosted runner)
- Automated UART capture of boot OKV1 warn lines

## Historical note

An earlier draft referenced in-tree OpenPGP crates and USB provisioning CDC.
Current design: transport + PDDB helpers only; Persona A drops all USB CDC on
CCID images; OpenPGP stays out-of-tree via `CcidRxDeferred` / `CcidTx`.
