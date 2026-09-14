#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""HIL-01: CCID enumeration + Persona A composite layout.

What: bcdCCID / max message length; by default also 0 CDC, >=1 CCID, >=2 HID,
and 7 non-EP0 endpoints (Persona A: CCID+FIDO+NKRO).

Why: catch a wrong image (stock with CDC, overflow composite that somehow linked,
or drifted descriptors) before echo/stress hide the problem. Use --skip-persona-a
only when intentionally testing a non-Persona-A device.
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
    verify_ccid_descriptor,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAOSEC_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=BAOSEC_PID)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument(
        "--skip-persona-a",
        action="store_true",
        help="Only check CCID descriptor (omit composite/CDC/EP budget asserts)",
    )
    args = parser.parse_args()

    eps = find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)
    desc = verify_ccid_descriptor(eps.device)
    if desc["bcd_ccid"] != 0x0110 or desc["max_message_length"] != 0x10F:
        print(f"FAIL: descriptor mismatch: {desc}", file=sys.stderr)
        return 1

    if not args.skip_persona_a:
        try:
            layout = assert_persona_a_composite(eps.device)
        except RuntimeError as exc:
            print(f"FAIL: {exc}", file=sys.stderr)
            return 1
        print(f"Persona A composite OK: {layout}")

    print("HIL-01 PASS: CCID enumeration")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
