#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Host-side CCID transport smoke test (enumeration + bulk echo).

Requires a baosec device running an image built with ccid-openpgp and ccid-echo.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Allow running as tools/ccid_smoke.py or from tools/ccid_hil/
sys.path.insert(0, str(Path(__file__).resolve().parent / "ccid_hil"))

from ccid_usb import (  # noqa: E402
    BAOSEC_PID,
    BAOSEC_VID,
    ccid_bulk_roundtrip,
    find_ccid_device,
    make_get_slot_status,
    make_xfr_block,
    verify_ccid_descriptor,
)


def main() -> int:
    parser = argparse.ArgumentParser(description="CCID USB transport smoke test")
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAOSEC_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=BAOSEC_PID)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--skip-echo", action="store_true")
    args = parser.parse_args()

    print(f"Waiting for CCID device {args.vid:04x}:{args.pid:04x}...")
    eps = find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)
    print("Device enumerated.")

    desc = verify_ccid_descriptor(eps.device)
    print(
        f"CCID descriptor: bcd={desc['bcd_ccid']:04x} "
        f"protocols=0x{desc['dw_protocols']:08x} "
        f"max_msg={desc['max_message_length']}"
    )
    if desc["bcd_ccid"] != 0x0110:
        print("FAIL: unexpected bcdCCID (expected 0x0110)", file=sys.stderr)
        return 1
    if desc["dw_protocols"] != 0x02:
        print("FAIL: expected T=1 protocol bit in dwProtocols", file=sys.stderr)
        return 1

    if args.skip_echo:
        print("PASS (enumeration only)")
        return 0

    frame = make_get_slot_status(seq=1)
    print(f"Sending GetSlotStatus ({len(frame)} bytes)...")
    reply = ccid_bulk_roundtrip(eps, frame)
    if reply != frame:
        print(f"FAIL: echo mismatch\n  sent: {frame.hex()}\n  recv: {reply.hex()}", file=sys.stderr)
        return 1
    print("GetSlotStatus echo OK.")

    payload = bytes(range(32))
    xfr = make_xfr_block(seq=2, payload=payload)
    print(f"Sending XfrBlock ({len(xfr)} bytes)...")
    reply = ccid_bulk_roundtrip(eps, xfr)
    if reply != xfr:
        print(f"FAIL: XfrBlock echo mismatch\n  sent: {xfr.hex()}\n  recv: {reply.hex()}", file=sys.stderr)
        return 1
    print("XfrBlock echo OK.")
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
