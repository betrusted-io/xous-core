<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID smart-card transport verification report

This report records verification status for the `usb-bao1x` CCID transport
(`ccid-openpgp` feature) on branch `feature/usb-bao1x-ccid-openpgp`
([PR #890](https://github.com/betrusted-io/xous-core/pull/890)).

For protocol background, handler integration, Pi HIL setup, **security
considerations**, and source navigation, see [`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md)
and [`code_map.md`](code_map.md).

## What is verified in xous-core

| Area | Method | Status |
|------|--------|--------|
| CCID wire framing | `cargo test -p usb-bao1x --lib ccid_framing` (7 tests) | Pass (CI) |
| Hosted compile | `cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp` | Pass (CI) |
| Board compile | `cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x` | Pass (CI) |
| HIL image build | `cargo xtask ccid-hil --no-verify` (incl. swap signing) | Pass (CI) |
| CCID image build | `cargo xtask baosec-ccid --no-verify` | Pass (CI compile) |
| Default `baosec` image | `cargo xtask baosec --no-verify` | Pass (no CCID; matches upstream) |
| USB enumeration + bulk echo | `tools/ccid_smoke.py` on flashed device | **Manual only** (not in CI) |
| HIL regression suite | `tools/ccid_hil/run_all.sh` | **Manual only** (not in CI) |
| Provisioning CDC | `test_provision.py` with `CCID_HIL_PROVISION=1` | **Manual only** |
| Fork CI (compile) | GitHub Actions on `Supermagnum/xous-core` | Pass |
| `ccid-hil.yml` self-hosted | Runner label `baosec-hil` | **Not deployed** (workflow scaffolding only) |
| OpenPGP / APDU / GnuPG E2E | — | **Not in scope** (external handler) |
| Security architecture review | Manual | See [Security considerations](CCID_PROTOCOL_AND_HIL.md#security-considerations) |

## CI workflows

| Workflow | Runner | Hardware | Trigger |
|----------|--------|----------|-----------|
| `ccid-ci.yml` | GitHub-hosted Ubuntu | No | push/PR |
| `build.yml` (`baosec` matrix job) | GitHub-hosted Ubuntu | No | push/PR |
| `ccid-hil.yml` | Self-hosted (`baosec-hil`) | Intended | nightly / manual (no runner registered) |

Fork CI note: workflows fetch annotated release tags from `betrusted-io/xous-core`
before image signing so `SemVer::from_git()` succeeds on fork clones that lack
local tags.

## Image targets

| `cargo xtask` target | CCID features | Use |
|---------------------|---------------|-----|
| `baosec` | none | Default production image (unchanged vs upstream `dev`) |
| `baosec-ccid` | `ccid-openpgp` | Production CCID transport + provisioning |
| `ccid-hil` | `ccid-openpgp` + `ccid-echo` + `oem-baosec-lite` | USB HIL bench testing |

## Local / container verification (2026-07)

Reproduced in clean `ubuntu:24.04` containers (Podman/Docker):

1. **Signing failure without tags** — fork `git describe` fails; swap signing aborts.
2. **Signing success after upstream tag fetch** — `cargo xtask ccid-hil --no-verify` completes.
3. **Remote compile CI** — `build`, `ccid-ci`, `rustfmt_check`, `trailing_whitespace_check` green on fork.

Container and GitHub CI do **not** attach a USB device.

## Explicitly not tested here

- Parsing `PC_to_RDR_XfrBlock` payloads into APDUs
- OpenPGP card command handling (SIGN, DECRYPT, etc.)
- `pcscd` or `gpg --card-status` against production `baosec-ccid` + handler
- CCID interrupt endpoint notifications (card insert/remove)
- Automated hardware regression in GitHub Actions (no self-hosted runner)

Those require bench hardware plus (for E2E) the out-of-tree OpenPGP handler service.

## Historical note

An earlier draft of this file (2026-05-10) referenced in-tree `baochip-openpgp`
crate checks. That crate is **not** part of the merged design: xous-core provides
transport and PDDB provisioning only; OpenPGP logic remains in a separate
firmware service connected via `CcidRxDeferred` / `CcidTx` IPC.
