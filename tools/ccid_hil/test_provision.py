#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""HIL-02: Persona A provisioning surface check (no USB CDC provisioning).

Under Persona A (`ccid-openpgp` images), the Corigine 8-slot budget forces
CCID+FIDO+NKRO only. Debug CDC and provisioning CDC are NOT allocated.
PIN lines are offline / pre-flash PDDB only; host USB cannot provision.

This test therefore does NOT open a serial port or send two PIN lines.
It verifies the CCID image presents NO CDC ACM interfaces for the device VID:PID
while CCID is present — the USB path that `test_provision.py` used to exercise
is gone by design.

Offline OKV1 / PDDB seeding cannot be proven over USB from this harness (OPEN:
UART log capture not wired into HIL). When that is added, a follow-up can assert
boot log text for OKV1 vs warn-and-continue.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ccid_usb import (
    BAOSEC_PID,
    BAOSEC_VID,
    assert_persona_a_composite,
    find_ccid_device,
    list_cdc_interfaces,
)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="HIL-02 Persona A: no USB CDC provisioning on CCID images"
    )
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAOSEC_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=BAOSEC_PID)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument(
        "--legacy-usb-provision",
        action="store_true",
        help="Deprecated: refuses to run old CDC two-line path (always exits 2)",
    )
    args = parser.parse_args()

    if args.legacy_usb_provision:
        print(
            "HIL-02 REFUSED: --legacy-usb-provision is dead under Persona A "
            "(no provisioning CDC on CCID images). Seed PDDB offline instead.",
            file=sys.stderr,
        )
        return 2

    eps = find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)
    layout = assert_persona_a_composite(eps.device)
    cdc = list_cdc_interfaces(eps.device)
    if cdc:
        print(
            f"FAIL: Persona A expects zero CDC interfaces; found {cdc}",
            file=sys.stderr,
        )
        return 1

    print(
        "HIL-02 PASS (Persona A): no CDC ACM; USB PIN provision path absent; "
        f"layout={layout}"
    )
    print(
        "NOTE: Offline OKV1 / UART boot-warn not checked here "
        "(HIL harness has no UART capture yet)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
