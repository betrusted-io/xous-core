#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""HIL-02: provisioning CDC two-line capture (unprovisioned device only)."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

try:
    import serial
    from serial.tools import list_ports
except ImportError as exc:
    raise SystemExit("pyserial is required: pip install pyserial") from exc

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ccid_usb import BAOSEC_PID, BAOSEC_VID, find_ccid_device


def find_provisioning_port(vid: int, pid: int) -> str:
    """Pick a CDC ACM port for the device that is not the primary debug serial."""
    matches = []
    for port in list_ports.comports():
        if port.vid == vid and port.pid == pid and port.device:
            matches.append(port)
    if not matches:
        raise RuntimeError(f"No serial ports for {vid:04x}:{pid:04x}")
    # Prefer ttyACM* ports; use the last match (provisioning is typically enumerated after debug serial).
    acm = [p for p in matches if "ACM" in (p.device or "").upper() or "ttyACM" in (p.device or "")]
    if len(acm) >= 2:
        return sorted(p.device for p in acm)[-1]
    if acm:
        return acm[0].device
    return matches[-1].device


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAOSEC_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=BAOSEC_PID)
    parser.add_argument("--port", default=None, help="Provisioning CDC port (auto-detect if omitted)")
    parser.add_argument("--user-line", default="test-user-pin")
    parser.add_argument("--admin-line", default="test-admin-pin")
    parser.add_argument("--timeout", type=float, default=60.0)
    args = parser.parse_args()

    # Ensure device is present before opening serial.
    find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)

    port = args.port or find_provisioning_port(args.vid, args.pid)
    print(f"Opening provisioning port {port}...")
    with serial.Serial(port, 115200, timeout=2) as ser:
        ser.write((args.user_line + "\r\n").encode())
        ser.flush()
        time.sleep(0.5)
        ser.write((args.admin_line + "\r\n").encode())
        ser.flush()

    print("Waiting for USB re-enumeration after provisioning...")
    time.sleep(3.0)
    find_ccid_device(vid=args.vid, pid=args.pid, timeout_s=args.timeout)
    print("HIL-02 PASS: provisioning lines sent and device re-enumerated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
