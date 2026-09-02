<!--
SPDX-License-Identifier: Apache-2.0
-->

# CCID and USB code map

Navigation guide for code-proficient developers who need to locate, debug, or fix
USB enumeration and CCID transport behavior in `usb-bao1x`
([PR #937](https://github.com/betrusted-io/xous-core/pull/937);
earlier transport work [PR #890](https://github.com/betrusted-io/xous-core/pull/890)).
PDDB PIN blobs are offline-only on CCID images (Persona A — no USB provision CDC).

**Related docs:** protocol, security, HIL setup —
[`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md);
verification status — [`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md);
enumeration deep-dive (community) —
[`CCID_USB_ENUMERATION_DEBUG.md`](CCID_USB_ENUMERATION_DEBUG.md);
`openpgp-apdu` 8-process boot failure —
[`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md).

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
11. [In-tree openpgp-apdu handler](#in-tree-openpgp-apdu-handler)
12. [Out-of-tree (not in this repo)](#out-of-tree-not-in-this-repo)
13. [Related paths quick index](#related-paths-quick-index)

---

## Debug decision tree

Work top to bottom. Each step names the firmware layer and the doc section to
open next.

```
Flashed image?
  |
  +-- Unknown / wrong target
  |     --> Confirm: cargo xtask dabao | dabao-ccid | dabao-ccid openpgp-apdu
  |                  | baosec | baosec-ccid | ccid-hil
  |         (see CCID_TEST_REPORT.md "Image targets")
  |         dabao / baosec = no CCID
  |         dabao-ccid / baosec-ccid / ccid-hil = CCID transport enabled
  |         dabao-ccid openpgp-apdu = 8-process image; currently does NOT enum
  |         Hardware-confirmed CCID enum: Dabao dabao-ccid (1d50:6197, HS bulk MPS 512)
  |         Archives: images/dabao-ccid/known-good/ vs images/dabao-ccid/openpgp-apdu/
  |
  +-- Known target --> Host: lsusb -d 1d50:  (6196=boot1, 6197=dabao kernel, 6198=baosec)
        |
        +-- 1d50:6196 only (BAOCHIP + ttyACM)
        |     --> Still in boot1. Copy all three UF2s, sync, send `boot`
        |         at 1 000 000 8N1 on ttyACM (PROG alone may not leave bootwait).
        |
        +-- NO LINE (device not visible at all)
        |     --> Discriminate BEFORE treating as base USB:
        |         1. dmesg error -32 / -71 then 1d50:6197 a few seconds later
        |            = expected SE0 gap (boot1 drop, usb-bao1x attach). SUCCESS.
        |         2. Flashed images/dabao-ccid/openpgp-apdu/ or
        |            cargo xtask dabao-ccid openpgp-apdu --no-verify
        |            = 8-process boot failure. UART on PB13/PB14 1M 8N1.
        |            See OPENPGP_APDU_BOOT_DEBUG.md. Do not debug CCID framing.
        |         3. Confirm known-good: images/dabao-ccid/known-good/ or
        |            cargo xtask dabao-ccid --no-verify (7 processes). Must get 6197.
        |         4. If known-good also never appears: pre-CCID (power, cable,
        |            incomplete UF2 MSC write — serial uf2send.py). Stock dabao
        |            as last baseline.
        |         After kernel boot there is NO ttyACM (Persona A). Do not wait
        |         for CDC. Success is lsusb 1d50:6197 only.
        |         Files: main.rs boot, hw.rs init/poll, driver.rs handle_event_inner;
        |                services/openpgp-apdu/ if the 8-process image was used
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
                    --> XfrBlock needs a deferred handler (GetSlotStatus / IccPowerOn
                        ATR are inline). In-tree: services/openpgp-apdu (does not
                        currently reach this stage — image never enums).
                        Out-of-tree stub also valid. Host: add 1D50:6197 to libccid
                        Info.plist. See CCID_TEST_REPORT.md / protocol handler skeleton.
```

**Minimum report** (if still stuck after the tree): flashed `xtask` target or
which `images/dabao-ccid/` folder, output of `lsusb -d 1d50:`, whether
`dabao-ccid` known-good enumerates as `6197` on the same board, and whether
dmesg showed `-32`/`-71` then `6197` or never returned. No firmware source
paste required if this table was followed.

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
  +-- setup_usb_pins + SE0 Low (500 ms) → cu.init() → delay 150 ms → SE0 High
  |
  v
main loop: reply_and_receive_next only (IPC). USB runs in IRQ.

Corigine hardware IRQ
  |
  v
composite_handler (hw.rs)
  |
  +-- if CORIGINE_IRQ (independent of SW; not else-if)
  |     handle_event_inner (libs/bao1x-hal/.../driver.rs)
  |       Port reset, cable connect, EP0 setup/data, bulk completion
  |     device.poll(&mut [classes...])  (usb-device crate)
  |       GET_DESCRIPTOR, SET_ADDRESS, SET_CONFIGURATION
  |       State becomes UsbDeviceState::Configured
  |     Class I/O in same IRQ (HID; optional serial; CCID bulk)
  |
  +-- if SW_IRQ (both bits can be set in one invocation)
        FidoTx / KbdTx / CcidTx; CcidTx sets irq_serviced after poll_bulk_in
```

| Stage | File | Symbol / area |
|-------|------|----------------|
| Gadget assembly | `hw.rs` | `Bao1xUsb::new`, `EpBudgetLedger`, class list |
| Controller start | `hw.rs` | `Bao1xUsb::init` |
| SE0 / attach | `main.rs` | `setup_usb_pins`, Low → 500 ms → `cu.init()` → 150 ms → High |
| IRQ entry | `hw.rs` | `composite_handler` |
| Low-level events | `libs/bao1x-hal/src/usb/driver.rs` | `handle_event_inner` |
| Bus poll adapter | `libs/bao1x-hal/src/usb/driver.rs` | `CorigineWrapper::poll`, `set_device_address`, `reset` |
| Configured gate | `main.rs` | `cu.device.state() != Configured` on U2fTx, CcidTx, etc. |
| Forced re-enumerate | `hw.rs` / `main.rs` | `Bao1xUsb::unplug`, PMIC `VbusIrq::Remove` |

**Endpoint budget:** Corigine `CRG_EP_NUM = 8`. Persona A (`ccid-openpgp`):
CCID(2)+FIDO(2)+NKRO(2)=**6/8** (interrupt IN omitted); debug and provisioning
CDC are never allocated. Stock `baosec`: FIDO+NKRO+debug CDC = **7/8**. Debug on
CCID images uses `xous-log` UART/DUART (`services/xous-log/.../bao1x`).

**Guard:** `ep_budget::EpBudgetLedger` tracks the **cumulative** reserved total
across classes (not independent subtotals). Each `reserve_before_alloc` runs
before that class's `alloc.*` calls; after all classes,
`assert_matches_live(cw.allocated_non_ep0_count())` checks the shared counter
updated inside `CorigineWrapper::alloc_ep`. Per-class `assert_class_ep_budget`
remains. Regression: `ep_budget` tests + `tools/test_ep_budget_cumulative.py`
(fake class on a full stack must trip cumulative; independent checks would not).

**Service boot order (CCID images):** `ccid-openpgp` does **not** call
`Pddb::new()` (that blocked USB bring-up). Optional `ccid_store` lives behind
`ccid-pddb` and is not enabled by any xtask image. SE0 sequencing matches
boot1 (`setup_usb_pins` → Low → 500 ms → `cu.init()` → 150 ms → High);
`Keyboard::new()` is deferred until after SE0 High (KPC / SFR_IOX conflict on PF5).

---

## Runtime data flow (CCID layer)

```
USB IRQ (hw.rs: composite_handler)
  |
  +-- bulk OUT --> ccid_transport.rs: endpoint_out
  |                 append_bulk_out / drain_complete_messages (ccid_framing.rs)
  |                 |
  |                 +-- GetSlotStatus (0x65) --> inline RDR_to_PC_SlotStatus
  |                 |                         + poll_bulk_in (100 ms libccid window)
  |                 |
  |                 +-- IccPowerOn (0x62) --> inline RDR_to_PC_DataBlock + OPENPGP_ATR
  |                 |
  |                 +-- other frames --> ccid_rx queue (VecDeque<Vec<u8>>)
  |                                     --> IrqCcidRx scalar to usb-bao1x main loop
  |
  +-- bulk IN  <-- ccid_transport.rs: poll_bulk_in / enqueue_response
                    next_tx_chunk (ccid_framing.rs)

Base stack (driver.rs)
  set_device_address --> ep_enable loop --> prime bulk OUT receive TRB
  (ep_out_ready cleared if app buffer unavailable)

main.rs message loop
  CcidRxDeferred  --> park listener; re-check queue after park (TOCTOU)
  IrqCcidRx       --> deliver frame, or Denied if parked but queue empty
  CcidTx          --> ccid.enqueue_response + soft IRQ; wait irq_serviced
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
| [`services/usb-bao1x/src/ep_budget.rs`](../services/usb-bao1x/src/ep_budget.rs) | Cumulative EP ledger; regression: fake class on **6/8** |
| [`services/usb-bao1x/src/ccid_framing.rs`](../services/usb-bao1x/src/ccid_framing.rs) | Wire math: `CCID_WIRE_MAX` (271), `CCID_BULK_MAX_PACKET` (**512** HS), `append_bulk_out`, `drain_complete_frames`, `next_tx_chunk`, `is_get_slot_status`, `is_icc_power_on`, `rdr_to_pc_slot_status_ok`, `rdr_to_pc_data_block_atr`; **unit tests (9/9)** |
| [`services/usb-bao1x/src/ccid_transport.rs`](../services/usb-bao1x/src/ccid_transport.rs) | USB class 0x0B descriptors, bulk OUT assembly, bulk IN chunking (512-byte packets), inline GetSlotStatus + IccPowerOn ATR, `enqueue_response` / `prime_bulk_out` / `force_prime_bulk_out` |
| [`services/usb-bao1x/src/ccid_store.rs`](../services/usb-bao1x/src/ccid_store.rs) | PDDB dict `usb.ccid`; compiled only with `ccid-pddb`; not used at boot |
| [`services/usb-bao1x/src/hw.rs`](../services/usb-bao1x/src/hw.rs) | Composite gadget, EP budget assert, `device.poll`; `composite_handler` independent CORIGINE + SW IRQ branches |
| [`services/usb-bao1x/src/main.rs`](../services/usb-bao1x/src/main.rs) | Boot, SE0 Low/High timing, IPC loop, `CcidRxDeferred` TOCTOU re-check, `CcidTx` `irq_serviced` wait; serial opcodes gated off on CCID |
| [`services/usb-bao1x/src/lib.rs`](../services/usb-bao1x/src/lib.rs) | Public `ccid_framing` module; U2F client API (template for handler IPC) |
| [`services/openpgp-apdu/`](../services/openpgp-apdu/) | In-tree deferred APDU harness (SELECT / GET DATA / VERIFY fixtures). **8-process dabao-ccid image does not enumerate**; UART needed — [`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md) |
| [`xtask/src/main.rs`](../xtask/src/main.rs) | `dabao` / `baosec` = no CCID; `dabao-ccid` / `baosec-ccid` add `ccid-openpgp`; `ccid-hil` adds echo + `oem-baosec-lite`; positional `openpgp-apdu` or `--with-openpgp-test-apdu` |

---

## Key functions by concern

| Concern | Location |
|---------|----------|
| Enumeration (control plane) | `driver.rs` `handle_event_inner`; `hw.rs` `composite_handler` + `device.poll` |
| CORIGINE + SW IRQ in one invocation | `hw.rs` `composite_handler` — separate `if` on each mask (not `else if`) |
| SE0 release for host attach | `main.rs` boot1-matching Low (500 ms) → `cu.init()` → 150 ms → High |
| SET_ADDRESS / EP enable / bulk OUT prime | `driver.rs` `set_device_address` — after `ep_enable` loop, queue first bulk OUT receive TRB; clear `ep_out_ready` if app buffer missing |
| Bus reset handling | `driver.rs` `EventPortStatusChange`; `hw.rs` reset branch in `composite_handler` |
| Endpoint allocation limit | `driver.rs` `CRG_EP_NUM` + `allocated_non_ep0`; `ep_budget::EpBudgetLedger` (cumulative) |
| Poll class list | `hw.rs` `composite_handler` — HID+CCID (Persona A) or HID+debug CDC (stock) |
| CCID descriptor bytes | `ccid_transport.rs` — `ccid_class_descriptor_bytes`, `get_configuration_descriptors` |
| Reject oversize host frames | `ccid_framing.rs` — `append_bulk_out` returns `Overflow`, clears buffer |
| GetSlotStatus inline (100 ms) | `ccid_transport.rs` `drain_complete_messages` + `ccid_framing::{is_get_slot_status,rdr_to_pc_slot_status_ok}` |
| IccPowerOn inline ATR | `ccid_transport.rs` + `ccid_framing::{is_icc_power_on,rdr_to_pc_data_block_atr,OPENPGP_ATR}` |
| Frame ready notification | `ccid_transport.rs` — `drain_complete_messages` sends `IrqCcidRx` for non-inline frames |
| Handler receives frame | `main.rs` — `Opcode::CcidRxDeferred` (TOCTOU re-check after park), `Opcode::IrqCcidRx` (Denied if parked + empty) |
| Handler sends reply | `main.rs` — `Opcode::CcidTx` calls `enqueue_response` + soft IRQ; waits `irq_serviced` |
| In-tree APDU harness | `services/openpgp-apdu/src/main.rs` `ccid_main`; `usb_link.rs` `CcidLink::connect_to_usb_driver` (`"_Xous USB device driver_"`) |
| HIL echo (non-production) | `main.rs` — `#[cfg(feature = "ccid-echo")]` inside `IrqCcidRx` |
| Second listener rejected | `main.rs` — `CcidRxDeferred` sets `CcidCode::Denied` for other PIDs |
| Already provisioned? | `ccid_store.rs` — only with `ccid-pddb`; **not** called from `main.rs` at boot |
| Offline PIN seed helper | `ccid_store.rs` — `save_provisioned_pins` (not USB-wired; feature `ccid-pddb`) |
| PMIC unplug reset | `main.rs` — `Opcode::PmicIrq`, `cu.unplug()` |

---

## Symptom to code (self-service debug)

| Symptom | First places to inspect |
|---------|-------------------------|
| **Nothing enumerates** (`lsusb` empty for `1d50:6197` / `6198`) | **First:** which image? `openpgp-apdu` 8-process never enums — UART, not framing. **Known-good** `dabao-ccid` (7 processes) must show `6197`. dmesg `-32`/`-71` then `6197` is the SE0 gap, not failure. **If known-good also missing:** `main.rs` boot / SE0, incomplete MSC UF2 (use `bao1x-boot/uf2send.py`), cable. **Baseline:** stock `dabao` / `baosec`. |
| Stuck in **boot1** `1d50:6196` | Send `boot` on ttyACM at 1M 8N1; `bootwait` may ignore PROG. Copy `loader.uf2` + `xous.uf2` + `apps.uf2` then `sync`. |
| Kernel up but **no ttyACM** | Expected on `dabao-ccid` (Persona A). Success is `1d50:6197` only. UART is PB13/PB14. |
| **CCID image breaks enumeration; stock works** | Endpoint budget: Persona A must stay at CCID+FIDO+NKRO (**6/8**, no interrupt IN). Check `EpBudgetLedger` / accidental CDC add. If the CCID image included `openpgp-apdu`, see [`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md) first. |
| Device visible but **stuck at "new full-speed USB"** / never configured | `driver.rs` EP0 / `set_device_address`; `composite_handler` double-lock (`double_lock_detected` in main loop). Host `dmesg` for STALL. Brief FS `-32` then HS `6197` is normal. |
| **pcscd WriteUSB timeout / RFAddReader fails** | Bulk OUT not primed: `driver.rs` `set_device_address` after `ep_enable`; also `ccid.prime_bulk_out` on first Configured / LinkStatus. |
| **pcscd CreateChannel / ReadUSB ~100 ms timeout** | GetSlotStatus must be inline: `ccid_transport` `drain_complete_messages` + framing helpers (do not rely on stub for 0x65). |
| **pcscd ATR timeout / card not present** | IccPowerOn must be inline: `is_icc_power_on` + `rdr_to_pc_data_block_atr` (`OPENPGP_ATR`). |
| **Double lock** log in main loop | `hw.rs` `composite_handler` `try_lock` failure path |
| No CCID interface in `lsusb -v` | Built stock `dabao` / `baosec` (no CCID) instead of `dabao-ccid` / `baosec-ccid` / `ccid-hil`; missing `ccid-openpgp` feature |
| `echo mismatch` / smoke test fail | Image has `ccid-echo`? `main.rs` `IrqCcidRx` echo branch; host timing (`ccid_smoke.py`) |
| Handler never receives frames | Handler on `_Xous USB device driver_`? `CcidRxDeferred` + `RxWait`; production must **not** use `ccid-echo`; GetSlotStatus (0x65) and IccPowerOn (0x62) are answered inline. In-tree listener: `openpgp-apdu` (only after 8-process boot is fixed). |
| `CcidCode::Denied` on receive | Only one listener PID; `main.rs` `CcidRxDeferred` |
| `CcidCode::Hangup` on send | USB not configured; `main.rs` `CcidTx` checks `UsbDeviceState::Configured` |
| Partial / truncated CCID frames | `ccid_framing.rs` `drain_complete_frames`; host sending before configured |
| Oversize frame / silent drop | `append_bulk_out` overflow in `ccid_transport.rs` `endpoint_out` |
| Bulk IN stuck / no reply | `poll_bulk_in`, `tx_pending` in `ccid_transport.rs`; handler called `CcidTx`? soft IRQ armed? |
| No USB CDC / no provision port on CCID | Expected (Persona A); use UART (`xous-log`) for debug |
| PDDB not OKV1 on CCID image | Expected when `ccid-pddb` unused; seed PDDB offline — no USB provision |
| PDDB keys wrong / missing | `ccid_store.rs`; PDDB basis policy (out of tree) |
| `test_provision.py` fails | CDC present (Persona A regression) or pyusb/permissions; not “missing provision port” |
| `test_provision.py` PASS | Confirms no CDC — does **not** prove PDDB OKV1 |
| Board compile error in CI | `ccid-ci.yml`; `RefCell`/`borrow` in `hw.rs` / `main.rs` |
| Fork CI `Can't sign swap image` | `.github/workflows/build.yml` upstream tag fetch |
| Unit test regression | `ccid_framing.rs` `mod tests`; `cargo test -p usb-bao1x --lib ccid_framing` (**9/9**); `cargo test -p usb-bao1x --lib ep_budget`; `cargo test -p openpgp-apdu --lib` |

---

## Host checks (copy-paste)

Run on the machine with the device plugged in. Interpret via
[Debug decision tree](#debug-decision-tree).

```bash
# Step 1 — boot1 vs kernel vs gone
lsusb -d 1d50:
# 6196 = boot1 (BAOCHIP + ttyACM). Send: printf 'boot\r\n' > /dev/ttyACM0
# 6197 = dabao-ccid kernel (hardware-confirmed). No ttyACM after this.
# 6198 = baosec. empty = SE0 gap, openpgp-apdu image, or incomplete flash.

# Step 2 — interfaces (stock: HID+CDC; ccid images: HID+CCID 0x0B, no CDC)
# Expect CCID bulk wMaxPacketSize 0x0200 (512) on high-speed
lsusb -d 1d50:6197 -v 2>/dev/null | grep -E 'bInterfaceClass|iInterface|idProduct|wMaxPacketSize'

# Step 3 — kernel view (stall, reset loops)
# -32 / -71 during attach then 6197 = expected; never 6197 = fail
dmesg -T | tail -40

# Step 4 — CCID transport only (ccid-hil or *-ccid + ccid-echo image)
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197
```

Expected `idProduct`: dabao `0x6197`, baosec `0x6198` (`hw.rs` `UsbVidPid(0x1d50, pid)`).
Confirmed on hardware: Dabao `dabao-ccid` (7-process, no `openpgp-apdu`), HS 480 Mbps,
CCID bulk MPS 512; `pcsc_scan` reader + ATR + OpenPGP Card V2 (with stub). Petrn UART
on known-good: 7 processes, `usb-bao1x` PID 6, then host `1d50:6197`. See
[`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md).

Flash archives (in git): `images/dabao-ccid/known-good/` and
`images/dabao-ccid/openpgp-apdu/` (`loader.uf2`, `xous.uf2`, `apps.uf2`).
`xtask` still overwrites `target/.../release/`. If MSC copy is unreliable,
`python3 bao1x-boot/uf2send.py <file.uf2>` from boot1.

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
| [`.github/workflows/ccid-ci.yml`](../.github/workflows/ccid-ci.yml) | `ccid_framing` + `ep_budget` unit tests + hosted/board check + `baosec-ccid` + `ccid-hil` compile |
| [`.github/workflows/build.yml`](../.github/workflows/build.yml) | Full `cargo xtask baosec` matrix (default image, no CCID) |
| [`.github/workflows/ccid-hil.yml`](../.github/workflows/ccid-hil.yml) | Self-hosted `tools/ccid_hil/run_all.sh` (scaffolding; no runner yet) |

Local equivalents:

```bash
cargo test -p usb-bao1x --lib ccid_framing
cargo test -p usb-bao1x --lib ep_budget
cargo test -p openpgp-apdu --lib
cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp
cargo check -p usb-bao1x -p modals --features board-baosec,ccid-openpgp,bao1x --target riscv32imac-unknown-xous-elf
cargo xtask dabao-ccid --no-verify                 # 7-process; hardware-confirmed 6197
cargo xtask dabao-ccid openpgp-apdu --no-verify    # 8-process; currently no USB enum
cargo xtask baosec --no-verify                     # baseline USB (no CCID)
cargo xtask baosec-ccid --no-verify                # baosec CCID transport
cargo xtask ccid-hil --no-verify                   # CCID + echo for bench
python3 tools/ccid_smoke.py --vid 0x1d50 --pid 0x6197
```

---

## Compile-time feature branches

When reading `main.rs`, note `cfg` gates:

| `cfg` | Effect |
|-------|--------|
| `feature = "ccid-openpgp"` | CCID+FIDO+NKRO; no USB CDC; no boot PDDB check |
| `feature = "ccid-pddb"` | Compiles `ccid_store` only; not enabled by any xtask image |
| `feature = "ccid-echo"` | `IrqCcidRx` echoes frames; **disables** `CcidRxDeferred` handler path |
| `not(feature = "ccid-echo")` | Production path: deferred listener + `CcidRxDeferred` opcodes |
| `target_os = "xous"` | `ccid_transport` / `ccid_store` are device-only modules |

---

## In-tree openpgp-apdu handler

Minimal deferred CCID APDU test harness (`services/openpgp-apdu/`). Built into
dabao-ccid with:

```sh
cargo xtask dabao-ccid openpgp-apdu --no-verify
# or: cargo xtask dabao-ccid --with-openpgp-test-apdu --no-verify
```

| Item | Status |
|------|--------|
| Known-good (7 processes, no handler) | Enumerates `1d50:6197`. Archive: `images/dabao-ccid/known-good/` |
| With `openpgp-apdu` (8 processes, PID 8) | **Does not enumerate.** Archive: `images/dabao-ccid/openpgp-apdu/` |
| Deferred-path fixes in `usb-bao1x` | Present in both images; known-good still enums, so not the 8-process cause |
| Hosted unit tests | `cargo test -p openpgp-apdu --lib` |

UART on **PB13/PB14, 1M 8N1** during an openpgp-apdu boot. Look for
`"openpgp-apdu starting (PID 8)"`, `"USB driver connect failed"`, and
`usb-bao1x` / panic lines. Full procedure:
[`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md).

| File | Role |
|------|------|
| `src/main.rs` | `ccid_main`: log init (no panic), connect retry, APDU loop |
| `src/usb_link.rs` | `CcidLink` to `"_Xous USB device driver_"` opcodes 640 / 642 |
| `src/apdu/` | Parse / dispatch SELECT, GET DATA, VERIFY, GET RESPONSE |
| `src/ccid/` | `PC_to_RDR` / `RDR_to_PC` helpers |
| `src/openpgp/` | Fixture card, AID, DOs |

---

## Out-of-tree (not in this repo)

| Component | Responsibility |
|-----------|----------------|
| In-tree `openpgp-apdu` | Test harness in this repo (see section above). Not a production OpenPGP card. |
| Production OpenPGP / crypto service | Full APDU/T=1, keys, `CcidRxDeferred` / `CcidTx` client (still out of tree) |
| Factory tooling | Seeds PDDB (`usb.ccid` / `OKV1`) offline before CCID image flash |
| `pcscd` / GnuPG on host | End-user smart-card access; add `1D50:6197` to libccid `Info.plist` |

Handler authors: copy the
[Handler skeleton (Rust)](CCID_PROTOCOL_AND_HIL.md#handler-skeleton-rust) in the
protocol doc, or start from `services/openpgp-apdu/`, and wire the process into
the product's Xous service table (`xtask` positional cratespec).

---

## Related paths quick index

| Path | Purpose |
|------|---------|
| `libs/bao1x-hal/src/usb/driver.rs` | Corigine UDC, enumeration events, EP0, `set_device_address` bulk OUT prime |
| `services/usb-bao1x/src/hw.rs` | Composite gadget, IRQ handler (independent CORIGINE + SW), `device.poll` |
| `services/usb-bao1x/src/main.rs` | Boot, SE0, IPC loop, `CcidRxDeferred` TOCTOU, configured gates |
| `services/usb-bao1x/src/ccid_transport.rs` | USB CCID class driver |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire format helpers + unit tests |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB provisioning storage |
| `services/usb-bao1x/src/api.rs` | IPC opcodes and `CcidMsgIpc`; `"_Xous USB device driver_"` |
| `services/openpgp-apdu/` | In-tree deferred APDU harness (8-process image does not enum) |
| `images/dabao-ccid/known-good/` | Flash set that enumerates `1d50:6197` |
| `images/dabao-ccid/openpgp-apdu/` | Flash set that drops off USB |
| `xtask/src/main.rs` | Image targets and service order; `dabao-ccid openpgp-apdu` |
| `docs/OPENPGP_APDU_BOOT_DEBUG.md` | UART procedure for the 8-process boot failure |
| `tools/ccid_smoke.py` | Host smoke test |
| `tools/ccid_hil/` | HIL scripts and suite |
| `bao1x-boot/uf2send.py` | Serial UF2 when MSC copy is unreliable |
| `.github/workflows/ccid-ci.yml` | CI compile + `ccid_framing` / `ep_budget` unit tests |
| `.github/workflows/ccid-hil.yml` | Nightly Pi HIL (scaffolding) |
