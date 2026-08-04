<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID and USB code map

Navigation guide for code-proficient developers who need to locate, debug, or fix
USB enumeration and CCID transport behavior in `usb-bao1x`
([PR #890](https://github.com/betrusted-io/xous-core/pull/890)).
PDDB PIN blobs are offline-only on CCID images (Persona A — no USB provision CDC).

**Related docs:** protocol, security, HIL setup —
[`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md);
verification status — [`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md);
enumeration deep-dive (community) —
[`CCID_USB_ENUMERATION_DEBUG.md`](CCID_USB_ENUMERATION_DEBUG.md).

**Start here:** [Debug decision tree](#debug-decision-tree) (one host check, then
symptom table). Do not paste code snippets into the PR until the tree step
identifies the layer.

## Table of contents

1. [Debug decision tree](#debug-decision-tree)
2. [USB enumeration flow (base stack)](#usb-enumeration-flow-base-stack)
3. [Runtime data flow (CCID layer)](#runtime-data-flow-ccid-layer)
4. [Source files (device firmware)](#source-files-device-firmware)
5. [Key functions by concern](#key-functions-by-concern)
6. [Symptom to code (self-service debug)](#symptom-to-code-self-service-debug)
7. [Host checks (copy-paste)](#host-checks-copy-paste)
8. [Host-side test code](#host-side-test-code)
9. [CI and build entry points](#ci-and-build-entry-points)
10. [Compile-time feature branches](#compile-time-feature-branches)
11. [Out-of-tree (not in this repo)](#out-of-tree-not-in-this-repo)
12. [Related paths quick index](#related-paths-quick-index)

---

## Debug decision tree

Work top to bottom. Each step names the firmware layer and the doc section to
open next.

```
Flashed image?
  |
  +-- Unknown / wrong target
  |     --> Confirm: cargo xtask dabao | dabao-ccid | baosec | baosec-ccid | ccid-hil
  |         (see CCID_TEST_REPORT.md "Image targets")
  |         dabao / baosec = no CCID
  |         dabao-ccid / baosec-ccid / ccid-hil = CCID transport enabled
  |         Hardware-confirmed CCID enum: Dabao dabao-ccid (1d50:6197, HS bulk MPS 512)
  |
  +-- Known target --> Host: lsusb -d 1d50:6197 (dabao) or 1d50:6198 (baosec)
        |
        +-- NO LINE (device not visible at all)
        |     --> BASE USB LAYER — not CCID-specific
        |         1. Flash cargo xtask dabao or baosec (baseline). Still nothing?
        |            Problem is pre-CCID: board power, cable, SE0, Corigine driver.
        |         2. If stock works but *-ccid / ccid-hil does not:
        |            Endpoint budget or CCID boot path — see symptom table rows
        |            "Nothing enumerates" and "CCID image breaks enumeration".
        |         Files: main.rs boot, hw.rs init/poll, driver.rs handle_event_inner
        |
        +-- DEVICE VISIBLE, lsusb -v shows HID + serial, NO interface class 0x0B
        |     --> Wrong image (stock dabao/baosec) OR ccid-openpgp not in feature set
        |         Fix: cargo xtask dabao-ccid, baosec-ccid, or ccid-hil
        |         Files: xtask/src/main.rs, usb-bao1x/Cargo.toml
        |
        +-- DEVICE VISIBLE, class 0x0B (CCID) present
              |
              +-- ccid_smoke.py / HIL echo fails
              |     --> CCID transport layer (framing, echo, configured state)
              |         Files: ccid_transport.rs, ccid_framing.rs, main.rs IrqCcidRx
              |
              +-- Echo OK, OpenPGP / pcscd fails
                    --> Out-of-tree handler not connected to CcidRxDeferred / CcidTx
                        See protocol doc handler skeleton
```

**Minimum PR report** (if still stuck after the tree): flashed `xtask` target,
output of `lsusb -d 1d50:6198`, and whether stock `baosec` enumerates on the
same board. No firmware source paste required if this table was followed.

---

## USB enumeration flow (base stack)

Enumeration is **interrupt-driven**, not done in the `main.rs` IPC loop.

```
main_hw() boot (main.rs)
  |
  +-- Map Corigine USB MMIO + IFRAM + IRQ CSR
  +-- CorigineUsb::new + reset
  +-- Bao1xUsb::new — UsbDeviceBuilder + register UsbClass instances
  |     stock: FIDO+NKRO+debug CDC
  |     ccid-openpgp (Persona A): CCID+FIDO+NKRO only (no USB CDC)
  +-- cu.init() — claim IRQ, core init/start, enable EV_ENABLE
  +-- setup_usb_pins + SE0 released (Input) — host can see device
  |
  v
main loop: reply_and_receive_next only (IPC). USB runs in IRQ.

Corigine hardware IRQ
  |
  v
composite_handler (hw.rs)
  |
  +-- handle_event_inner (libs/bao1x-hal/.../driver.rs)
  |     Port reset, cable connect, EP0 setup/data, bulk completion
  |     --> CrgEvent stored on CorigineWrapper
  |
  +-- device.poll(&mut [classes...])  (usb-device crate)
  |     Host GET_DESCRIPTOR, SET_ADDRESS, SET_CONFIGURATION
  |     State becomes UsbDeviceState::Configured
  |
  +-- Class I/O in same IRQ (HID reports; optional serial read; CCID bulk)
```

| Stage | File | Symbol / area |
|-------|------|----------------|
| Gadget assembly | `hw.rs` | `Bao1xUsb::new`, `EpBudgetLedger`, class list |
| Controller start | `hw.rs` | `Bao1xUsb::init` |
| SE0 / attach | `main.rs` | `setup_usb_pins`, `set_gpio_pin_dir(..., Input)` |
| IRQ entry | `hw.rs` | `composite_handler` |
| Low-level events | `libs/bao1x-hal/src/usb/driver.rs` | `handle_event_inner` |
| Bus poll adapter | `libs/bao1x-hal/src/usb/driver.rs` | `CorigineWrapper::poll`, `set_device_address`, `reset` |
| Configured gate | `main.rs` | `cu.device.state() != Configured` on U2fTx, CcidTx, etc. |
| Forced re-enumerate | `hw.rs` / `main.rs` | `Bao1xUsb::unplug`, PMIC `VbusIrq::Remove` |

**Endpoint budget:** Corigine `CRG_EP_NUM = 8`. Persona A (`ccid-openpgp`):
CCID(3)+FIDO(2)+NKRO(2)=**7/8**; debug and provisioning CDC are never allocated.
Stock `baosec`: FIDO+NKRO+debug CDC = **7/8**. Debug on CCID images uses
`xous-log` UART/DUART (`services/xous-log/.../bao1x`).

**Guard:** `ep_budget::EpBudgetLedger` tracks the **cumulative** reserved total
across classes (not independent subtotals). Each `reserve_before_alloc` runs
before that class's `alloc.*` calls; after all classes,
`assert_matches_live(cw.allocated_non_ep0_count())` checks the shared counter
updated inside `CorigineWrapper::alloc_ep`. Per-class `assert_class_ep_budget`
remains. Regression: `ep_budget` tests + `tools/test_ep_budget_cumulative.py`
(fake class on a 7/8 stack must trip cumulative; independent checks would not).

**Service boot order (CCID images):** `pddb` must start before `usb-bao1x` in
`baosec_common()` because `main.rs` calls `pddb::Pddb::new()` before `cu.init()`
when `ccid-openpgp` is enabled (OKV1 check / warn only; no USB provision path).

---

## Runtime data flow (CCID layer)

```
USB IRQ (hw.rs: composite_handler)
  |
  +-- bulk OUT --> ccid_transport.rs: endpoint_out
  |                 append_bulk_out / drain_complete_frames (ccid_framing.rs)
  |                 --> ccid_rx queue (VecDeque<Vec<u8>>)
  |                 --> IrqCcidRx scalar to usb-bao1x main loop
  |
  +-- bulk IN  <-- ccid_transport.rs: poll_bulk_in / enqueue_response
                    next_tx_chunk (ccid_framing.rs)

main.rs message loop
  CcidRxDeferred / IrqCcidRx  --> deliver frame to handler (or ccid-echo)
  CcidTx                      --> ccid.enqueue_response
  (no USB PIN provision path — Persona A)
```

Shared queues on `Bao1xUsb` (`hw.rs`):

| Field | Type | Producer | Consumer |
|-------|------|----------|----------|
| `ccid_rx` | `Rc<RefCell<VecDeque<Vec<u8>>>>` | `ccid_transport` | `main.rs` IPC / echo |

---

## Source files (device firmware)

| File | What to change here |
|------|---------------------|
| [`libs/bao1x-hal/src/usb/driver.rs`](../libs/bao1x-hal/src/usb/driver.rs) | Corigine UDC: `handle_event_inner`, EP0, `set_device_address`, `PollResult`, port reset |
| [`services/usb-bao1x/Cargo.toml`](../services/usb-bao1x/Cargo.toml) | Feature flags: `ccid-openpgp`, `ccid-echo`; optional `pddb` dep |
| [`services/usb-bao1x/src/api.rs`](../services/usb-bao1x/src/api.rs) | IPC contract: `Opcode::{CcidRxDeferred,CcidTx,IrqCcidRx}`, `CcidMsgIpc`, `CcidCode` |
| [`services/usb-bao1x/src/ep_budget.rs`](../services/usb-bao1x/src/ep_budget.rs) | Cumulative EP ledger; regression: fake class on 7/8 |
| [`services/usb-bao1x/src/ccid_framing.rs`](../services/usb-bao1x/src/ccid_framing.rs) | Wire math: `CCID_WIRE_MAX` (271), `CCID_BULK_MAX_PACKET` (**512** HS), `append_bulk_out`, `drain_complete_frames`, `next_tx_chunk`; **unit tests** |
| [`services/usb-bao1x/src/ccid_transport.rs`](../services/usb-bao1x/src/ccid_transport.rs) | USB class 0x0B descriptors, bulk OUT assembly, bulk IN chunking (512-byte packets), `enqueue_response` |
| [`services/usb-bao1x/src/ccid_store.rs`](../services/usb-bao1x/src/ccid_store.rs) | PDDB dict `usb.ccid`; `is_ccid_provisioned`; offline `save_provisioned_pins` |
| [`services/usb-bao1x/src/hw.rs`](../services/usb-bao1x/src/hw.rs) | Composite gadget, EP budget assert, `device.poll` (HID+CCID or HID+serial) |
| [`services/usb-bao1x/src/main.rs`](../services/usb-bao1x/src/main.rs) | Boot, SE0, OKV1 log/warn, IPC loop, serial opcodes gated off on CCID |
| [`services/usb-bao1x/src/lib.rs`](../services/usb-bao1x/src/lib.rs) | Public `ccid_framing` module; U2F client API (template for handler IPC) |
| [`xtask/src/main.rs`](../xtask/src/main.rs) | `dabao` / `baosec` = no CCID; `dabao-ccid` / `baosec-ccid` add `ccid-openpgp`; `ccid-hil` adds echo + `oem-baosec-lite` |

---

## Key functions by concern

| Concern | Location |
|---------|----------|
| Enumeration (control plane) | `driver.rs` `handle_event_inner`; `hw.rs` `composite_handler` + `device.poll` |
| SE0 release for host attach | `main.rs` ~327 `set_gpio_pin_dir(..., Input)` |
| SET_ADDRESS / EP enable | `driver.rs` `set_device_address` ~2524 |
| Bus reset handling | `driver.rs` `EventPortStatusChange`; `hw.rs` reset branch in `composite_handler` |
| Endpoint allocation limit | `driver.rs` `CRG_EP_NUM` + `allocated_non_ep0`; `ep_budget::EpBudgetLedger` (cumulative) |
| Poll class list | `hw.rs` `composite_handler` — HID+CCID (Persona A) or HID+debug CDC (stock) |
| CCID descriptor bytes | `ccid_transport.rs` — `ccid_class_descriptor_bytes`, `get_configuration_descriptors` |
| Reject oversize host frames | `ccid_framing.rs` — `append_bulk_out` returns `Overflow`, clears buffer |
| Frame ready notification | `ccid_transport.rs` — `drain_complete_messages` sends `IrqCcidRx` |
| Handler receives frame | `main.rs` — `Opcode::CcidRxDeferred`, `Opcode::IrqCcidRx` |
| Handler sends reply | `main.rs` — `Opcode::CcidTx` calls `enqueue_response` |
| HIL echo (non-production) | `main.rs` — `#[cfg(feature = "ccid-echo")]` inside `IrqCcidRx` |
| Second listener rejected | `main.rs` — `CcidRxDeferred` sets `CcidCode::Denied` for other PIDs |
| Already provisioned? | `ccid_store.rs` — `is_ccid_provisioned`; boot log/warn in `main.rs` |
| Offline PIN seed helper | `ccid_store.rs` — `save_provisioned_pins` (not USB-wired on CCID) |
| PMIC unplug reset | `main.rs` — `Opcode::PmicIrq`, `cu.unplug()` |

---

## Symptom to code (self-service debug)

| Symptom | First places to inspect |
|---------|-------------------------|
| **Nothing enumerates** (`lsusb` empty for `1d50:6198`) | **Layer 1:** `main.rs` boot order, `cu.init()`, SE0 GPIO ~327. **Layer 2:** `composite_handler` running? `driver.rs` `handle_event_inner`. **Layer 3:** CCID image only — `pddb` before `usb-bao1x` in `xtask`; PDDB hang in `main.rs` ~157. **Baseline:** flash `baosec`; if that also fails, not CCID-specific. |
| **CCID image breaks enumeration; `baosec` works** | Endpoint budget: Persona A must stay at CCID+FIDO+NKRO (7/8). Check `EpBudgetLedger` / accidental CDC add. Boot: `pddb` before `usb-bao1x`. |
| Device visible but **stuck at "new full-speed USB"** / never configured | `driver.rs` EP0 / `set_device_address`; `composite_handler` double-lock (`double_lock_detected` in main loop). Host `dmesg` for STALL. |
| **Double lock** log in main loop | `hw.rs` `composite_handler` ~306 `try_lock` failure path |
| No CCID interface in `lsusb -v` | Built stock `dabao` / `baosec` (no CCID) instead of `dabao-ccid` / `baosec-ccid` / `ccid-hil`; missing `ccid-openpgp` feature |
| `echo mismatch` / smoke test fail | Image has `ccid-echo`? `main.rs` `IrqCcidRx` echo branch; host timing (`ccid_smoke.py`) |
| Handler never receives frames | Handler on `_Xous USB device driver_`? `CcidRxDeferred` + `RxWait`; production must **not** use `ccid-echo` |
| `CcidCode::Denied` on receive | Only one listener PID; `main.rs` `CcidRxDeferred` |
| `CcidCode::Hangup` on send | USB not configured; `main.rs` `CcidTx` checks `UsbDeviceState::Configured` |
| Partial / truncated CCID frames | `ccid_framing.rs` `drain_complete_frames`; host sending before configured |
| Oversize frame / silent drop | `append_bulk_out` overflow in `ccid_transport.rs` `endpoint_out` |
| Bulk IN stuck / no reply | `poll_bulk_in`, `tx_pending` in `ccid_transport.rs`; handler called `CcidTx`? |
| No USB CDC / no provision port on CCID | Expected (Persona A); use UART (`xous-log`) for debug |
| PDDB not OKV1 on CCID image | Expected warn at boot; seed PDDB offline — no USB provision |
| PDDB keys wrong / missing | `ccid_store.rs`; PDDB basis policy (out of tree) |
| `test_provision.py` fails | CDC present (Persona A regression) or pyusb/permissions; not “missing provision port” |
| `test_provision.py` PASS | Confirms no CDC — does **not** prove PDDB OKV1 |
| Board compile error in CI | `ccid-ci.yml`; `RefCell`/`borrow` in `hw.rs` / `main.rs` |
| Fork CI `Can't sign swap image` | `.github/workflows/build.yml` upstream tag fetch |
| Unit test regression | `ccid_framing.rs` `mod tests`; `cargo test -p usb-bao1x --lib ccid_framing` |

---

## Host checks (copy-paste)

Run on the machine with the device plugged in. Interpret via
[Debug decision tree](#debug-decision-tree).

```bash
# Step 1 — is anything visible?
lsusb -d 1d50:6197   # dabao (hardware-confirmed CCID)
# lsusb -d 1d50:6198   # baosec

# Step 2 — interfaces (stock: HID+CDC; ccid images: HID+CCID 0x0B, no CDC)
# Expect CCID bulk wMaxPacketSize 0x0200 (512) on high-speed
lsusb -d 1d50:6197 -v 2>/dev/null | grep -E 'bInterfaceClass|iInterface|idProduct|wMaxPacketSize'

# Step 3 — kernel view (stall, reset loops)
dmesg -T | tail -30

# Step 4 — CCID transport only (ccid-hil or *-ccid + ccid-echo image)
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197
```

Expected `idProduct`: dabao `0x6197`, baosec `0x6198` (`hw.rs` `UsbVidPid(0x1d50, pid)`).
Confirmed on hardware: Dabao `dabao-ccid`, HS 480 Mbps, CCID bulk MPS 512.

---

## Host-side test code

| File | Role |
|------|------|
| [`tools/ccid_smoke.py`](../tools/ccid_smoke.py) | Single-shot enumeration + echo check |
| [`tools/ccid_hil/ccid_usb.py`](../tools/ccid_hil/ccid_usb.py) | Frame builders, device find, bulk roundtrip |
| [`tools/ccid_hil/test_enumerate.py`](../tools/ccid_hil/test_enumerate.py) | Descriptor field assertions (`HIL-01`) |
| [`tools/ccid_hil/test_echo.py`](../tools/ccid_hil/test_echo.py) | GetSlotStatus / XfrBlock echo (`HIL-03`, `HIL-05`) |
| [`tools/ccid_hil/test_provision.py`](../tools/ccid_hil/test_provision.py) | HIL-02: assert **no CDC** on CCID images (Persona A); why: USB PIN path must stay gone |
| [`tools/check_ep_budget.py`](../tools/check_ep_budget.py) | Static EP totals vs `CRG_EP_NUM` (why: catch overflow before HIL) |
| [`tools/test_ep_budget_cumulative.py`](../tools/test_ep_budget_cumulative.py) | Old independent vs cumulative guard gap |
| [`services/usb-bao1x/src/ep_budget.rs`](../services/usb-bao1x/src/ep_budget.rs) | Cumulative `EpBudgetLedger` + unit tests |
| [`tools/ccid_hil/run_all.sh`](../tools/ccid_hil/run_all.sh) | Ordered suite driver |
| [`tools/ccid_hil/wait_device.sh`](../tools/ccid_hil/wait_device.sh) | USB presence gate |

To reproduce a HIL failure: run the failing script, then match the step to the
symptom table above.

---

## CI and build entry points

| Path | Runs |
|------|------|
| [`.github/workflows/ccid-ci.yml`](../.github/workflows/ccid-ci.yml) | Unit tests + hosted/board check + `baosec-ccid` + `ccid-hil` compile |
| [`.github/workflows/build.yml`](../.github/workflows/build.yml) | Full `cargo xtask baosec` matrix (default image, no CCID) |
| [`.github/workflows/ccid-hil.yml`](../.github/workflows/ccid-hil.yml) | Self-hosted `tools/ccid_hil/run_all.sh` (scaffolding; no runner yet) |

Local equivalents:

```bash
cargo test -p usb-bao1x --lib ccid_framing
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x --target riscv32imac-unknown-xous-elf
cargo xtask dabao-ccid --no-verify      # hardware-confirmed CCID on Dabao
cargo xtask baosec --no-verify          # baseline USB (no CCID)
cargo xtask baosec-ccid --no-verify     # baosec CCID transport
cargo xtask ccid-hil --no-verify        # CCID + echo for bench
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197
```

---

## Compile-time feature branches

When reading `main.rs`, note `cfg` gates:

| `cfg` | Effect |
|-------|--------|
| `feature = "ccid-openpgp"` | CCID+FIDO+NKRO; no USB CDC; `pddb` OKV1 check at boot |
| `feature = "ccid-echo"` | `IrqCcidRx` echoes frames; **disables** `CcidRxDeferred` handler path |
| `not(feature = "ccid-echo")` | Production path: deferred listener + `CcidRxDeferred` opcodes |
| `target_os = "xous"` | `ccid_transport` / `ccid_store` are device-only modules |

---

## Out-of-tree (not in this repo)

| Component | Responsibility |
|-----------|----------------|
| OpenPGP handler service | APDU/T=1 parse, crypto, `CcidRxDeferred` / `CcidTx` client |
| Factory tooling | Seeds PDDB (`usb.ccid` / `OKV1`) offline before CCID image flash |
| `pcscd` / GnuPG on host | End-user smart-card access |

Handler authors: copy the
[Handler skeleton (Rust)](CCID_PROTOCOL_AND_HIL.md#handler-skeleton-rust) in the
protocol doc and wire the process into the product's Xous service table.

---

## Related paths quick index

| Path | Purpose |
|------|---------|
| `libs/bao1x-hal/src/usb/driver.rs` | Corigine UDC, enumeration events, EP0 |
| `services/usb-bao1x/src/hw.rs` | Composite gadget, IRQ handler, `device.poll` |
| `services/usb-bao1x/src/main.rs` | Boot, SE0, IPC loop, configured gates |
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class driver |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire format helpers + unit tests |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning storage |
| `services/usb-bao1x/src/api.rs` | IPC opcodes and `CcidMsgIpc` |
| `xtask/src/main.rs` | Image targets and service order |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts and suite |
| `.github/workflows/ccid-ci.yml` | CI compile + unit tests |
| `.github/workflows/ccid-hil.yml` | Nightly Pi HIL (scaffolding) |
