#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Local Corigine USB endpoint-budget pre-check (no hardware).

Mirrors Persona A counting used by `usb-bao1x` / `assert_composite_ep_budget`
against `CRG_EP_NUM=8` (`libs/bao1x-hal/src/usb/driver.rs`).

Run anytime a USB class is added or an xtask feature set changes:

  python3 tools/check_ep_budget.py
  python3 tools/check_ep_budget.py --fail-fragile   # also fail if headroom < 2

This is intentionally a standalone arithmetic check: constructing `Bao1xUsb::new`
needs live Corigine MMIO and is not unit-testable in isolation here.
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from typing import List, Tuple

CRG_EP_NUM = 8  # libs/bao1x-hal/src/usb/driver.rs

# Class costs (unidirectional non-EP0 direction slots).
#
# CCID — services/usb-bao1x/src/ccid_transport.rs CcidTransportClass::new:
#   alloc.bulk (OUT), alloc.bulk (IN). Interrupt IN omitted (NKRO PEI collision).
EP_CCID = 2
# FIDO — xous-usb-hid device/fido.rs RawFidoConfig:
#   interface.rs allocate: interrupt IN + interrupt OUT                => 2
EP_FIDO = 2
# NKRO — xous-usb-hid device/keyboard.rs NKROBootKeyboardConfig:
#   interrupt IN + with_out_endpoint interrupt OUT                     => 2
EP_NKRO = 2
# Debug CDC — usbd-serial cdc_acm.rs CdcAcmClass::new:
#   interrupt (comm) + bulk OUT + bulk IN                              => 3
EP_DEBUG_CDC = 3
# Provisioning CDC (removed under Persona A; historical cost if re-enabled)
EP_PROV_CDC = 3
# Mass storage on usb-bao1x: feature flag exists but NOT wired in hw.rs (0 EPs)


@dataclass(frozen=True)
class Target:
    name: str
    classes: Tuple[str, ...]
    eps: int
    source: str
    via_xtask: bool
    notes: str = ""
    expect_overflow: bool = False


def sum_eps(*parts: int) -> int:
    return sum(parts)


TARGETS: List[Target] = [
    Target(
        name="baosec / baosec-lite / dabao (stock)",
        classes=("FIDO", "NKRO", "debug CDC"),
        eps=sum_eps(EP_FIDO, EP_NKRO, EP_DEBUG_CDC),
        source="services/usb-bao1x/src/hw.rs Bao1xUsb::new (not ccid-openpgp)",
        via_xtask=True,
        notes="xtask baosec/baosec-lite/dabao; headroom=1 (FRAGILE)",
    ),
    Target(
        name="baosec-ccid / dabao-ccid / ccid-hil (Persona A)",
        classes=("CCID", "FIDO", "NKRO"),
        eps=sum_eps(EP_CCID, EP_FIDO, EP_NKRO),
        source="ccid_transport.rs CcidTransportClass::new + HID; SerialPort cfg-gated off",
        via_xtask=True,
        notes="xtask baosec-ccid, dabao-ccid, ccid-hil; headroom=2",
    ),
    Target(
        name="baosec-emu (hosted)",
        classes=("(no Corigine alloc)",),
        eps=0,
        source="main_hosted.rs",
        via_xtask=True,
        notes="N/A for CRG_EP_NUM",
    ),
    Target(
        name="REJECTED: ccid + debug CDC",
        classes=("CCID", "FIDO", "NKRO", "debug CDC"),
        eps=sum_eps(EP_CCID, EP_FIDO, EP_NKRO, EP_DEBUG_CDC),
        source="pre-Persona A overflow",
        via_xtask=False,
        notes="9/8 (was 10/8 with CCID interrupt IN) — must stay impossible (cfg gate)",
        expect_overflow=True,
    ),
    Target(
        name="REJECTED: ccid + debug + provis CDC",
        classes=("CCID", "FIDO", "NKRO", "debug CDC", "prov CDC"),
        eps=sum_eps(EP_CCID, EP_FIDO, EP_NKRO, EP_DEBUG_CDC, EP_PROV_CDC),
        source="historical unprovisioned CCID image",
        via_xtask=False,
        notes="12/8 (was 13/8 with CCID interrupt IN) — path removed",
        expect_overflow=True,
    ),
]


def classify(eps: int) -> str:
    if eps == 0:
        return "N/A"
    if eps > CRG_EP_NUM:
        return "OVERFLOW"
    if eps == CRG_EP_NUM:
        return "PASS_FULL"
    if CRG_EP_NUM - eps <= 1:
        return "PASS_FRAGILE"
    return "PASS"


def main() -> int:
    parser = argparse.ArgumentParser(description="Corigine USB EP budget arithmetic check")
    parser.add_argument(
        "--fail-fragile",
        action="store_true",
        help="Exit non-zero if any shipping target has headroom < 2",
    )
    args = parser.parse_args()

    print(f"CRG_EP_NUM = {CRG_EP_NUM}")
    print(f"{'Target':<46} {'EPs':>7} {'Status':<12} Notes")
    print("-" * 100)

    exit_code = 0
    for t in TARGETS:
        status = classify(t.eps)
        label = f"{t.eps}/{CRG_EP_NUM}"
        print(f"{t.name:<46} {label:>7} {status:<12} {t.notes}")

        if t.expect_overflow:
            if t.eps <= CRG_EP_NUM:
                print("  ERROR: REJECTED row unexpectedly fits budget", file=sys.stderr)
                exit_code = 1
            continue

        if status == "N/A":
            continue
        if status == "OVERFLOW":
            print(f"  ERROR: over budget: {t.source}", file=sys.stderr)
            exit_code = 1
        if args.fail_fragile and status in ("PASS_FRAGILE", "PASS_FULL"):
            print(f"  ERROR: fragile under --fail-fragile: {t.name}", file=sys.stderr)
            exit_code = 1

    print()
    print("Class cite summary:")
    print(f"  CCID  = {EP_CCID}  services/usb-bao1x/src/ccid_transport.rs alloc.bulk x2 (no interrupt IN)")
    print(f"  FIDO  = {EP_FIDO}  xous-usb-hid interface.rs interrupt IN+OUT (fido.rs)")
    print(f"  NKRO  = {EP_NKRO}  xous-usb-hid keyboard.rs in_endpoint + with_out_endpoint")
    print(f"  CDC   = {EP_DEBUG_CDC}  usbd-serial cdc_acm.rs interrupt + bulk x2")
    print()
    print("Guard note:")
    print("  EpBudgetLedger (services/usb-bao1x/src/ep_budget.rs) tracks CUMULATIVE totals.")
    print("  Per-class assert_class_ep_budget remains; live count: CorigineWrapper.allocated_non_ep0.")
    print("  Regression: cargo test -p usb-bao1x --lib ep_budget")
    print("             python3 tools/test_ep_budget_cumulative.py")
    print()
    print("Other xtask USB composites:")
    print("  precursor usbdev/usb-device-xous — Spinal UDC, not CRG_EP_NUM (out of scope).")
    print("  bao1x-sim / bao1x / baremetal — no usb-bao1x composite like baosec.")
    print()
    print("Untested cargo combos (bypass xtask):")
    print("  board-baosec,ccid-openpgp[,ccid-echo] — same Persona A 6/8 if built")
    print("  board-dabao,ccid-openpgp — same 6/8 math (official xtask: dabao-ccid)")
    print("  *,mass-storage on usb-bao1x — feature empty; no EP alloc in hw.rs")
    print("  ccid-openpgp + SerialPort — not reachable (cfg(not(ccid-openpgp)))")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
