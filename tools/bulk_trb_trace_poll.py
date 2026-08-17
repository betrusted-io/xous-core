#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Poll bulk-TRB lifecycle counters over EP0 vendor control-IN (no UART).

Device must be built with usb-bao1x feature `irq-pending-trace` (dabao-ccid).

Setup packet (matches bao1x-hal::usb::driver):
  bmRequestType = 0xC0  (Device-to-Host | Vendor | Device)
  bRequest      = 0x43
  wValue        = 0
  wIndex        = 0
  wLength       = 32

Response (little-endian):
  u32 out_trb_armed
  u32 out_trb_consumed
  u32 in_trb_armed
  u32 in_trb_consumed
  u32 out_force_prime_calls
  u32 seq
  u16 out_enq_index, out_deq_index
  u16 in_enq_index, in_deq_index

Can run concurrently with tools/irq_pending_trace_poll.py (0x42) — independent
EP0 vendor requests; neither claims an interface, so pcscd can keep CCID.

Does not claim any interface — EP0 only.
Requires: pip install pyusb  (and permission to open 1d50:6197)

This script never creates/modifies system files or escalates privileges.
On EACCES it prints a copy-pasteable sudo suggestion and exits.

Example:
  python3 tools/bulk_trb_trace_poll.py -o bulk-trb-trace.log
  # optional parallel: python3 tools/irq_pending_trace_poll.py -o irq-pending-trace.log
  # in another terminal: watch -n 2 gpg --card-status
"""

from __future__ import annotations

import argparse
import errno
import struct
import sys
import time
from datetime import datetime, timezone

try:
    import usb.core
    import usb.util
except ImportError as exc:
    raise SystemExit("pyusb is required: pip install pyusb") from exc

BAO_VID = 0x1D50
DABAO_PID = 0x6197

# Must match libs/bao1x-hal/src/usb/driver.rs
BM_REQUEST_TYPE = 0xC0
B_REQUEST = 0x43
RESP_LEN = 32
# <6x u32><4x u16>
RESP_FMT = "<IIIIIIHHHH"


def _is_access_denied(err: BaseException) -> bool:
    """True only for permission denied (EACCES), not timeout/stall/not-found."""
    if getattr(err, "errno", None) == errno.EACCES:
        return True
    text = str(err).lower()
    return "access denied" in text or "permission denied" in text or "errno 13" in text


def _exit_permission_denied(vid: int, pid: int) -> None:
    args = " ".join(sys.argv[1:])
    cmd = f"sudo python3 tools/bulk_trb_trace_poll.py {args}".rstrip()
    print(
        f"Permission denied opening {vid:04x}:{pid:04x}.\n"
        "This script will not modify any system files or permissions on its own.\n"
        "If you want to grant it access for this run only, you can choose to\n"
        "re-run the exact same command with sudo:\n"
        f"    {cmd}\n"
        "That is your decision to make — this script will not do it for you,\n"
        "and will not persist any change to your system.",
        file=sys.stderr,
    )
    raise SystemExit(1)


def find_device(vid: int, pid: int, timeout_s: float):
    deadline = time.monotonic() + timeout_s
    while True:
        try:
            dev = usb.core.find(idVendor=vid, idProduct=pid)
        except usb.core.USBError as e:
            if _is_access_denied(e):
                _exit_permission_denied(vid, pid)
            raise
        if dev is not None:
            return dev
        if time.monotonic() >= deadline:
            raise SystemExit(f"device {vid:04x}:{pid:04x} not found within {timeout_s}s")
        time.sleep(0.25)


def read_stats(dev, timeout_ms: int) -> tuple[int, int, int, int, int, int, int, int, int, int]:
    data = dev.ctrl_transfer(
        BM_REQUEST_TYPE,
        B_REQUEST,
        0,
        0,
        RESP_LEN,
        timeout=timeout_ms,
    )
    if len(data) < RESP_LEN:
        raise RuntimeError(f"short response: {len(data)} bytes")
    return struct.unpack_from(RESP_FMT, bytes(data), 0)


def _is_stall(err: BaseException) -> bool:
    """libusb PIPE / STALL on unmatched vendor request (old firmware or wrong bRequest)."""
    text = str(err).lower()
    if "pipe" in text or "stall" in text:
        return True
    errno_v = getattr(err, "errno", None)
    if errno_v in (-9, 32):
        return True
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description="Poll bulk-TRB trace via EP0 vendor IN")
    parser.add_argument("--vid", type=lambda x: int(x, 0), default=BAO_VID)
    parser.add_argument("--pid", type=lambda x: int(x, 0), default=DABAO_PID)
    parser.add_argument("-o", "--output", default="bulk-trb-trace.log")
    parser.add_argument("--interval", type=float, default=0.5, help="seconds between polls")
    parser.add_argument("--timeout-ms", type=int, default=1000)
    parser.add_argument("--wait", type=float, default=30.0, help="seconds to wait for device")
    parser.add_argument(
        "--seq-stall-polls",
        type=int,
        default=20,
        help="mark SEQ_STALL after this many consecutive polls with unchanged seq "
        "(default 20 = ~10s at --interval 0.5)",
    )
    args = parser.parse_args()

    print(
        f"Waiting for {args.vid:04x}:{args.pid:04x}; "
        f"ctrl_transfer bmRequestType=0x{BM_REQUEST_TYPE:02x} bRequest=0x{B_REQUEST:02x}",
        flush=True,
    )
    print(
        "Note: STALL/PIPE before reflash = expected (old image). "
        "STALL after reflash = bug (wrong firmware or 0x43 not wired). "
        "Safe to run alongside irq_pending_trace_poll.py (0x42).",
        flush=True,
    )
    try:
        dev = find_device(args.vid, args.pid, args.wait)
        read_stats(dev, args.timeout_ms)
    except usb.core.USBError as e:
        if _is_access_denied(e):
            _exit_permission_denied(args.vid, args.pid)
        raise

    print(f"Opened bus={dev.bus} addr={dev.address}; logging to {args.output}", flush=True)

    last_seq: int | None = None
    seq_unchanged = 0
    saw_success = False

    with open(args.output, "a", buffering=1) as log:
        log.write(
            f"# start {datetime.now(timezone.utc).isoformat()} "
            f"vid={args.vid:04x} pid={args.pid:04x} interval={args.interval}\n"
        )
        while True:
            ts = datetime.now().isoformat(timespec="milliseconds")
            try:
                (
                    out_armed,
                    out_consumed,
                    in_armed,
                    in_consumed,
                    force_prime,
                    seq,
                    out_enq,
                    out_deq,
                    in_enq,
                    in_deq,
                ) = read_stats(dev, args.timeout_ms)
                saw_success = True
                flags: list[str] = []

                if last_seq is None:
                    flags.append("FIRST")
                    seq_unchanged = 0
                elif seq == last_seq:
                    seq_unchanged += 1
                    if seq_unchanged >= args.seq_stall_polls:
                        flags.append(f"SEQ_STALL(n={seq_unchanged})")
                else:
                    flags.append(f"SEQ_DELTA={seq - last_seq}")
                    seq_unchanged = 0

                last_seq = seq
                out_depth = int(out_enq) - int(out_deq)
                in_depth = int(in_enq) - int(in_deq)

                flag_s = (" " + " ".join(flags)) if flags else ""
                line = (
                    f"{ts} out_armed={out_armed} out_consumed={out_consumed} "
                    f"in_armed={in_armed} in_consumed={in_consumed} "
                    f"force_prime={force_prime} seq={seq} "
                    f"out_depth={out_depth} in_depth={in_depth}{flag_s}"
                )
                print(line, flush=True)
                log.write(line + "\n")
            except usb.core.USBError as e:
                if _is_access_denied(e):
                    _exit_permission_denied(args.vid, args.pid)
                kind = "STALL" if _is_stall(e) else "USBError"
                if kind == "STALL" and saw_success:
                    kind = "STALL_AFTER_OK"
                elif kind == "STALL" and not saw_success:
                    kind = "STALL_BEFORE_OK"
                err = f"{ts} ERROR {kind} {e}"
                print(err, file=sys.stderr, flush=True)
                log.write(err + "\n")
                try:
                    dev = find_device(args.vid, args.pid, timeout_s=5.0)
                except SystemExit:
                    pass
            except Exception as e:
                err = f"{ts} ERROR {e}"
                print(err, file=sys.stderr, flush=True)
                log.write(err + "\n")
            time.sleep(args.interval)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(0)
