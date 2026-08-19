<!--
SPDX-License-Identifier: Apache-2.0
-->

# openpgp-apdu code map (boot investigation)

Navigable reference for the 8-process `dabao-ccid openpgp-apdu` boot failure
and the CCID transport fixes around it.

**Branch:** [`feature/usb-bao1x-ccid-openpgp`](https://github.com/betrusted-io/xous-core/tree/feature/usb-bao1x-ccid-openpgp)
**HEAD snapshot:** `6baceee6d` (`fix: ccid-hil compile, rustfmt openpgp-apdu, refresh dabao-ccid images`)
**Line numbers:** taken from that HEAD. Re-verify before treating them as exact after later commits.

Related docs:

- [`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md) — UART procedure and discrimination test
- [`code_map.md`](code_map.md) — symptom-to-source USB/CCID map
- [`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md) — hardware-confirmed 7-process results
- [`CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md) — protocol and handler API

**This document is read-only investigation notes. It does not change firmware.**

---

## Table of contents

1. [Bugs fixed (with exact references)](#1-bugs-fixed-with-exact-references)
2. [Bugs likely unfixed (investigation needed)](#2-bugs-likely-unfixed-investigation-needed)
3. [openpgp-apdu service](#3-openpgp-apdu-service)
4. [Boot sequence (PID order)](#4-boot-sequence-pid-order)
5. [Three key files with fixes](#5-three-key-files-with-fixes)
6. [xtask integration](#6-xtask-integration)
7. [IPC handoff: openpgp-apdu and usb-bao1x](#7-ipc-handoff-openpgp-apdu--usb-bao1x)
8. [Known-good image reference](#8-known-good-image-reference)
9. [Investigation status and next steps](#9-investigation-status-and-next-steps)
10. [Discrepancies vs. earlier investigation notes](#10-discrepancies-vs-earlier-investigation-notes)

---

## 1. Bugs fixed (with exact references)

### Bug 1: Blocking `log::info!` in IRQ context

**Status:** NOT FOUND AS DESCRIBED (related IRQ-path logging is absent; a different IRQ-safety change lives at these functions)

**File:** [`libs/bao1x-hal/src/usb/driver.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs)

**Claimed symptom:** `log::info!()` in `enable_interrupts()` / `disable_interrupts()` resolves to blocking IPC (`xous::send_message(...Message::Borrow...).unwrap()`) when called from IRQ context, stalling or panicking `usb-bao1x`.

**What git actually contains:**

`git log -S 'log::info' -- libs/bao1x-hal/src/usb/driver.rs` is empty. Parent of
[`f265ee346`](https://github.com/betrusted-io/xous-core/commit/f265ee34650af247f89c34e514987a1a5fcd0d2f)
had no logging in these functions:

```rust
pub fn disable_interrupts(&self) { self.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 0); }

pub fn enable_interrupts(&self) {
    self.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
    self.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 3);
}
```

**Current code (no `log::info!`):**

- [`disable_interrupts()`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs#L2832) at **lines 2832–2844** — saves `EV_ENABLE`, optionally samples `EV_PENDING` into atomics under `irq-pending-trace`, writes `EV_ENABLE = 0`. No log macros.
- [`restore_interrupts()`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs#L2854) at **lines 2854–2864** — restores previous `EV_ENABLE`. Does **not** write `EV_PENDING`.
- [`enable_interrupts_clear_pending()`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs#L2871) at **lines 2871–2883** — reset/init only; still blanket-clears `EV_PENDING`.

`UsbBus::write()` (non-EP0) calls `disable_interrupts` / `restore_interrupts` at
**lines 3234–3245 and 3283**, not `enable_interrupts_clear_pending`.

**Nearby `log::info!` that is not IRQ context:**
[`services/usb-bao1x/src/main.rs` lines 669–674](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L669)
(`Opcode::CcidPrimeBulkOut`, main loop, `irq-pending-trace` only).

IRQ path uses `crate::println!` (DUART), e.g. double-lock in
[`hw.rs` line 433](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs#L433).

**Fix:** Logging was never present in these two functions in committed history. The real change at this site is Bug 2 (do not clear `EV_PENDING` on the write restore path).

**Confirmed by:** static history search only. No UART proof of a `log::info` IRQ panic was found in-tree.

---

### Bug 2: Lost-wakeup via blanket `EV_PENDING` clear

**Status:** FIXED on the `UsbBus::write()` path. **Not** changed in `composite_handler`'s IRQ ack.

**Primary file (actual location):** [`libs/bao1x-hal/src/usb/driver.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs)
**Commit:** [`f265ee346`](https://github.com/betrusted-io/xous-core/commit/f265ee34650af247f89c34e514987a1a5fcd0d2f) — *stop synthetic OUT retire and IRQ enable clear-on-write*

**Symptom:** `enable_interrupts()` used to write `EV_PENDING = 0xFFFF_FFFF` after a short IRQ mask around `UsbBus::write()`. An IRQ that latched while masked was discarded. That is a lost wakeup (bulk IN/OUT completion or SW IRQ never runs).

**Before** (`enable_interrupts` on the write restore path):

```rust
pub fn enable_interrupts(&self) {
    self.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
    self.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 3);
}
```

**After** (current **lines 2832–2883**): mask saves `EV_ENABLE`; restore writes only `EV_ENABLE`; blanket pending-clear is renamed and documented as reset/init only.

**`hw.rs` still blanket-clears after sampling** — this is the IRQ handler ack, not the write-path bug:

```391:403:services/usb-bao1x/src/hw.rs
    let pending = usb.irq_csr.r(utra::irqarray1::EV_PENDING);
    // ...
    // clear pending
    usb.irq_csr.wo(utra::irqarray1::EV_PENDING, 0xffff_ffff);
    // re-enable interrupts
    usb.irq_csr.wo(utra::irqarray1::EV_ENABLE, CORIGINE_IRQ_MASK | SW_IRQ_MASK);
```

Init still clears pending at
[`hw.rs` lines 246–247](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs#L246)
(first enable after `start()`). That is reset, not lost-wakeup around `write()`.

**Confirmed by:** commit message of `f265ee346`: *Confirmed on Dabao with 2392 GetSlotStatus iterations over 120s (out_depth 0).* Known-good 7-process `dabao-ccid` enumerates as `1d50:6197` (see Section 8).

---

### Bug 3: Synthetic OUT retire in `force_prime_bulk_out`

**Status:** FIXED (current code never calls `retire_app_buf_ptr` from force-prime)

**File:** [`libs/bao1x-hal/src/usb/driver.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs)
**Function:** [`force_prime_bulk_out`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs#L2898) **lines 2898–2953**
**Callers in `usb-bao1x`:** [`main.rs` 659, 777](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L659); [`ccid_transport.rs` 228–229](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_transport.rs#L228)

**Symptom:** A force-prime path retired any outstanding OUT slot when `enq != deq` with no hardware-completion check, advancing software `deq` while bypassing `out_trb_consumed`. Keepalive / GetSlotStatus then failed (no live OUT TRB, or data-integrity hazard).

**Git note:** `force_prime_bulk_out` first appears in `f265ee346` **already without** `retire_app_buf_ptr`. There is no committed parent that still has the synthetic retire. The removed pattern is recorded in comments at **lines 2920–2926** and flight-src 13 at **line 2638**.

**Current gate (no retire):**

```2915:2940:libs/bao1x-hal/src/usb/driver.rs
        self.ep_out_ready[ep].store(false, Ordering::SeqCst);

        let len = CRG_UDC_APP_BUF_LEN.min(max_packet_size);
        let mut hw = self.core();
        let pei = CorigineUsb::pei(ep as u8, CRG_OUT);
        // Do NOT call retire_app_buf_ptr here. ...
        if hw.app_enq_index[pei] != hw.app_deq_index[pei] {
            #[cfg(feature = "irq-pending-trace")]
            {
                // ... Reuses src 13 so post-fix dumps show the
                // gate firing without index mutation ...
            }
            drop(hw);
            // Keep ready false so a later genuine read / idle prime can proceed.
            return;
        }
```

Genuine retire remains only on completion paths, e.g.
[`retire_app_buf_ptr` at line 1153](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/libs/bao1x-hal/src/usb/driver.rs#L1153)
and `UsbBus::read` / IN complete (around **3345** and **3633**).

**Confirmed by:** same `f265ee346` Dabao run (2392 GetSlotStatus / 120 s, `out_depth 0`). This was the keepalive root cause on the 7-process image.

---

### Deferred-path structural fixes (also in tree)

These are distinct from Bugs 1–3 above. They were applied in
[`f2c6dfbb8`](https://github.com/betrusted-io/xous-core/commit/f2c6dfbb851c457a91a909ed4841914dad6668cf)
and are present at HEAD. Discrimination test: 7-process `dabao-ccid` still enumerates, so they are not the cause of missing USB when `openpgp-apdu` is added.

| Fix | File | Lines | Status |
|-----|------|-------|--------|
| Independent CORIGINE + SW IRQ (`else if` removed) | [`hw.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs) | 405 and 556 | FIXED |
| `irq_serviced` set after bulk IN **submission** | [`ccid_transport.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_transport.rs) | 357–370 | FIXED |
| CcidRxDeferred TOCTOU re-check after park | [`main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs) | 610–632 | FIXED |
| Edition-2021 RefCell pop-into-local | [`main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs) | 600, 715 | FIX IN CODE, UART UNVERIFIED |

Details in Sections 2 and 5.

---

## 2. Bugs likely unfixed (investigation needed)

### Outstanding: RefCell double-borrow in CcidRxDeferred

**Status:** FIX ATTEMPTED, AWAITING UART TEST

**File:** [`services/usb-bao1x/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs)

**Line-number note:** older notes pointed at **line 612** as the `if let Some(frame) = cu.ccid_rx.borrow_mut().pop_front()` site. At current HEAD that pattern is gone. The handler is **lines 590–641**. Line 612 is now `assert_eq!(buf.code, CcidCode::RxWait);` inside the TOCTOU re-check.

**Symptom:** On Edition 2021, `if let Some(frame) = cell.borrow_mut().pop_front()` keeps the `RefMut` alive through the `else` branch. The empty-queue path (normal startup wait) then panics with `RefCell already borrowed` if anything in `else` borrows `ccid_rx` again (`prime_bulk_out` → `drain_complete_messages` → `complete_rx.borrow_mut()`, or the TOCTOU `pop_front`).

**Original (before `f2c6dfbb8`):**

```rust
if let Some(frame) = cu.ccid_rx.borrow_mut().pop_front() {
    // fill CcidMsgIpc, RxAck
} else {
    ccid_listener = msg_opt.take();
    cu.ccid.prime_bulk_out();
}
```

**Fix applied (current lines 595–632):** pop into a local so `RefMut` drops before `else`:

```595:619:services/usb-bao1x/src/main.rs
                    // Pop in its own statement so the RefMut is dropped before the else
                    // branch. Edition 2021 keeps `if let` scrutinee temporaries alive for
                    // the whole if-else; a second borrow_mut() there panics ("RefCell
                    // already borrowed") the first time the queue is empty — which is the
                    // normal CcidRxDeferred wait at startup.
                    let queued = cu.ccid_rx.borrow_mut().pop_front();
                    if let Some(frame) = queued {
                        // ... RxAck ...
                    } else {
                        ccid_listener = msg_opt.take();
                        // ...
                        cu.ccid.prime_bulk_out();
                        // Re-check after park ...
                        if let Some(frame) = cu.ccid_rx.borrow_mut().pop_front() {
```

Same pattern in `Opcode::IrqCcidRx` at **lines 713–715**.

**Unit test:** [`ccid_framing.rs` lines 261–271](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_framing.rs#L261) `empty_queue_pop_drops_refmut_before_reborrow`.

**Status of fix:** compiled and built (`f2c6dfbb8` + `6baceee6d`). Awaiting UART capture (petrn) to confirm the panic is gone.

**Remaining risk even if RefCell panic is gone:**

1. **8-process image still does not enumerate** ([`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md)). Discrimination test: same `usb-bao1x` without `openpgp-apdu` enumerates. Failure may be earlier than CcidRxDeferred (usb-bao1x init hang/panic, names connect, kernel process load).
2. **`CcidTx` can spin forever** if `poll_bulk_in` returns before `signal_bulk_in_attempted()` (`tx_pending` false or no chunk) — wait at [`main.rs` 771–774](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L771).
3. **`force_prime_bulk_out` during SET_ADDRESS** is already gated: CcidRxDeferred uses `prime_bulk_out` (line 616); CcidPrimeBulkOut skips until Configured (658); openpgp-apdu waits for Configured before `receive_rx()` ([`openpgp-apdu/src/main.rs` 48–64](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/main.rs#L48)). If a future change force-primes too early, host descriptor timeout `-110` returns.
4. **`composite_handler` try_lock failure** still early-returns ([`hw.rs` 429–435](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs#L429)) and can drop events.

---

## 3. openpgp-apdu service

**Location:** [`services/openpgp-apdu/`](https://github.com/betrusted-io/xous-core/tree/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu)

**Purpose:** Minimal OpenPGP APDU handler **test harness**. Receives APDU frames via `CcidRxDeferred` IPC, parses and dispatches to command handlers, returns responses via `CcidTx`. Not a production HSM (VERIFY always OK; crypto INS return `6D00`; fixture card).

**Cargo:** [`services/openpgp-apdu/Cargo.toml`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/Cargo.toml) — edition 2021, crate `openpgp-apdu`, lib `openpgp_apdu`. Xous deps: `xous` 0.9.70, `xous-ipc` 0.10.10, `xous-names` 0.9.71, `log-server` 0.1.69, `ticktimer` 0.9.70, `rkyv` 0.8.8.

**Entry point:** [`services/openpgp-apdu/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/main.rs)

- Hosted stub: **lines 3–6**
- Xous `main` → `ccid_main`: **lines 20–23**

**Startup sequence (`ccid_main`, lines 23–64):**

1. Initialize logging without panic (**24–31**). On success, `log::info!("openpgp-apdu starting (PID {})", ...)`.
2. Connect to USB driver via `CcidLink::connect_to_usb_driver()` with retry + `yield_slice` (**33–43**).
3. Wait until `link.link_status() == UsbDeviceState::Configured` (**48–64**) — do not park `CcidRxDeferred` during SET_ADDRESS.
4. Enter main loop.

**Main loop (lines 66–135):**

1. Optional link-status log.
2. `link.receive_rx()` — blocking `lend_mut` on opcode 640.
3. Hangup (`ProcessTerminated`): clear card select/chunk state, continue.
4. Other errors: warn, `sleep_ms(50)` or yield, continue (does **not** spin the UART).
5. Parse PC_to_RDR:
   - `IccPowerOn` / `GetSlotStatus`: skip `CcidTx` (usb-bao1x already answered inline).
   - `IccPowerOff` / `Abort`: `RDR_to_PC_SlotStatus`.
   - `XfrBlock`: parse APDU, `dispatch_apdu`, wrap in `RDR_to_PC_DataBlock`.
   - Malformed: `cmd_not_supported` slot status.
6. `link.send_tx(tx_frame)` (opcode 642).

openpgp-apdu does **not** register a names-server entry. It is a client of `"_Xous USB device driver_"`.

**Key modules:**

| Path | Role |
|------|------|
| [`usb_link.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs) | IPC connection to usb-bao1x |
| [`ccid/mod.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/ccid/mod.rs) | CCID frame parse/build |
| [`apdu/`](https://github.com/betrusted-io/xous-core/tree/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/apdu) | Parse, status words, dispatch |
| [`apdu/dispatch.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/apdu/dispatch.rs) | SELECT / GET DATA / VERIFY / GET RESPONSE |
| [`openpgp/`](https://github.com/betrusted-io/xous-core/tree/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/openpgp) | Fixture card, AID, DOs |

---

## 4. Boot sequence (PID order)

Based on xtask `dabao-ccid` service list
([`xtask/src/main.rs` lines 809–835](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/xtask/src/main.rs#L809))
and each service's init. PID 1 is the kernel. Subsequent PIDs follow **image creation order** (`Builder::add_service` pushes onto `services` in [`xtask/src/builder.rs` line 499](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/xtask/src/builder.rs#L499)).

| PID | Service | File | Init / registration | Notes |
|-----|---------|------|---------------------|-------|
| 1 | Kernel | N/A | N/A | N/A |
| 2 | xous-ticktimer | [`services/xous-ticktimer/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/xous-ticktimer/src/main.rs) | PID log **26**; `create_server_with_address(b"ticktimer-server")` **43–44** | Timer; `init_wait` for log at **24** |
| 3 | keystore | [`services/keystore/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/keystore/src/main.rs) | PID log **10**; `register_name(SERVER_NAME_KEYS)` **14** | Crypto/provisioning; blocks on names + log |
| 4 | xous-log | [`services/xous-log/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/xous-log/src/main.rs) | PID print in `main` **342**; server in `reader_thread` **49** `create_server_with_address(b"xous-log-server ")` | Logging backend (UART). Listed **after** keystore in xtask; earlier PIDs `init_wait` until this exists |
| 5 | xous-names | [`services/xous-names/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/xous-names/src/main.rs) | PID log **350**; `create_server_with_address(b"xous-name-server")` **352–353**; `Opcode::Register` **368** | Name registry |
| 6 | usb-bao1x | [`services/usb-bao1x/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs) | PID log **87**; `register_name(SERVER_NAME_USB_DEVICE)` **90** | **CCID/HID USB driver** |
| 7 | bao1x-hal-service | [`services/bao1x-hal-service/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/bao1x-hal-service/src/main.rs) | PID log **137**; `register_name(SERVER_NAME_BAO1X_HAL)` **140** | Hardware abstraction (`_bao1x-SoC HAL_`) |
| 8 | openpgp-apdu | [`services/openpgp-apdu/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/main.rs) | PID log **30** (only if log init succeeded); connect **35** | **APDU handler; client of PID 6**. No names registration |

7-process known-good image stops at PID 7 (no openpgp-apdu).

**Critical timing:**

- PID 5 (xous-names) must be serving `Register` / `TryConnect` before PID 6 registers `"_Xous USB device driver_"` and before PID 8's `request_connection_blocking()`.
- PID 6 must finish `register_name` (**line 90**) before PID 8's blocking connect succeeds. USB enumeration itself happens later (`cu.init()`, SE0, composite).
- If PID 6 panics during init (map_memory, IFRAM, `Bao1xUsb::new`, `cu.init()`), PID 8 retries forever in `connect_to_usb_driver()` (**33–43**). USB never enumerates.
- PID 8 parking `CcidRxDeferred` does **not** by itself stop usb-bao1x from enumerating; a panic **inside** usb-bao1x on the first empty-queue wait would.

---

## 5. Three key files with fixes

### File 1: [`services/usb-bao1x/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs)

**CcidRxDeferred handler:** **lines 590–641**
(`#[cfg(all(feature = "ccid-openpgp", not(feature = "ccid-echo")))]`)

**Bug location (historical line 612):** the edition-2021 `if let` + `borrow_mut()` was at the start of the authorized-PID block. Current pop-into-local is **line 600**. Current **line 612** is `assert_eq!(buf.code, CcidCode::RxWait);` in the TOCTOU path.

**Original (before `f2c6dfbb8`):**

```rust
if let Some(frame) = cu.ccid_rx.borrow_mut().pop_front() {
    let mut response = unsafe {
        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
    };
    let mut buf = response.to_original::<CcidMsgIpc, _>().unwrap();
    assert_eq!(buf.code, CcidCode::RxWait, "Expected CcidCode::RxWait");
    buf.data = frame;
    buf.code = CcidCode::RxAck;
    response.replace(buf).unwrap();
} else {
    ccid_listener = msg_opt.take();
    cu.ccid.prime_bulk_out();
}
```

**Fixed (current):**

```595:633:services/usb-bao1x/src/main.rs
                    // Pop in its own statement so the RefMut is dropped before the else
                    // branch. ...
                    let queued = cu.ccid_rx.borrow_mut().pop_front();
                    if let Some(frame) = queued {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                        };
                        let mut buf = response.to_original::<CcidMsgIpc, _>().unwrap();
                        assert_eq!(buf.code, CcidCode::RxWait, "Expected CcidCode::RxWait");
                        buf.data = frame;
                        buf.code = CcidCode::RxAck;
                        response.replace(buf).unwrap();
                    } else {
                        ccid_listener = msg_opt.take();
                        cu.ccid.prime_bulk_out();
                        if let Some(frame) = cu.ccid_rx.borrow_mut().pop_front() {
                            if let Some(mut listener) = ccid_listener.take() {
                                // ... RxAck on parked listener ...
                            }
                        }
                    }
```

**Explanation:** Explicit `let queued = ...` drops `RefMut` at end of statement. `else` may borrow again (prime + TOCTOU pop) without panic.

**Related fixes in the same file:**

- **IrqCcidRx (689–749):** same pop-into-local at **715**; hangup on reset arg1 (**690–705**); empty-queue `Denied` to parked waiter (**736–747**).
- **CcidPrimeBulkOut (645–677):** `force_prime_bulk_out` only if `UsbDeviceState::Configured` (**658–659**). Avoids SET_ADDRESS race (`-110`).
- **CcidTx (751–780):** `enqueue_response` + `sw_irq(CcidTx)` + wait `irq_serviced` (**771–774**) + `force_prime_bulk_out` (**777**).
- **Sleep on InternalError:** **not in this file.** Backoff is in [`openpgp-apdu/src/main.rs` 81–89](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/main.rs#L81) (`sleep_ms(50)` on `receive_rx` errors).

---

### File 2: [`services/usb-bao1x/src/hw.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs)

**Bug location:** `composite_handler` IRQ class branching. Changed in `f2c6dfbb8` from `else if` to two independent `if`s.

**Before (`f265ee346` / `f2c6dfbb8^`, around old line 551):**

```rust
    if (pending & CORIGINE_IRQ_MASK) != 0 {
        // Corigine event handling ...
        if usb.csr.rf(IMAN_IE) != 0 {
            usb.csr.wo(IMAN, usb.csr.ms(IMAN_IE, 1) | usb.csr.ms(IMAN_IP, 1));
        }
    } else if (pending & SW_IRQ_MASK) != 0 {
        // SW IRQ handling (FidoTx / KbdTx / CcidTx)
    }
```

**After (current lines 405 and 556–588):**

```405:405:services/usb-bao1x/src/hw.rs
    if (pending & CORIGINE_IRQ_MASK) != 0 {
```

```556:585:services/usb-bao1x/src/hw.rs
    if (pending & SW_IRQ_MASK) != 0 {
        let composite = usb.class.borrow_mut();
        match usb.irq_req.take() {
            Some(UsbIrqReq::FidoTx) => { /* ... */ usb.irq_serviced.store(true, Ordering::SeqCst); }
            Some(UsbIrqReq::KbdTx) => { /* ... */ }
            #[cfg(feature = "ccid-openpgp")]
            Some(UsbIrqReq::CcidTx) => {
                // irq_serviced is set inside poll_bulk_in after bulk_in.write() is attempted.
                usb.ccid.poll();
            }
            None => (),
        }
    }
```

**Explanation:** Under sustained USB traffic, both CORIGINE (hardware) and SW (CcidTx from main loop) IRQ bits can be pending simultaneously. The old `else if` meant that if CORIGINE fired first, the SW branch never ran, so `irq_serviced` was never set, causing the main loop to spin forever waiting for a response that was never queued. The fix ensures both branches always execute in the same handler invocation, even when both bits are set.

**Not changed here:** blanket `EV_PENDING` clear at **line 401** (ack after snapshot at **391**). See Bug 2.

---

### File 3: [`services/usb-bao1x/src/ccid_transport.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_transport.rs)

**Bug location:** `irq_serviced` flag movement (IRQ completion vs bulk IN submission)

**Original (hw.rs CcidTx arm, `f2c6dfbb8^`):**

```rust
Some(UsbIrqReq::CcidTx) => {
    usb.ccid.poll();
    usb.irq_serviced.store(true, Ordering::SeqCst);
}
```

That ran at the **end** of the SW IRQ arm, after `poll()`. If `poll_bulk_in` returned early (`!tx_pending` or no chunk) **before** a write, the flag still became true (or, with the `else if` bug, the arm never ran and main waited forever). The intended timeout/spin fix is: signal **immediately after** `bulk_in.write()` is attempted, not "hardware transfer complete".

**Fixed pattern:**

- Attach: [`hw.rs` 190–192](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs#L190) `ccid.attach_irq_serviced(&irq_serviced)`
- Pointer + store: [`ccid_transport.rs` 165–166, 193–203](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_transport.rs#L165)
- Set after write attempt: **`poll_bulk_in` lines 357–370**

```357:370:services/usb-bao1x/src/ccid_transport.rs
    fn poll_bulk_in(&self) {
        let chunk: Vec<u8> = {
            let g = self.inner.borrow();
            if !g.tx_pending {
                return;
            }
            match next_tx_chunk(&g.tx_buf) {
                Some(c) => c.to_vec(),
                None => return,
            }
        };
        let write_result = self.bulk_in.write(&chunk);
        // CcidTx main waits on this: bulk IN submission was attempted, not transfer complete.
        self.signal_bulk_in_attempted();
```

- HW arm no longer stores the flag: [`hw.rs` 580–584](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/hw.rs#L580)

**Exact lines:**

- Original location: `hw.rs` CcidTx arm, `usb.irq_serviced.store(true, ...)` after `usb.ccid.poll()` (removed in `f2c6dfbb8`).
- New location: [`ccid_transport.rs` line 370](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_transport.rs#L370) `self.signal_bulk_in_attempted();`

**Explanation:** Originally, `irq_serviced` was set at the end of the `composite_handler` IRQ, waiting for the hardware to actually complete the transfer. This caused `main` to spin indefinitely if the hardware never signaled completion (e.g. due to a bug or stall). Moving the flag to `poll_bulk_in()`, right after `bulk_in.write()`, changes its meaning: it now represents "the driver accepted the bulk IN submission," not "the hardware finished it." This unblocks `main` immediately, preventing deadlocks during early boot where hardware state might be uncertain. The actual transfer completion still happens in hardware; the flag just indicates the submission succeeded. **Caveat:** early `return` at 360–365 never signals; `CcidTx` at `main.rs` 771–774 would spin. That is a remaining deferred-path risk, not exercised on the known-good inline-only image.

---

## 6. xtask integration

**File:** [`xtask/src/main.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/xtask/src/main.rs)

**How openpgp-apdu gets composed:**

`dabao-ccid` always adds the 6 RRAM services (ticktimer … bao1x-hal-service) plus `ccid-openpgp`. openpgp-apdu is **optional**:

1. **Positional cratespec** (before flags): `cargo xtask dabao-ccid openpgp-apdu --no-verify`
   - Help text: **lines 1076–1078**
   - Implementation: `get_cratespecs()` **1125–1136**, then **lines 832–835**:

```832:835:xtask/src/main.rs
            for svc in get_cratespecs() {
                let (name, region) = crate::builder::region_from_name(&svc, LoaderRegion::Flash);
                builder.add_service(name, region);
            }
```

2. **Flag:** `cargo xtask dabao-ccid --with-openpgp-test-apdu --no-verify`
   - **lines 826–828:**

```826:828:xtask/src/main.rs
            if std::env::args().any(|a| a == "--with-openpgp-test-apdu") {
                builder.add_service("openpgp-apdu", LoaderRegion::Flash);
            }
```

**Base 7-process list (always):** **lines 809–822**

```809:822:xtask/src/main.rs
            let bao_rram_pkgs =
                ["xous-ticktimer", "keystore", "xous-log", "xous-names", "usb-bao1x", "bao1x-hal-service"]
                    .to_vec();
            // ...
            for service in bao_rram_pkgs {
                builder.add_service(service, LoaderRegion::Flash);
            }
```

**Loader region:** Flash (same as usb-bao1x). `add_service` is [`xtask/src/builder.rs` 499–503](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/xtask/src/builder.rs#L499).

**PID assignment:** image creation order. usb-bao1x is the 5th userspace service (PID 6); openpgp-apdu is appended last (PID 8).

`--with-openpgp-test-apdu` plus positional `openpgp-apdu` would add the crate twice; do not combine them.

---

## 7. IPC handoff: openpgp-apdu ↔ usb-bao1x

**Server name:** `"_Xous USB device driver_"`

| Side | Constant | Line |
|------|----------|------|
| usb-bao1x | `SERVER_NAME_USB_DEVICE` | [`api.rs` line 4](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L4) |
| usb-bao1x register | `xns.register_name(...)` | [`main.rs` line 90](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L90) |
| openpgp-apdu | `SERVER_NAME_USB_DEVICE` | [`usb_link.rs` line 7](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L7) |

**Opcodes:**

| Opcode | Value | usb-bao1x definition | openpgp-apdu definition |
|--------|-------|----------------------|-------------------------|
| `CcidRxDeferred` | **640** | [`api.rs` line 77](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L77) | `OP_CCID_RX_DEFERRED` [`usb_link.rs` line 10](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L10) |
| `CcidRxTimeout` | 641 | [`api.rs` line 79](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L79) | (unused by harness) |
| `CcidTx` | **642** | [`api.rs` line 82](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L82) | `OP_CCID_TX` [`usb_link.rs` line 11](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L11) |
| `IrqCcidRx` | 770 | [`api.rs` line 73](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L73) | (server-internal) |
| `LinkStatus` | 0 | [`api.rs` line 9](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/api.rs#L9) | `OP_LINK_STATUS` [`usb_link.rs` line 9](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L9) |

**Connection flow:**

1. openpgp-apdu [`main.rs` line 35](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/main.rs#L35): `CcidLink::connect_to_usb_driver()`.
2. [`usb_link.rs` lines 55–58](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L55): `XousNames::new()` then `request_connection_blocking(SERVER_NAME_USB_DEVICE)`.
3. usb-bao1x [`main.rs` line 90](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L90): registers the server name.

**RX path (`CcidRxDeferred`):**

1. Host bulk OUT → Corigine IRQ → `CcidTransportClass` assembles CCID frames (`ccid_transport.rs` `drain_complete_messages`, **280–355**).
2. Inline: GetSlotStatus `0x65` and IccPowerOn `0x62` answered in IRQ; not queued.
3. Other messages (XfrBlock `0x6F`, …): `complete_rx.borrow_mut().push_back(frame)` then `try_send_message(IrqCcidRx)`.
4. Main `IrqCcidRx` delivers to parked listener, or queues on `cu.ccid_rx`.
5. openpgp-apdu `receive_rx()` (**usb_link.rs 73–84**): `lend_mut(conn, 640)` with `CcidCode::RxWait`; returns `data` on `RxAck`.

**TX path (`CcidTx`):**

1. openpgp-apdu `link.send_tx(response_frame)` — [`usb_link.rs` lines 86–96](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/usb_link.rs#L86).
2. usb-bao1x `Opcode::CcidTx` (**751–780**): `enqueue_response` + `sw_irq(CcidTx)` + wait `irq_serviced`.
3. IRQ `UsbIrqReq::CcidTx` → `ccid.poll()` → `poll_bulk_in` → `bulk_in.write()`.
4. Host receives RDR_to_PC on bulk IN.

**Error Paths & Recovery:**

1. **If `openpgp-apdu` never receives `CcidRxDeferred` frame:**
   - openpgp-apdu is blocked in `link.receive_rx()`, waiting forever for a frame
   - usb-bao1x IPC send may fail (InternalError) if the queue is full or the receiver crashed
   - openpgp-apdu's `receive_rx()` error handler (main.rs lines 81–89) logs warning and retries; does NOT panic
   - This is why the 50ms sleep on InternalError is critical (lines 81–89): prevents UART spam if usb-bao1x is down
2. **If `poll_bulk_in()` fails or returns without sending:**
   - `bulk_in.write()` at ccid_transport.rs:368 can return error (e.g., WouldBlock if ring is full)
   - `signal_bulk_in_attempted()` at line 370 is still called (sets `irq_serviced = true`)
   - Main loop unblocks regardless of submit success, preventing deadlock
   - The error is logged; response will never reach the host
   - Host-side timeout will occur; retry will go back through CcidRxDeferred
3. **If usb-bao1x crashes during boot (e.g., RefCell panic at old line 612):**
   - openpgp-apdu (PID 8) spins forever on `request_connection_blocking()` in usb_link.rs line 57 (retry loop in main.rs lines 34–43)
   - OS scheduler is alive, so other services continue, but boot is stalled
   - This is why UART capture is essential: to see whether usb-bao1x actually panicked or just takes time to init

**Frame structure** (from [`openpgp-apdu/src/ccid/mod.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/openpgp-apdu/src/ccid/mod.rs) and [`usb-bao1x/src/ccid_framing.rs`](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/ccid_framing.rs)):

10-byte header: `bMessageType`, `dwLength` (LE), `bSlot`, `bSeq`, then 3 type-specific bytes.

**PC_to_RDR (host → device):**

| Type | Code | Handler |
|------|------|---------|
| IccPowerOn | `0x62` | Inline ATR in usb-bao1x; openpgp-apdu skips CcidTx |
| IccPowerOff | `0x63` | Deferred: SlotStatus |
| GetSlotStatus | `0x65` | Inline SlotStatus in usb-bao1x; openpgp-apdu skips CcidTx |
| XfrBlock | `0x6F` | Deferred APDU |
| Abort | `0x72` | Deferred: SlotStatus |

**RDR_to_PC (device → host):**

| Type | Code | Builder |
|------|------|---------|
| DataBlock | `0x80` | `rdr_to_pc_data_block` / IccPowerOn ATR |
| SlotStatus | `0x81` | `rdr_to_pc_slot_status` |

IPC payload is `CcidMsgIpc { data: Vec<u8>, code: CcidCode }` (rkyv), same layout on both sides (`usb_link.rs` 13–28 vs usb-bao1x API).

---

## 8. Known-good image reference

**Build:** `cargo xtask dabao-ccid --no-verify` (7 processes, no openpgp-apdu)

**Flash archive:** `images/dabao-ccid/known-good/`

**Failing counterpart:** `cargo xtask dabao-ccid openpgp-apdu --no-verify` — archive `images/dabao-ccid/openpgp-apdu/`

**Services included:**

- PID 2: xous-ticktimer
- PID 3: keystore
- PID 4: xous-log
- PID 5: xous-names
- PID 6: usb-bao1x (feature `ccid-openpgp`, **without** openpgp-apdu process)
- PID 7: bao1x-hal-service

**Confirmed behavior** ([`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md), commit `f265ee346`):

- Boots and enumerates as **1d50:6197** (CCID + HID; no CDC on Persona A)
- High-speed, CCID bulk MPS 512
- GetSlotStatus survives **2392/2392** iterations over **120 seconds** (`out_depth 0`)
- Inline CCID opcodes **0x62 IccPowerOn** and **0x65 GetSlotStatus** work
- With an out-of-tree stub, `pcsc_scan` reports OpenPGP Card V2 ATR

**Why it is known-good:**

- Bugs 2–3 (pending-clear on write restore, synthetic OUT retire) are in this image.
- Deferred-path structural fixes are also in this image but **not exercised**: no openpgp-apdu → no XfrBlock listener → `CcidRxDeferred` is never parked by PID 8.
- Confirms those transport fixes do not break the inline path.

**Use as baseline:** if an openpgp-apdu image still fails after the RefCell fix, diff UART against a known-good boot (process-start lines, last panic, whether PID 6 printed `my PID is`).

---

## 9. Investigation status and next steps

### What's confirmed fixed

1. Blanket `EV_PENDING` clear on `UsbBus::write()` restore → FIXED (Bug 2, `driver.rs` 2832–2864 / 3234–3283)
2. Synthetic OUT retire in `force_prime_bulk_out` → FIXED (Bug 3, `driver.rs` 2920–2940)
3. `composite_handler` `else if` skipping SW_IRQ → FIXED (`hw.rs` 405, 556)
4. CcidRxDeferred TOCTOU re-check after park → FIXED (`main.rs` 617–632)
5. `irq_serviced` after bulk IN submission attempt → FIXED (`ccid_transport.rs` 368–370)

### What's not found as described

1. Blocking `log::info!` in `enable_interrupts` / `disable_interrupts` → **no such calls in git history** (Bug 1 claim)

### What's awaiting UART verification

1. RefCell double-borrow pop-into-local (`main.rs` 600, 715) → **in source**, awaiting petrn UART
2. Whether that fix makes the 8-process image enumerate → **unconfirmed**; last hardware result was "never reappears as 1d50:6197"

### If boot still fails after RefCell fix

Investigate in order:

1. UART: last line from usb-bao1x vs openpgp-apdu vs kernel (`print-panics` is on for dabao-ccid, [`xtask` 817](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/xtask/src/main.rs#L817)).
2. usb-bao1x init before any CcidRxDeferred: `map_memory`, `Bao1xUsb::new`, SE0, `cu.init()` ([`main.rs` 85–198](https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/services/usb-bao1x/src/main.rs#L85)).
3. `CcidTx` `irq_serviced` wait if `poll_bulk_in` returns early (`ccid_transport.rs` 360–365, `main.rs` 771).
4. `force_prime_bulk_out` re-arm (`main.rs` 658–659, 777; `driver.rs` 2898).
5. `composite_handler` double-lock early return (`hw.rs` 429–435).

### To debug further

1. Capture UART on **PB13 (Rx) / PB14 (Tx), 1 000 000 8N1** during `openpgp-apdu` boot. CDC-ACM is gone after `boot` leaves boot1.
2. Look for: does usb-bao1x panic? Where? Last log line? `"openpgp-apdu starting"`, `"USB driver connect failed"`, `"RefCell already borrowed"`.
3. Enable `irq-pending-trace` only after enumeration succeeds; it does not help if USB never comes up. Procedure: [`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md).

---

## 10. Discrepancies vs. earlier investigation notes

Recorded so later readers do not chase stale line numbers.

| Earlier claim | Actual at HEAD `6baceee6d` |
|---------------|----------------------------|
| `log::info!` in `enable_interrupts` / `disable_interrupts` | Never in git history of `driver.rs` |
| Bug 2 file is `hw.rs` save/restore of `EV_PENDING` | Write-path fix is `driver.rs` `restore_interrupts`. `hw.rs` composite_handler still writes `EV_PENDING = 0xffff_ffff` at line **401** after sampling |
| CcidRxDeferred bug at **line 612** | Handler **590–641**; pop-into-local at **600**; line **612** is now `assert_eq!(buf.code, CcidCode::RxWait)` |
| `force_prime` retire removed from `main.rs` ~2900 | Retire was never in `main.rs`. Function is `CorigineWrapper::force_prime_bulk_out` in `driver.rs` **2898–2953**. `main.rs` only **calls** it (659, 777) |
| Sleep on InternalError in usb-bao1x | Sleep is in **openpgp-apdu** `main.rs` **81–89** |
| openpgp-apdu "registration line 28" | Line **28** is `log::set_max_level`. Process does not register a name. Connect is **35** |
| Synthetic retire existed in parent commit | `force_prime_bulk_out` landed in `f265ee346` already without `retire_app_buf_ptr`; prior pattern lives in comments only |

---

## Files read for this map

| Path | Why |
|------|-----|
| `libs/bao1x-hal/src/usb/driver.rs` | Bugs 1–3, `force_prime_bulk_out`, IRQ mask/restore |
| `services/usb-bao1x/src/main.rs` | CcidRxDeferred, CcidTx, IrqCcidRx, register_name, primes |
| `services/usb-bao1x/src/hw.rs` | `composite_handler`, IRQ branching, `irq_serviced` attach |
| `services/usb-bao1x/src/ccid_transport.rs` | `poll_bulk_in`, `signal_bulk_in_attempted`, inline vs queue |
| `services/usb-bao1x/src/ccid_framing.rs` | Wire constants, RefCell unit test |
| `services/usb-bao1x/src/api.rs` | Opcodes and server name |
| `services/openpgp-apdu/src/main.rs` | Startup, main loop |
| `services/openpgp-apdu/src/usb_link.rs` | IPC client |
| `services/openpgp-apdu/src/ccid/mod.rs` | Frame parse/build |
| `services/openpgp-apdu/src/apdu/*`, `openpgp/*`, `Cargo.toml`, `lib.rs` | Harness layout |
| `xtask/src/main.rs`, `xtask/src/builder.rs` | Image composition, PID order |
| `services/xous-ticktimer/src/main.rs` | PID 2 |
| `services/keystore/src/main.rs` | PID 3 |
| `services/xous-log/src/main.rs` | PID 4 |
| `services/xous-names/src/main.rs` | PID 5 |
| `services/bao1x-hal-service/src/main.rs` | PID 7 |
| `docs/CCID_TEST_REPORT.md`, `OPENPGP_APDU_BOOT_DEBUG.md`, `code_map.md` | Hardware status |
| git: `f265ee346`, `f2c6dfbb8`, parents | Before/after |
