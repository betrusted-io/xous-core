<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID code map

Navigation guide for code-proficient developers who need to locate, debug, or fix
CCID and provisioning behavior in `usb-bao1x` ([PR #890](https://github.com/betrusted-io/xous-core/pull/890)).

**Related docs:** protocol, security, and testing —
[`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md);
verification status — [`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md).

Start from a **symptom** in [Symptom to code](#symptom-to-code-self-service-debug), then
open the listed file and function.

## Table of contents

1. [Runtime data flow (device)](#runtime-data-flow-device)
2. [Source files (device firmware)](#source-files-device-firmware)
3. [Key functions by concern](#key-functions-by-concern)
4. [Symptom to code (self-service debug)](#symptom-to-code-self-service-debug)
5. [Host-side test code](#host-side-test-code)
6. [CI and build entry points](#ci-and-build-entry-points)
7. [Compile-time feature branches](#compile-time-feature-branches)
8. [Out-of-tree (not in this repo)](#out-of-tree-not-in-this-repo)
9. [Related paths quick index](#related-paths-quick-index)

---

## Runtime data flow (device)

```
USB IRQ (hw.rs: composite_handler)
  |
  +-- bulk OUT --> ccid_transport.rs: endpoint_out
  |                 append_bulk_out / drain_complete_frames (ccid_framing.rs)
  |                 --> ccid_rx queue (VecDeque<Vec<u8>>)
  |                 --> IrqCcidRx scalar to usb-bao1x main loop
  |
  +-- bulk IN  <-- ccid_transport.rs: poll_bulk_in / enqueue_response
  |                 next_tx_chunk (ccid_framing.rs)
  |
  +-- prov CDC <-- hw.rs IRQ path (prov_capture_enabled)
                    --> prov_lines queue --> IrqProvSerialRx --> main.rs

main.rs message loop
  CcidRxDeferred / IrqCcidRx  --> deliver frame to handler (or ccid-echo)
  CcidTx                      --> ccid.enqueue_response
  IrqProvSerialRx             --> ccid_store::save_provisioned_pins + USB reset
```

Shared queues on `Bao1xUsb` (`hw.rs`):

| Field | Type | Producer | Consumer |
|-------|------|----------|----------|
| `ccid_rx` | `Rc<RefCell<VecDeque<Vec<u8>>>>` | `ccid_transport` | `main.rs` IPC / echo |
| `prov_lines` | `Rc<RefCell<VecDeque<Vec<u8>>>>` | `composite_handler` IRQ | `main.rs` `IrqProvSerialRx` |
| `prov_line_acc` | `RefCell<Vec<u8>>` | IRQ byte loop | Line delimiter handler |
| `prov_capture_enabled` | `Arc<AtomicBool>` | `main.rs` after save | IRQ gate for CDC read |

---

## Source files (device firmware)

| File | What to change here |
|------|---------------------|
| [`services/usb-bao1x/Cargo.toml`](../services/usb-bao1x/Cargo.toml) | Feature flags: `ccid-openpgp`, `ccid-echo`; optional `pddb` dep |
| [`services/usb-bao1x/src/api.rs`](../services/usb-bao1x/src/api.rs) | IPC contract: `Opcode::{CcidRxDeferred,CcidTx,IrqCcidRx,IrqProvSerialRx}`, `CcidMsgIpc`, `CcidCode` |
| [`services/usb-bao1x/src/ccid_framing.rs`](../services/usb-bao1x/src/ccid_framing.rs) | Wire math: `CCID_WIRE_MAX`, `append_bulk_out`, `drain_complete_frames`, `next_tx_chunk`; **unit tests** |
| [`services/usb-bao1x/src/ccid_transport.rs`](../services/usb-bao1x/src/ccid_transport.rs) | USB class 0x0B descriptors, bulk OUT assembly, bulk IN chunking, `enqueue_response` |
| [`services/usb-bao1x/src/ccid_store.rs`](../services/usb-bao1x/src/ccid_store.rs) | PDDB dict `usb.ccid`, keys `user_pin_line` / `admin_pin_line` / `provisioned` |
| [`services/usb-bao1x/src/hw.rs`](../services/usb-bao1x/src/hw.rs) | Composite gadget registration, `make_ccid_transport`, `make_provisioning_serial`, IRQ provisioning capture |
| [`services/usb-bao1x/src/main.rs`](../services/usb-bao1x/src/main.rs) | Boot: PDDB provision check, queue setup; loop: CCID IPC, echo branch, provisioning commit |
| [`services/usb-bao1x/src/lib.rs`](../services/usb-bao1x/src/lib.rs) | Public `ccid_framing` module; U2F client API (template for handler IPC) |
| [`xtask/src/main.rs`](../xtask/src/main.rs) | `baosec` = no CCID; `baosec-ccid` adds `ccid-openpgp`; `ccid-hil` adds echo + `oem-baosec-lite`; `pddb` before `usb-bao1x` in service order |

---

## Key functions by concern

| Concern | Location |
|---------|----------|
| CCID descriptor bytes | `ccid_transport.rs` — `ccid_class_descriptor_bytes`, `get_configuration_descriptors` |
| Reject oversize host frames | `ccid_framing.rs` — `append_bulk_out` returns `Overflow`, clears buffer |
| Frame ready notification | `ccid_transport.rs` — `drain_complete_messages` sends `IrqCcidRx` |
| Handler receives frame | `main.rs` — `Opcode::CcidRxDeferred` (~522), `Opcode::IrqCcidRx` (~550) |
| Handler sends reply | `main.rs` — `Opcode::CcidTx` (~601) calls `enqueue_response` |
| HIL echo (non-production) | `main.rs` — `#[cfg(feature = "ccid-echo")]` inside `IrqCcidRx` |
| Second listener rejected | `main.rs` — `CcidRxDeferred` sets `CcidCode::Denied` for other PIDs |
| Provisioning line parse | `hw.rs` — `composite_handler` prov CDC loop (~397) |
| Provisioning PDDB write | `main.rs` — `IrqProvSerialRx` (~574); `ccid_store::save_provisioned_pins` |
| USB reset after provision | `main.rs` — `cu.unplug()` (baosec) or `force_reset` |
| Already provisioned? | `ccid_store.rs` — `is_ccid_provisioned`; called at boot in `main.rs` (~159) |

---

## Symptom to code (self-service debug)

| Symptom | First places to inspect |
|---------|-------------------------|
| No CCID interface in `lsusb` | Built `baosec` (no CCID) instead of `baosec-ccid` / `ccid-hil`; `hw.rs` composite class list; missing `ccid-openpgp` |
| `echo mismatch` / smoke test fail | Image has `ccid-echo`? `main.rs` `IrqCcidRx` echo branch; host timing (`ccid_smoke.py`) |
| Handler never receives frames | Handler connected to `_Xous USB device driver_`? `CcidRxDeferred` with `RxWait`; production image must **not** use `ccid-echo` |
| `CcidCode::Denied` on receive | Only one listener PID; second process blocked in `main.rs` `CcidRxDeferred` |
| `CcidCode::Hangup` on send | USB not configured; `main.rs` `CcidTx` checks `UsbDeviceState::Configured` |
| Partial / truncated CCID frames | `ccid_framing.rs` `drain_complete_frames`; host sending before configured |
| Oversize frame / silent drop | `append_bulk_out` overflow path in `ccid_transport.rs` `endpoint_out` |
| Bulk IN stuck / no reply | `poll_bulk_in`, `tx_pending` in `ccid_transport.rs`; handler called `CcidTx`? |
| Provisioning port missing | Already `OKV1` in PDDB (`is_ccid_provisioned` at boot); `hw.rs` skips `provision_serial` when provisioned |
| Provisioning port won't accept lines | `prov_capture_enabled` false; non-printable bytes filtered in `hw.rs` |
| Provisioning saved but no reset | `save_provisioned_pins` error path; `main.rs` `IrqProvSerialRx` |
| PDDB keys wrong / missing | `ccid_store.rs` dict and key names; PDDB basis policy (out of tree) |
| Board compile error in CI | `ccid-ci.yml` feature set; `RefCell`/`borrow` patterns in `hw.rs` / `main.rs` |
| Fork CI `Can't sign swap image` | `.github/workflows/build.yml` upstream tag fetch step |
| Unit test regression | `ccid_framing.rs` `mod tests`; run `cargo test -p usb-bao1x --lib ccid_framing` |

---

## Host-side test code

| File | Role |
|------|------|
| [`tools/ccid_smoke.py`](../tools/ccid_smoke.py) | Single-shot enumeration + echo check |
| [`tools/ccid_hil/ccid_usb.py`](../tools/ccid_hil/ccid_usb.py) | Frame builders, device find, bulk roundtrip |
| [`tools/ccid_hil/test_enumerate.py`](../tools/ccid_hil/test_enumerate.py) | Descriptor field assertions (`HIL-01`) |
| [`tools/ccid_hil/test_echo.py`](../tools/ccid_hil/test_echo.py) | GetSlotStatus / XfrBlock echo (`HIL-03`, `HIL-05`) |
| [`tools/ccid_hil/test_provision.py`](../tools/ccid_hil/test_provision.py) | CDC two-line provision (`HIL-02`) |
| [`tools/ccid_hil/run_all.sh`](../tools/ccid_hil/run_all.sh) | Ordered suite driver |
| [`tools/ccid_hil/wait_device.sh`](../tools/ccid_hil/wait_device.sh) | USB presence gate |

To reproduce a HIL failure locally: run the exact script from the CI log, then
match the failing step to the table above.

---

## CI and build entry points

| Path | Runs |
|------|------|
| [`.github/workflows/ccid-ci.yml`](../.github/workflows/ccid-ci.yml) | Unit tests + hosted/board check + `ccid-hil` compile |
| [`.github/workflows/build.yml`](../.github/workflows/build.yml) | Full `cargo xtask baosec` matrix (default image, no CCID) |
| [`.github/workflows/ccid-hil.yml`](../.github/workflows/ccid-hil.yml) | Self-hosted `tools/ccid_hil/run_all.sh` |

Local equivalents:

```bash
cargo test -p usb-bao1x --lib ccid_framing
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x --target riscv32imac-unknown-xous-elf
cargo xtask ccid-hil --no-verify
python3 tools/ccid_smoke.py
```

---

## Compile-time feature branches

When reading `main.rs`, note `cfg` gates:

| `cfg` | Effect |
|-------|--------|
| `feature = "ccid-openpgp"` | All CCID + provisioning code compiled in |
| `feature = "ccid-echo"` | `IrqCcidRx` echoes frames; **disables** `CcidRxDeferred` handler path |
| `not(feature = "ccid-echo")` | Production path: deferred listener + `CcidRxDeferred` opcodes |
| `target_os = "xous"` | `ccid_transport` / `ccid_store` are device-only modules |

---

## Out-of-tree (not in this repo)

| Component | Responsibility |
|-----------|----------------|
| OpenPGP handler service | APDU/T=1 parse, crypto, `CcidRxDeferred` / `CcidTx` client |
| Factory provisioning tool | Sends two lines on CDC during trusted setup |
| `pcscd` / GnuPG on host | End-user smart-card access |

Handler authors: copy the
[Handler skeleton (Rust)](CCID_PROTOCOL_AND_HIL.md#handler-skeleton-rust) in the
protocol doc and wire the process into the product's Xous service table.

---

## Related paths quick index

| Path | Purpose |
|------|---------|
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class driver |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire format helpers + unit tests |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning storage |
| `services/usb-bao1x/src/api.rs` | IPC opcodes and `CcidMsgIpc` |
| `services/usb-bao1x/src/main.rs` | Deferred listener, echo, provisioning |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts and suite |
| `.github/workflows/ccid-ci.yml` | CI compile + unit tests |
| `.github/workflows/ccid-hil.yml` | Nightly Pi HIL |
