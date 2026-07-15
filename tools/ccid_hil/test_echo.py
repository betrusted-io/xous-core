#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""HIL-03 / HIL-05: CCID bulk echo round-trip (requires ccid-echo image).

What: GetSlotStatus echo (HIL-03); optional --stress N random XfrBlock echoes
(HIL-05). Finds the CCID interface by class 0x0B (no CDC / index assumptions).

Why: prove USB bulk transport and multi-packet reassembly without an OpenPGP
handler. Does not check Persona A layout (that is HIL-01/02) or PDDB/OKV1.
"""

from __future__ import annotations

import argparse
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ccid_usb import (
    BAOSEC_PID,
    BAOSEC_VID,
    ccid_bulk_roundtrip,
    find_ccid_device,
    make_get_slot_status,
    make_xfr_block,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAOSEC_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=BAOSEC_PID)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--stress", type=int, default=0, help="repeat random XfrBlock count")
    args = parser.parse_args()

    eps = find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)

    frame = make_get_slot_status(seq=0)
    reply = ccid_bulk_roundtrip(eps, frame)
    if reply != frame:
        print("FAIL: GetSlotStatus echo mismatch", file=sys.stderr)
        return 1

    stress_n = max(args.stress, 1)
    for i in range(stress_n):
        payload_len = random.randint(1, 128) if args.stress else 16
        payload = bytes(random.getrandbits(8) for _ in range(payload_len))
        xfr = make_xfr_block(seq=(i + 1) & 0xFF, payload=payload)
        reply = ccid_bulk_roundtrip(eps, xfr)
        if reply != xfr:
            print(f"FAIL: XfrBlock echo mismatch at iteration {i}", file=sys.stderr)
            return 1

    label = "HIL-05 PASS: stress echo" if args.stress else "HIL-03 PASS: CCID echo"
    print(label)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
