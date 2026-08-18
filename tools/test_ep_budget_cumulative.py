#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression: cumulative EP budget vs independent per-class subtotals.

Demonstrates the guard gap closed by EpBudgetLedger:
  OLD: assert each class subtotal independently → fake +3 on 6/8 PASSES
  NEW: cumulative running total → fake +3 on 6/8 FAILS

Mirrors services/usb-bao1x/src/ep_budget.rs unit tests (run those with cargo).

  python3 tools/test_ep_budget_cumulative.py
"""

from __future__ import annotations

import sys

CRG_EP_NUM = 8


def old_independent_ok(subtotals: list[int]) -> bool:
    """Pre-fix guard semantics: each subtotal checked alone."""
    return all(n <= CRG_EP_NUM for n in subtotals)


def cumulative_reserve(parts: list[tuple[str, int]]) -> tuple[bool, int, str]:
    """New EpBudgetLedger::reserve_before_alloc semantics."""
    total = 0
    for name, n in parts:
        if n > CRG_EP_NUM:
            return False, total, f"class {name} alone claims {n}"
        total += n
        if total > CRG_EP_NUM:
            labeled = "+".join(f"{a}({b})" for a, b in parts[: parts.index((name, n)) + 1])
            return False, total, f"{labeled}={total} > {CRG_EP_NUM}"
    return True, total, "ok"


def main() -> int:
    print("=== Construction-order alloc sites (Persona A / stock) ===")
    print("CCID image:")
    print("  1. ccid_transport.rs CcidTransportClass::new: alloc.bulk OUT, bulk IN (=2; no interrupt IN)")
    print("  2. xous-usb-hid NKRO: interrupt IN+OUT (=2)")
    print("  3. xous-usb-hid FIDO: interrupt IN+OUT (=2)")
    print("  (no CDC) total 6 — live count: CorigineWrapper.allocated_non_ep0")
    print("Stock baosec:")
    print("  1-2. NKRO+FIDO (=4) then usbd-serial CDC interrupt+bulk×2 (=3) → 7")
    print()

    persona = [("CCID", 2), ("NKRO", 2), ("FIDO", 2)]
    ok, tot, msg = cumulative_reserve(persona)
    assert ok and tot == 6, msg
    print(f"PASS cumulative persona A: {tot}/8")

    stock = [("NKRO", 2), ("FIDO", 2), ("debug CDC", 3)]
    ok, tot, msg = cumulative_reserve(stock)
    assert ok and tot == 7, msg
    print(f"PASS cumulative stock: {tot}/8")

    # Regression gap proof (matches ep_budget.rs: CCID(2)+HID(4)+FAKE(3))
    old_subs = [2, 4, 3]
    assert old_independent_ok(old_subs), "OLD independent must pass (documents the gap)"
    print("PASS old-independent([2,4,3]) — GAP: would allow overflow")

    overflow = [("CCID", 2), ("NKRO", 2), ("FIDO", 2), ("FAKE_EXTRA", 3)]
    ok, tot, msg = cumulative_reserve(overflow)
    assert not ok and tot == 9, "NEW cumulative must reject 6+3"
    print(f"PASS new-cumulative rejects FAKE_EXTRA: {msg}")

    print()
    print("Also run: cargo test -p usb-bao1x --lib ep_budget")
    print("OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
