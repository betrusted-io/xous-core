<!--
SPDX-License-Identifier: Apache-2.0
-->

# Follow-Up Issue: GetSlotStatus WriteUSB Timeout (Post-Enumeration)

**Primary goal: ACHIEVED (enumeration)**  
**Secondary finding: IDENTIFIED (CCID bulk OUT traffic; not a boot/RefCell blocker)**

Related:

- [`OPENPGP_APDU_CODE_MAP.md`](OPENPGP_APDU_CODE_MAP.md) — fix inventory, IPC, boot PID order
- [`OPENPGP_APDU_BOOT_DEBUG.md`](OPENPGP_APDU_BOOT_DEBUG.md) — UART / discrimination procedure
- [`CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md) — known-good hardware ATR results

This is a **next-phase** investigation for xous-core maintainers or future CCID
contributors. It does **not** block documenting or merging enumeration-related
work that already landed on `feature/usb-bao1x-ccid-openpgp`.

---

## 1. Status summary

### Primary goal: ACHIEVED

- 8-process `dabao-ccid openpgp-apdu` image enumerates as **`1d50:6197`**.
- Confirmed on host USB (local Dabao flash) and independently via petrn UART /
  board bring-up context used in the boot investigation.
- Deferred-path structural fixes are in tree (RefCell pop-into-local, IRQ
  `else if` → two `if`s, `irq_serviced` after bulk IN submit). See the
  [code map](OPENPGP_APDU_CODE_MAP.md).

### Secondary finding: IDENTIFIED (does not undo enumeration)

- After enumeration, PC/SC **GetSlotStatus** fails: `pcscd` logs
  `WriteUSB() ... LIBUSB_ERROR_TIMEOUT`, card state **Status unavailable** /
  **Card not present**, `gpg --card-status` → no device.
- This is a **bulk OUT completion / arming** problem, not a RefCell panic at
  boot and not “missing ATR builder logic” in isolation (inline GetSlotStatus /
  IccPowerOn never get a chance if WriteUSB never completes).
- **Discrimination (already run):**
  - **8-process** (`images/dabao-ccid/openpgp-apdu/`): enum OK, WriteUSB timeout.
  - **7-process known-good** (`images/dabao-ccid/known-good/`): enum OK,
    **Card inserted**, ATR
    `3B DA 18 FF 81 B1 FE 75 1F 03 00 31 C5 73 C0 01 40 00 90 00 0C`.

### Confidence level: Medium

- Hypothesis is code-backed (`driver.rs` + `usb-bao1x` / `openpgp-apdu` main).
- No `irq-pending-trace` / flight-ring capture yet on a failing 8-process run.
- Several mechanisms remain plausible; one smoking-gun line is not proven.

---

## 2. Hypothesis H2: Durable OUT starvation via `force_prime` gate

### Mechanism

1. openpgp-apdu waits for Configured by calling `link.link_status()` about every
   20 ms ([`services/openpgp-apdu/src/main.rs` 50–64](../services/openpgp-apdu/src/main.rs)).
2. Each `Opcode::LinkStatus` that reports **Configured** calls soft
   **`prime_bulk_out()`** ([`services/usb-bao1x/src/main.rs` 1179–1196](../services/usb-bao1x/src/main.rs)
   — note: **not** `force_prime` on this path).
3. After Configured, `receive_rx()` parks on `CcidRxDeferred`; empty-queue path
   calls soft **`prime_bulk_out()`** again
   ([`main.rs` 610–616](../services/usb-bao1x/src/main.rs)).
4. Independently, a timer thread sends `CcidPrimeBulkOut` every 100 ms; when
   Configured, that path calls **`force_prime_bulk_out()`**
   ([`main.rs` 332–346, 645–659](../services/usb-bao1x/src/main.rs)).
5. In [`force_prime_bulk_out`](../libs/bao1x-hal/src/usb/driver.rs)
   ([`driver.rs` 2898–2953](../libs/bao1x-hal/src/usb/driver.rs)): if
   `app_enq_index != app_deq_index` (**2927–2940**), re-arm is **skipped**
   (no synthetic retire). If software advanced enqueue (e.g. `get_app_buf_ptr` +
   `bulk_xfer`) but the completion path never retires the slot
   (`UsbBus::read` retire ~**3345**), force_prime never recovers → **no live
   OUT TRB** → host WriteUSB times out forever.
6. Soft `prime_bulk_out` / `UsbBus::read` only arms when `ep_out_ready` was
   false; stuck `ep_out_ready == true` without a TRB is also a soft-prime no-op
   (see code map / driver comments).

### Why 8-process only (vs known-good)

| Factor | 7-process known-good | 8-process + openpgp-apdu |
|--------|----------------------|---------------------------|
| `CcidRxDeferred` park + prime at 616 | Never | Yes, after Configured |
| Frequent `LinkStatus` → prime at Configured | Rare / none from a handler | Every ~20 ms until Configured |
| Periodic `force_prime` (100 ms) | Yes | Yes |
| Host GetSlotStatus / ATR | Works | WriteUSB timeout |

### Why this is not “the earlier boot fixes failed”

| Id | Hypothesis | Role here |
|----|------------|-----------|
| H1 | RefCell held across park | **Ruled out** — pop at 600; park at 611 after `RefMut` drop |
| H3 | `irq_serviced` race | **Unlikely for WriteUSB** — TX / bulk IN wait, not OUT |
| H4 | Early connect before USB init | **Contributing** (name at line 90; LinkStatus can queue early) — does not alone explain permanent starvation |

---

## 3. What the code rules out

| Hypothesis | Status | Evidence |
|------------|--------|----------|
| RefCell held across park | RULED OUT | [`main.rs` 600](../services/usb-bao1x/src/main.rs), park at 611 |
| usb-bao1x main blocked by park | RULED OUT | `msg_opt.take()` + return; `reply_and_receive_next` continues |
| IRQ handler blocked by park | RULED OUT | `composite_handler` is IRQ-context; park is main IPC only |
| CcidRxDeferred skips re-arm | RULED OUT | Soft `prime_bulk_out()` at **616** |
| `irq_serviced` breaks OUT | UNLIKELY | Set in `poll_bulk_in` after IN write ([`ccid_transport.rs` ~370](../services/usb-bao1x/src/ccid_transport.rs)) |

**Not:** RefCell deadlock, IPC parking of the whole USB process, IRQ stall, or “forgot to call prime.”

**Likely:** OUT ring / `ep_out_ready` desync plus `force_prime`’s `enq != deq` safety gate preventing recovery, triggered by the 8-process LinkStatus + deferred-park priming pattern.

---

## 4. Code locations

| Issue | File | Lines | What to look for |
|-------|------|-------|------------------|
| Configured wait + LinkStatus poll | [`openpgp-apdu/.../main.rs`](../services/openpgp-apdu/src/main.rs) | 50–64, 66–73 | How often LinkStatus hits before/after park |
| LinkStatus soft prime | [`usb-bao1x/.../main.rs`](../services/usb-bao1x/src/main.rs) | 1179–1196 | `Configured` → `prime_bulk_out()` every poll |
| Soft prime during park | same | 590–641 (esp. **616**) | Empty queue → park → `prime_bulk_out()` |
| Periodic force_prime | same | 332–346, 645–659 | 100 ms `CcidPrimeBulkOut` |
| `force_prime` gate | [`bao1x-hal/.../driver.rs`](../libs/bao1x-hal/src/usb/driver.rs) | **2927–2940** | Skip when `enq != deq`; no sync-back |
| Completion retire | same | ~**3345** (`UsbBus::read`) | Does `retire_app_buf_ptr` always run after OUT DMA done? |
| `ep_out_ready` | same | ~2892–2951, ~3371+ | Stuck true → soft prime no-op |

GitHub (branch):  
https://github.com/betrusted-io/xous-core/blob/feature/usb-bao1x-ccid-openpgp/

---

## 5. Experiments (no UART required)

### Experiment 1: Telemetry capture (recommended)

Rebuild with `irq-pending-trace` (feature name as wired in `usb-bao1x` /
`bao1x-hal` Cargo.toml — confirm flag spelling in-tree before build):

```bash
# Example — adjust if xtask feature passthrough differs:
cargo xtask dabao-ccid openpgp-apdu --no-verify
# with irq-pending-trace enabled on usb-bao1x / bao1x-hal for that image
```

Flash the 8-process image, reproduce timeout (`pcsc_scan` / `opensc-tool -a` /
`gpg --card-status`), then:

```bash
python3 tools/bulk_trb_trace_poll.py -o bulk-trb-after-timeout.log
```

**Look for:**

- `out_enq != out_deq` (enqueue advanced, no matching consume)
- `out_consumed` frozen / not tracking arms
- `force_prime` count still climbing (retries without recovery)

If that pattern appears: **H2 is confirmed.**

### Experiment 2: Delay deferred park (code change)

Do **not** call `receive_rx()` until after Configured and a deliberate delay (or
until after a first successful host session). If GetSlotStatus / ATR then work
on the 8-process image, deferred park + park-time prime are implicated.

### Experiment 3: Limit LinkStatus priming (code change)

Gate [`main.rs` 1193–1194](../services/usb-bao1x/src/main.rs) so soft prime runs
**once** on first Configured LinkStatus (or rely only on the 100 ms
`CcidPrimeBulkOut` path), not on every openpgp-apdu poll. If GetSlotStatus
recovers, LinkStatus priming churn is the trigger.

### Experiment 4: Known-good baseline (already run)

```bash
# Flash images/dabao-ccid/known-good/ (no openpgp-apdu)
sudo systemctl restart pcscd.socket pcscd.service
timeout 8 pcsc_scan -n
timeout 10 opensc-tool -a
```

**Result (local HIL):** PASS — Card inserted + ATR as above. Isolates the
failure to the **8-process + openpgp-apdu startup / deferred pattern**, not
cable / pcscd / board.

---

## 6. Confidence and recommendations

**Confidence: Medium** (code-backed; no failing-run telemetry dump yet)

**Why not high:** Several recovery-failure mechanisms (force_prime gate,
stuck `ep_out_ready`, lost HW TRB with advanced enq). No flight-ring proof on
the failing image.

**Why not low:** Inspection rules out RefCell / full-process park / missing
prime attempt. Pattern matches unique 8-process LinkStatus + `CcidRxDeferred`
prime. Known-good discrimination already passed.

**Suggested priority:**

1. **High:** Experiment 1 (telemetry on failing 8-process image).
2. **Done / reference:** Experiment 4 (known-good ATR) — keep as regression baseline.
3. **Medium:** Experiments 2–3 only after telemetry supports H2 (avoid speculative churn).

**Not blocking** the primary enumeration milestone. Track as follow-up CCID
transport work for maintainers or future contributors.

---

## Quick reproduction (8-process fail)

```bash
# Flash images/dabao-ccid/openpgp-apdu/ → boot → wait for 1d50:6197
lsusb -d 1d50:6197
sudo systemctl restart pcscd.socket pcscd.service
timeout 8 pcsc_scan -n          # reader may appear; Card state: Status unavailable
timeout 10 opensc-tool -a       # Card not present
journalctl -u pcscd --since '1 min ago' | grep WriteUSB
# expect: LIBUSB_ERROR_TIMEOUT
```
