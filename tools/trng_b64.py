#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
from datetime import datetime
from pathlib import Path
import sys
import time
import json
from progressbar.bar import ProgressBar

try:
    import serial  # pyserial
    from serial import SerialException
except Exception as e:
    print("pyserial is required (import serial failed).", file=sys.stderr)
    sys.exit(1)


RO_MARK = "====ROSTART===="
AV_MARK = "====AVSTART===="


def parse_size(text: str) -> int:
    s = text.strip()
    if not s:
        raise ValueError("empty length")
    suffix = s[-1].lower()
    if suffix in ("k", "m", "g"):
        num = float(s[:-1])
        mult = {"k": 1024, "m": 1024**2, "g": 1024**3}[suffix]
        return int(num * mult)
    return int(float(s))


def now_stamp() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        description="Serial capture tool for RO/AVA TRNG base64 streams."
    )
    ap.add_argument("--port", default="/dev/ttyACM0", help="Serial port (default: /dev/ttyACM0)")
    ap.add_argument("--type", choices=["ro", "ava"], default="ro", help='Command type: "ro" or "ava" (default: ro)')
    ap.add_argument("--length", default="50M", help="Bytes to capture, supports k/m/g suffixes. Default: 50M")
    ap.add_argument("--baud", type=int, default=480_000_000, help="Baud rate")
    ap.add_argument("--outdir", type=Path, default=Path("."), help="Output directory (default: .)")
    ap.add_argument("--quiet", action="store_true", help="Suppress console logs, output JSON summary on exit")
    args = ap.parse_args(argv)

    try:
        target_bytes = parse_size(args.length)
    except Exception as e:
        print(f"Invalid --length '{args.length}': {e}", file=sys.stderr)
        return 2

    if target_bytes < 65536: # due to size of read buffer for good performance
        print("Target read length must be at least 64k")
        return 2

    cmd = "trngro\r" if args.type == "ro" else "trngava\r"

    try:
        ser = serial.Serial(
            port=args.port,
            baudrate=args.baud,
            timeout=0.25,
            write_timeout=1.0,
        )
    except SerialException as e:
        print(f"Failed to open serial port {args.port}: {e}", file=sys.stderr)
        return 3

    def log(msg: str):
        if not args.quiet:
            print(msg)

    def send_initiate():
        try:
            ser.write(cmd.encode("ascii"))
            ser.flush()
        except SerialException as e:
            print(f"Write failed: {e}", file=sys.stderr)
            raise

    log(f"Opened {args.port} @ {args.baud} baud")
    log(f"Sending initiate command: {cmd.strip()}")

    send_initiate()

    last_send = time.monotonic()
    start_tag = None
    try:
        while True:
            line_bytes = ser.readline()
            now = time.monotonic()
            if not line_bytes:
                if now - last_send >= 2.0:
                    try:
                        ser.write(b"\n\n")
                        ser.flush()
                    except SerialException:
                        pass
                    send_initiate()
                    last_send = now
                continue

            line = line_bytes.decode("utf-8", errors="replace").strip()
            if not line:
                continue

            if line == RO_MARK:
                start_tag = "ro"
                break
            if line == AV_MARK:
                start_tag = "ava"
                break

            log(f"[pre] {line}")

    except KeyboardInterrupt:
        log("Interrupted before start tag.")
        ser.close()
        return 130

    stamp = now_stamp()
    prefix = "ro" if start_tag == "ro" else "av"
    out_path = (args.outdir / f"{prefix}-{stamp}.bin").resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)
    log(f"Start tag detected: {RO_MARK if start_tag=='ro' else AV_MARK}")
    log(f"Writing to: {out_path}")

    written = 0
    skipped_lines = 0
    exit_code = 0
    progress = ProgressBar(min_value=0, max_value=target_bytes, prefix='Receiving ').start()
    buffer = b""

    try:
        with out_path.open("wb") as fh:
            while written < target_bytes:
                chunk = ser.read(65536) # this is weird - if 4096, decodes fail. A larger buffer allows linux to work...
                if not chunk:
                    continue
                buffer += chunk

                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    raw = line.decode("ascii", errors="ignore").strip("\r\n")
                    if not raw:
                        continue
                    if len(raw) % 4 != 0:
                        skipped_lines += 1
                        log(f"[warn] base64 line length not multiple of 4: {len(raw)} chars")
                        continue

                    if not raw:
                        continue
                    if raw in (RO_MARK, AV_MARK):
                        log(f"[info] Marker repeated: {raw}")
                        continue

                    try:
                        data = base64.b64decode(raw, validate=True)
                    except Exception:
                        skipped_lines += 1
                        log("[warn] invalid base64, skipped")
                        continue
                    if len(data) != 256:
                        skipped_lines += 1
                        log(f"[warn] decoded {len(data)} bytes (expected 256): skipped")
                        log(f"  {raw}")
                        exit(0)
                        # continue

                    remaining = target_bytes - written
                    if len(data) > remaining:
                        fh.write(data[:remaining])
                        written += remaining
                        progress.finish()
                        break
                    else:
                        fh.write(data)
                        written += len(data)
                        progress.update(written)

    except KeyboardInterrupt:
        log(f"Interrupted. Wrote {written} bytes to {out_path}")
        exit_code = 130
    except Exception as e:
        print(f"Error during capture: {e}", file=sys.stderr)
        exit_code = 4

    ser.close()
    summary = {
        "exit_code": exit_code,
        "output_file": str(out_path),
        "bytes_written": written,
        "skipped_lines": skipped_lines,
        "target_bytes": target_bytes,
        "mode": start_tag,
    }

    if args.quiet:
        print(json.dumps(summary))
    else:
        log(f"Done. Wrote {written} bytes to {out_path}. Skipped lines: {skipped_lines}")

    return exit_code


if __name__ == "__main__":
    sys.exit(main())
