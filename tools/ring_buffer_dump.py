#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Dump the USB flight-recorder ring buffer over EP0 (post-mortem).

Device must be built with usb-bao1x feature `irq-pending-trace` (dabao-ccid).

Setup packet (matches bao1x-hal::usb::driver):
  bmRequestType = 0xC0  (Device-to-Host | Vendor | Device)
  bRequest      = 0x45
  wValue        = chunk_index (0 .. CHUNK_COUNT-1)
  wIndex        = 0
  wLength       = 256

Per-chunk response (little-endian):
  0..4    write_seq (u32)   — total events ever written
  4..8    capacity (u32)    — ring size (256)
  8..12   chunk_index (u32)
  12..16  n_entries (u32)   — entries in this chunk (<=20)
  16..    n_entries x UsbFlightEvent (12 bytes each):
            0..4  tick (u32)
            4..8  value (u32)
            8     source (u8)
            9..12 pad

Fetch all chunks, sort by tick, print chronologically with tick deltas.

Intended use: once after a failure / recovery / replug — not continuous polling.

Requires: pip install pyusb  (and permission to open 1d50:6197)

Example:
  python3 tools/ring_buffer_dump.py
  python3 tools/ring_buffer_dump.py -o flight-ring.log --last 40
"""

from __future__ import annotations

import argparse
import errno
import struct
import sys
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
BREQUEST = 0x45
RESP_LEN = 256
CHUNK_HEADER_LEN = 16
EVENT_SIZE = 12
ENTRIES_PER_CHUNK = 20
CHUNK_COUNT = 13
RING_CAP = 256

SOURCE_NAMES = {
    0: "composite_handler_entry",
    1: "composite_handler_exit",
    2: "lock_acquired",
    3: "lock_contended",
    4: "out_trb_consumed",
    5: "in_trb_consumed",
    6: "force_prime_call",
    7: "ev_pending_raw",
    8: "ev_enable_raw",
    9: "main_loop_raw_status",
    10: "main_loop_usbsts",
    11: "out_trb_armed",
    12: "in_trb_armed",
    13: "force_prime_synthetic_retire",
    0xFF: "empty_slot",
}


def _suggest_sudo(argv: list[str]) -> None:
    args = " ".join(argv[1:])
    cmd = f"sudo python3 tools/ring_buffer_dump.py {args}".rstrip()
    print(
        "Permission denied opening the USB device.\n"
        f"Re-run with sudo, e.g.:\n  {cmd}",
        file=sys.stderr,
    )


def open_device(vid: int, pid: int):
    try:
        dev = usb.core.find(idVendor=vid, idProduct=pid)
    except usb.core.USBError as exc:
        if getattr(exc, "errno", None) == errno.EACCES or "Access denied" in str(exc):
            _suggest_sudo(sys.argv)
            raise SystemExit(1) from exc
        raise
    if dev is None:
        raise SystemExit(f"No device found for {vid:04x}:{pid:04x}")
    return dev


def fetch_chunk(dev, chunk_index: int) -> bytes:
    try:
        return bytes(
            dev.ctrl_transfer(
                BM_REQUEST_TYPE,
                BREQUEST,
                wValue=chunk_index,
                wIndex=0,
                data_or_wLength=RESP_LEN,
                timeout=2000,
            )
        )
    except usb.core.USBError as exc:
        if getattr(exc, "errno", None) == errno.EACCES or "Access denied" in str(exc):
            _suggest_sudo(sys.argv)
            raise SystemExit(1) from exc
        raise


def parse_chunk(data: bytes) -> tuple[int, int, int, int, list[tuple[int, int, int]]]:
    if len(data) < CHUNK_HEADER_LEN:
        raise ValueError(f"chunk too short: {len(data)} bytes")
    write_seq, capacity, chunk_index, n_entries = struct.unpack_from("<IIII", data, 0)
    events: list[tuple[int, int, int]] = []
    for i in range(n_entries):
        off = CHUNK_HEADER_LEN + i * EVENT_SIZE
        if off + EVENT_SIZE > len(data):
            break
        tick, value, source = struct.unpack_from("<IIB", data, off)
        events.append((tick, source, value))
    return write_seq, capacity, chunk_index, n_entries, events


def format_value(source: int, value: int) -> str:
    if source in (7, 8, 10):
        return f"0x{value:08x}"
    if source == 9:
        pending = value & 0xFFFF
        enable = (value >> 16) & 0xFFFF
        return f"pending=0x{pending:04x} enable=0x{enable:04x} (raw=0x{value:08x})"
    if source in (11, 12, 13):
        enq = value & 0xFFFF
        deq = (value >> 16) & 0xFFFF
        return f"enq={enq} deq={deq} (raw=0x{value:08x})"
    if value == 0:
        return "0"
    return f"0x{value:08x}"


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Dump USB flight-recorder ring (EP0 0x45) after a failure/recovery. "
            "Chunk 0 freezes a device-side snapshot; fetch chunk 0 first."
        ),
    )
    parser.add_argument("-o", "--output", help="Also write decoded lines to this file")
    parser.add_argument(
        "--last",
        type=int,
        default=0,
        help="Print only the last N events (0 = all)",
    )
    parser.add_argument("--vid", type=lambda s: int(s, 0), default=BAO_VID)
    parser.add_argument("--pid", type=lambda s: int(s, 0), default=DABAO_PID)
    args = parser.parse_args()

    dev = open_device(args.vid, args.pid)

    all_events: list[tuple[int, int, int]] = []
    # Chunk 0 freezes the ring on-device; its write_seq is the snapshot seq.
    write_seq = 0
    capacity = RING_CAP
    for ci in range(CHUNK_COUNT):
        raw = fetch_chunk(dev, ci)
        chunk_seq, capacity, chunk_index, n_entries, events = parse_chunk(raw)
        if ci == 0:
            write_seq = chunk_seq
        elif chunk_seq != write_seq:
            print(
                f"warning: chunk {ci} write_seq={chunk_seq} != freeze seq={write_seq}",
                file=sys.stderr,
            )
        if chunk_index != ci:
            print(
                f"warning: chunk {ci} echoed chunk_index={chunk_index}",
                file=sys.stderr,
            )
        all_events.extend(events)

    # Drop unwritten slots. Before first wrap, empty slots still have tick=0 and
    # would pass `tick < write_seq`; firmware marks them source=0xFF (and older
    # images used source=0 with value=0 — treat both as empty when write_seq < cap).
    filtered: list[tuple[int, int, int]] = []
    for tick, source, value in all_events:
        if source == 0xFF:
            continue
        if write_seq <= capacity and tick == 0 and source == 0 and value == 0:
            # Pre-sentinel firmware empty slot (ambiguous with real entry@0; rare).
            continue
        if write_seq > capacity:
            if not (write_seq - capacity <= tick < write_seq):
                continue
        else:
            if not (tick < write_seq):
                continue
        filtered.append((tick, source, value))
    live = filtered
    live.sort(key=lambda e: e[0])

    if args.last > 0:
        live = live[-args.last :]

    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    header = (
        f"# ring_buffer_dump {ts} write_seq={write_seq} capacity={capacity} "
        f"events={len(live)} (freeze snapshot)"
    )
    lines = [header]
    prev_tick: int | None = None
    for tick, source, value in live:
        delta = 0 if prev_tick is None else int(tick) - int(prev_tick)
        prev_tick = tick
        name = SOURCE_NAMES.get(source, f"unknown_{source}")
        lines.append(
            f"tick={tick:<10} delta={delta:<6} src={source:2} {name:<28} "
            f"value={format_value(source, value)}"
        )

    text = "\n".join(lines) + "\n"
    sys.stdout.write(text)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(text)
        print(f"# wrote {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
