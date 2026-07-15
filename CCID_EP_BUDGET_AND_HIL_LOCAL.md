<!--
SPDX-License-Identifier: Apache-2.0

LOCAL WORKING REPORT — do not treat as committed docs unless explicitly asked.
Same convention as CCID_FUNCTIONAL_MAP.md.
Updated: 2026-07-15 — Persona A + cumulative EpBudgetLedger.
-->

# CCID Persona A — local HIL / EP-budget verification

Branch: `feature/usb-bao1x-ccid-openpgp`  
Scope: endpoint-budget audit and HIL surface after Persona A  
No commit / push / PR from this report unless asked.

## Part 1 — Tests (what / why)

| File | What it tests | Why |
|------|---------------|-----|
| `tools/ccid_hil/test_provision.py` | CCID present, **zero CDC**, refuse `--legacy-usb-provision` | Persona A: USB PIN path must stay gone; CDC return = fail |
| `tools/ccid_hil/ccid_usb.py` | `assert_persona_a_composite` (7 non-EP0, ≥2 HID, ≥1 CCID, 0 CDC) | Catch wrong composite without hardcoded IF indices |
| `tools/ccid_hil/test_enumerate.py` | CCID descriptor + Persona A (default) | Enum + layout in one gate |
| `tools/ccid_hil/test_echo.py` | GetSlotStatus / stress XfrBlock via class 0x0B | Transport without OpenPGP handler |
| `tools/ccid_hil/run_all.sh` | Ordered 00–04; ignores `CCID_HIL_PROVISION` | Full HIL; notes UART OPEN |
| `tools/ccid_smoke.py` | Enum + Persona A + optional echo | One-shot host sanity |
| `tools/check_ep_budget.py` | Static EP totals vs 8 | Pre-hardware overflow |
| `tools/test_ep_budget_cumulative.py` | Independent vs cumulative gap | Documents old guard bug |
| `tools/sim_persona_a_composite.py` | Mock layouts | No device needed |
| `cargo test -p usb-bao1x --lib ep_budget` | Ledger + 7+2 overflow | Firmware-side regression |
| `cargo test -p usb-bao1x --lib ccid_framing` | Frame math | Wire without USB |

HIL-02 does **not** prove PDDB OKV1 (needs UART / offline inspect).

## Part 2 — USB targets vs `CRG_EP_NUM=8`

| Target / combo | Classes | EPs | vs 8 | Notes |
|----------------|---------|-----|------|-------|
| `xtask baosec` | FIDO(2)+NKRO(2)+debug CDC(3) | **7** | OK (1 spare) | Stock |
| `xtask baosec-ccid` / `ccid-hil` | CCID(3)+FIDO(2)+NKRO(2) | **7** | OK (1 spare) | Persona A |
| REJECTED: CCID+HID+CDC | 3+2+2+3 | **10** | OVERFLOW | pre-fix |
| REJECTED: +prov CDC | 10+3 | **13** | OVERFLOW | path removed |

### Guards (current)

1. **`EpBudgetLedger`** (`ep_budget.rs`): `reserve_before_alloc` before each class; sum must fit.
2. **`allocated_non_ep0`** (`bao1x-hal` USB driver): live count incremented in `alloc_ep`.
3. **`assert_class_ep_budget` / `assert_composite_ep_budget`**: per-class sanity only; alone they miss cumulative overflow (see unit tests).

## Part 3 — How to run (no hardware)

```bash
cargo test -p usb-bao1x --lib ccid_framing
cargo test -p usb-bao1x --lib ep_budget
python3 tools/check_ep_budget.py
python3 tools/test_ep_budget_cumulative.py
python3 tools/sim_persona_a_composite.py
```

With hardware (`ccid-hil` image):

```bash
tools/ccid_hil/run_all.sh
python3 tools/ccid_smoke.py
```

Board compile (after `cargo xtask install-toolkit`):

```bash
cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x \
  --target riscv32imac-unknown-xous-elf
```

## OPEN

1. UART capture not in HIL — cannot auto-assert OKV1 warn vs continue.
2. Stock and Persona A both 7/8 — any new USB class needs an exclusion.
3. Do not commit local-only notes unless asked.
