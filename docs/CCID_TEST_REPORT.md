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
| CCID wire framing | `cargo test -p usb-bao1x --lib ccid_framing` (7 tests) | Pass |
| Hosted compile | `cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp` | Pass |
| Board compile | `cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x` | Pass |
| HIL image build | `cargo xtask ccid-hil --no-verify` (incl. swap signing) | Pass |
| Full baosec matrix | `cargo xtask baosec --no-verify` | Pass |
| USB enumeration + bulk echo | `tools/ccid_smoke.py` on `ccid-hil` image | Pass (hardware) |
| HIL regression suite | `tools/ccid_hil/run_all.sh` | Pass (hardware) |
| Provisioning CDC | `test_provision.py` with `CCID_HIL_PROVISION=1` | Pass (unprovisioned device) |
| Fork CI | GitHub Actions on `Supermagnum/xous-core` | Pass |
| OpenPGP / APDU / GnuPG E2E | — | **Not in scope** (external handler) |
| Security architecture review | Manual | See [Security considerations](CCID_PROTOCOL_AND_HIL.md#security-considerations) in protocol doc |

## CI workflows

| Workflow | Runner | Trigger |
|----------|--------|---------|
| `ccid-ci.yml` | GitHub-hosted Ubuntu | push/PR to `main`, `dev`, `feature/**` |
| `build.yml` (`baosec` matrix job) | GitHub-hosted Ubuntu | same |
| `ccid-hil.yml` | Self-hosted (`baosec-hil`) | nightly / manual |

Fork CI note: workflows fetch annotated release tags from `betrusted-io/xous-core`
before image signing so `SemVer::from_git()` succeeds on fork clones that lack
local tags.

## Local / container verification (2026-07)

Reproduced in clean `ubuntu:24.04` containers (Podman/Docker):

1. **Signing failure without tags** — `cargo xtask baosec` fails with
   `Can't sign swap image` when `git describe` has no annotated tags (typical
   fork checkout).
2. **Signing success after upstream tag fetch** — after
   `git fetch upstream --tags` from `betrusted-io/xous-core`, `git describe`
   returns e.g. `v0.9.8-791-g...` and `cargo xtask ccid-hil --no-verify`
   completes through loader/kernel signing.
3. **Remote CI** — all four workflows green on push of commit `e8e7e86e5`
   (`build`, `ccid-ci`, `rustfmt_check`, `trailing_whitespace_check`).

## Explicitly not tested here

- Parsing `PC_to_RDR_XfrBlock` payloads into APDUs
- OpenPGP card command handling (SIGN, DECRYPT, etc.)
- `pcscd` or `gpg --card-status` against production `baosec` + handler
- CCID interrupt endpoint notifications (card insert/remove)

Those require the out-of-tree OpenPGP handler service once it ships.

## Source files under test

| Path | Role |
|------|------|
| `services/usb-bao1x/src/ccid_framing.rs` | Framing unit tests |
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning |
| `services/usb-bao1x/src/main.rs` | IPC, echo, provisioning state machine |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts |

## Historical note

An earlier draft of this file (2026-05-10) referenced in-tree `baochip-openpgp`
crate checks. That crate is **not** part of the merged design: xous-core provides
transport and PDDB provisioning only; OpenPGP logic remains in a separate
firmware service connected via `CcidRxDeferred` / `CcidTx` IPC.
