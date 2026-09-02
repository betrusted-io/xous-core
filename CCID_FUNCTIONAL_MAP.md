<!--
SPDX-License-Identifier: Apache-2.0
LOCAL WORKING DOCUMENT — do not commit or push unless explicitly requested.
Updated: 2026-08-18 for Persona A (CCID bulk-only = 6/8; no USB CDC; no boot PDDB).
Authoritative docs: docs/CCID_PROTOCOL_AND_HIL.md, docs/code_map.md, docs/CCID_TEST_REPORT.md
-->

# Functional map: USB CCID transport (`feature/usb-bao1x-ccid-openpgp`)

## Executive summary

This branch adds a **USB CCID bulk transport** plus **PDDB helpers** (`OKV1` marker,
offline PIN blob keys) under feature `ccid-openpgp`. There is **no** OpenPGP
applet, APDU parser, or card crypto in-tree. OpenPGP lives in an **out-of-tree**
handler that uses `CcidRxDeferred` / `CcidTx`.

**Persona A (current):** Corigine PEI has `CRG_EP_NUM = 8` unidirectional non-EP0
slots. CCID images allocate **CCID(2)+FIDO(2)+NKRO(2) = 6/8** (interrupt IN
omitted). Debug CDC and provisioning CDC are **not** allocated. Debug is UART /
`xous-log`. PIN lines are offline / pre-seeded PDDB only; `main.rs` does not
open PDDB at boot.

`cargo xtask baosec` does **not** enable CCID. Use `baosec-ccid` or `ccid-hil`.

---

## 1. Source surface (CCID-related)

| Path | Role |
|------|------|
| `services/usb-bao1x/src/ccid_framing.rs` | Wire assemble / chunk / OKV1; unit tests |
| `services/usb-bao1x/src/ccid_transport.rs` | USB class: bulk OUT/IN only (interrupt IN omitted) |
| `services/usb-bao1x/src/ccid_store.rs` | PDDB helpers behind `ccid-pddb`; not called at boot |
| `services/usb-bao1x/src/ep_budget.rs` | Cumulative `EpBudgetLedger` + unit tests |
| `services/usb-bao1x/src/hw.rs` | Persona A composite; no `SerialPort` when `ccid-openpgp` |
| `services/usb-bao1x/src/main.rs` | Boot OKV1 check; CCID IPC; serial opcodes gated on CCID |
| `services/usb-bao1x/src/api.rs` | CCID opcodes; `IrqProvSerialRx` removed |
| `libs/bao1x-hal/src/usb/driver.rs` | `allocated_non_ep0` live counter in `alloc_ep` |
| `tools/ccid_hil/*`, `tools/ccid_smoke.py` | Host HIL / smoke |
| `tools/check_ep_budget.py`, `test_ep_budget_cumulative.py`, `sim_persona_a_composite.py` | Host EP / Persona A gates |
| `xtask` | `baosec-ccid`, `ccid-hil` |
| `.github/workflows/ccid-ci.yml`, `ccid-hil.yml` | Compile + unit; HIL scaffolding |

---

## 2. Boot / composite construction (Persona A)

```
main_hw (ccid-openpgp):
  EpBudgetLedger::new
  make_ccid_transport (ledger.reserve CCID=2) -> CcidTransportClass
  Bao1xUsb::new(..., ccid, ledger) -> FIDO+NKRO only (no SerialPort)
  setup_usb_pins → SE0 Low → 500 ms → cu.init() → 150 ms → SE0 High
  Keyboard::new after SE0 High
  ledger / allocated_non_ep0 must stay <= 8
```

Stock (`not ccid-openpgp`): FIDO+NKRO+debug CDC = 7/8.

---

## 3. Key symbols (current)

| Symbol | File | Why it exists |
|--------|------|---------------|
| `CcidTransportClass` | `ccid_transport.rs` | USB CCID class driver |
| `append_bulk_out` / `drain_complete_frames` / `next_tx_chunk` | `ccid_framing.rs` | Multi-packet wire math |
| `is_provisioned_marker` / `CCID_PROVISIONED_MARKER` | `ccid_framing.rs` | `OKV1` detect |
| `is_ccid_provisioned` | `ccid_store.rs` | Offline helper; **not** called at boot |
| `save_provisioned_pins` | `ccid_store.rs` | Offline / factory seed (`ccid-pddb` only) |
| `EpBudgetLedger` | `ep_budget.rs` | Cumulative reserve before each class |
| `make_ccid_transport` | `hw.rs` | Build CCID class under ledger (2 EPs) |
| `Opcode::CcidRxDeferred` / `CcidTx` | `api.rs` | Handler IPC |
| `assert_persona_a_composite` | `ccid_usb.py` | Host: 0 CDC, >=1 CCID, >=2 HID, **6** non-EP0 |

**Removed (do not document as live):** `make_provisioning_serial`,
`IrqProvSerialRx`, `find_provisioning_port`, USB two-line PIN capture, second CDC.

---

## 4. Host tests — what / why

| Script | Pass | What | Why |
|--------|------|------|-----|
| `test_enumerate.py` | HIL-01 | Descriptor + Persona A composite | Wrong image / EP regression |
| `test_provision.py` | HIL-02 (Persona A) | **Zero CDC**; refuse `--legacy-usb-provision` | USB PIN path must stay gone |
| `test_echo.py` | HIL-03 / HIL-05 | GetSlotStatus + stress XfrBlock | Transport without OpenPGP |
| `ccid_smoke.py` | smoke PASS | Enum + Persona A + optional echo | One-shot host sanity |
| `check_ep_budget.py` | exit 0 | Static totals per xtask image | Catch overflow before HIL |
| `test_ep_budget_cumulative.py` | exit 0 | Independent vs cumulative gap | Documents old guard bug |
| `sim_persona_a_composite.py` | exit 0 | Mock layouts | No hardware needed |
| `cargo test … ccid_framing` | 9 pass | Frame math + GetSlotStatus/IccPowerOn helpers | No USB |
| `cargo test … ep_budget` | pass | Ledger overflow catch | No USB |

`CCID_HIL_PROVISION` is obsolete (ignored). HIL does **not** prove PDDB OKV1
(UART capture OPEN).

---

## 5. Images

| xtask | USB composite | Use |
|-------|---------------|-----|
| `baosec` | FIDO+NKRO+debug CDC | Stock |
| `baosec-ccid` | CCID+FIDO+NKRO | Production transport |
| `ccid-hil` | same + `ccid-echo` | HIL bench |

---

## 6. Explicitly out of scope

- OpenPGP / APDU / GnuPG E2E
- Automated UART OKV1 assertion in HIL
- USB PIN provisioning on CCID images
