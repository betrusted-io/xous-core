<!--
SPDX-License-Identifier: Apache-2.0
-->

# openpgp-apdu Boot Failure Debugging Guide

**Status:** Hardware debugging hand-off. UART capture is required for the next step.

**Scope:** `cargo xtask dabao-ccid openpgp-apdu` images on Dabao (`board-dabao`).
Related: `services/openpgp-apdu/`, `services/usb-bao1x/`, `xtask` `dabao-ccid`.

Line numbers below are a snapshot from the investigation that produced this
guide. Verify against current HEAD before treating them as exact.

Related maps:

- [`docs/CCID_PROTOCOL_AND_HIL.md`](CCID_PROTOCOL_AND_HIL.md)
- [`docs/CCID_USB_ENUMERATION_DEBUG.md`](CCID_USB_ENUMERATION_DEBUG.md)
- [`docs/code_map.md`](code_map.md)
- [`docs/CCID_TEST_REPORT.md`](CCID_TEST_REPORT.md)

---

## 1. Symptom

The `dabao-ccid` image **with** the in-tree `openpgp-apdu` service never
enumerates as a USB device.

Observed sequence after flashing UF2s and sending `boot` on the boot1 USB
serial console (`/dev/ttyACM0`, 1 000 000 baud):

1. Board is in boot1 as `1d50:6196` (BAOCHIP mass-storage + CDC-ACM).
2. `boot` is sent.
3. USB drops (expected during stage change).
4. The board **never** reappears as `1d50:6197` (or as any `1d50:*` device).

This is a **boot-time / early-init failure**, not a runtime CCID timeout.
`pcsc_scan` and `gpg --card-status` cannot be run: there is no CCID reader.

Build command that produces the failing image:

```sh
cargo xtask dabao-ccid openpgp-apdu --no-verify
```

---

## 2. What is already known

### Discrimination test (deferred-path fixes are sound)

Rebuild and reflash **without** `openpgp-apdu`:

```sh
cargo xtask dabao-ccid --no-verify
```

That image enumerates cleanly as `1d50:6197` (Dabao CCID) after `boot`.
The same `usb-bao1x` deferred-path changes are in both images.

Conclusion: the three deferred-path fixes in `usb-bao1x` are **not** the
cause of the missing enumeration. The regression is specific to including
`openpgp-apdu` in the 8-process kernel image.

### Static review (no obvious standalone bug)

| Area | Finding |
|------|---------|
| xtask | `builder.add_service("openpgp-apdu", LoaderRegion::Flash)` via positional cratespec or `--with-openpgp-test-apdu`. Same pattern as `usb-bao1x`. |
| Binary spec | Crate name `openpgp-apdu` (not a path). Image lists PID 8 as `openpgp-apdu`. |
| Dependencies | `xous` 0.9.70, `xous-ipc` 0.10.10, `xous-names` 0.9.71, `rkyv` 0.8.8 — match `usb-bao1x`. |
| IPC | Server name `"_Xous USB device driver_"`, opcodes 640 / 642, `CcidMsgIpc` layout match `usb-bao1x`. |
| Image size | Kernel with `openpgp-apdu` ~1.23 MB; signing reported ~2.1 MB remaining. Unlikely RRAM overflow. |

### Defensive refactoring already applied

`services/openpgp-apdu/src/main.rs` no longer panics on:

- `log_server::init_wait()` failure (continues without logging)
- `CcidLink::connect_to_usb_driver()` failure (retry loop with `xous::yield_slice()`)

Reflash of that change **still failed to enumerate**. Those `.expect()` calls
were therefore not the root cause of the USB drop.

Note: `xous_names::XousNames::new()` still uses an internal `.expect()` in
`api/xous-api-names`. A names-server miss would still panic inside the
library, not in `openpgp-apdu` itself.

---

## 3. What still needs to be determined

Physical UART on **PB13 (Rx) / PB14 (Tx), 1 000 000 8N1**. CDC-ACM is gone
as soon as `boot` leaves boot1, so boot1 USB serial cannot show kernel /
service panics.

Capture from `boot` until either `1d50:6197` appears or ~15 s of silence.

Look for:

- `"openpgp-apdu starting (PID ...)"` — process 8 reached `ccid_main`
- `"USB driver connect failed"` — connect retry loop
- `"my PID is"` from `usb-bao1x` — USB driver started
- `"Missing PUBLIC_SERIAL"` / `"couldn't reserve register pages"` / `"couldn't allocate IFRAM"`
- Kernel panic hook output (`print-panics` is enabled on `dabao-ccid`)
- Loader / kernel messages that stop at a specific PID

---

## 4. Where to look, given UART output

### If UART shows an `openpgp-apdu` panic or stall

| UART clue | Code to inspect |
|-----------|-----------------|
| Panic before `"openpgp-apdu starting"` | `services/openpgp-apdu/src/main.rs` `ccid_main()`; `log_server::init_wait` |
| `"Couldn't connect to XousNames"` | `api/xous-api-names/src/lib.rs` `XousNames::new()`; `services/xous-names` |
| `"USB driver connect failed"` looping | `services/openpgp-apdu/src/usb_link.rs` `connect_to_usb_driver()`; usb-bao1x never registered `"_Xous USB device driver_"` |
| Hang with starting log, no USB | `receive_rx()` blocking `lend_mut` on opcode 640 — should **not** stop usb-bao1x enumeration; look at usb-bao1x instead |

### If UART shows a `usb-bao1x` panic during init

| UART clue | Code to inspect |
|-----------|-----------------|
| `"Missing PUBLIC_SERIAL"` | `services/usb-bao1x/src/main.rs` env lookup; loader `PUBLIC_SERIAL` |
| `"couldn't reserve register pages"` / IFRAM / IRQ map | same `main.rs` `map_memory` calls before `Bao1xUsb::new` |
| `"can't register server"` | `xns.register_name(SERVER_NAME_USB_DEVICE, ...)` |
| Panic inside `Bao1xUsb::new` / `cu.init()` | `services/usb-bao1x/src/hw.rs`; Corigine init; SE0 delays |

### If UART shows a kernel / loader failure

| UART clue | Code to inspect |
|-----------|-----------------|
| Process table / OOM / page alloc | kernel process load of PID 8; `xous-create-image` IniF for `openpgp-apdu` |
| Loader abort before kernel | `loader/src/platform/bao1x/` |
| Silent halt, no panics | stack overflow in PID 8 or IRQ path; enable more `debug-print` |

### If UART is clean and USB still never enumerates

Treat as usb-bao1x init hanging (SE0 / `cu.init()` / IRQ) under the 8-process
image, not as a CCID deferred-path bug. Rebuild with `irq-pending-trace` only
after the device enumerates; that instrumentation does not help if USB never
comes up.

---

## 5. Hypotheses to test with UART

1. **usb-bao1x panics or hangs during init in the 8-process image**
   (memory pressure, `map_memory`, IFRAM, SE0). `openpgp-apdu` then waits
   forever on BlockingConnect. USB never enumerates. Most consistent with
   "no `1d50:*` at all".

2. **openpgp-apdu panics in `XousNames::new()`** (library `.expect()`) before
   usb-bao1x finishes init, and the kernel stops globally. Less likely unless
   panics are fatal to the whole system.

3. **Loader / kernel fails to start PID 8**, leaving the rest of the image in
   a bad state. Check process-start messages vs. the 7-process known-good image.

4. **Not a runtime CCID bug.** Inline vs deferred XfrBlock is irrelevant until
   `1d50:6197` appears.

---

## 6. Flash / boot procedure used in this investigation

1. Hold PROG, plug in: boot1 as `1d50:6196`, volume `BAOCHIP`, `/dev/ttyACM0`.
2. Copy `loader.uf2`, `xous.uf2`, `apps.uf2` from the chosen archive under
   `target/riscv32imac-unknown-xous-elf/release/built/` (or from
   `target/riscv32imac-unknown-xous-elf/release/` after a fresh `xtask` build).
3. `sync`.
4. Send `boot` at 1 000 000 8N1 on `/dev/ttyACM0` (PROG alone was not used
   after the first failed boot).
5. Watch `lsusb -d 1d50:` for `6197` within ~10 s.

Known-good (enumerates): `cargo xtask dabao-ccid --no-verify`

Archive: `target/riscv32imac-unknown-xous-elf/release/built/known-good/`

Failing (drops off USB): `cargo xtask dabao-ccid openpgp-apdu --no-verify`

Archive: `target/riscv32imac-unknown-xous-elf/release/built/openpgp-apdu/`

These archives are local build products (`target/` is gitignored). A later
`xtask` run overwrites the files in `release/` but does not touch `built/`.

---

## 7. Out of scope until UART exists

- Further code changes to `openpgp-apdu` or `usb-bao1x` without a panic /
  hang line from UART.
- `irq-pending-trace` / flight-ring analysis (requires a live USB stack).
- Host tests (`pcsc_scan`, `gpg --card-status`).
