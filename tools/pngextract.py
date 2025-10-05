#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import binascii
import sys
import zlib
from datetime import datetime
from pathlib import Path
from typing import Iterator, Tuple

WIDTH = 256
HEIGHT = 240
BYTES_PER_IMAGE = WIDTH * HEIGHT  # 61440


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Extract base64 framebuffers and save as PNGs.")
    p.add_argument("--infile", required=True, type=Path, help="Path to input text file.")
    p.add_argument("--basename", default="image", help='Output base name (default: "image").')
    return p.parse_args()


def png_chunk(chunk_type: bytes, data: bytes) -> bytes:
    length = len(data).to_bytes(4, "big")
    crc = binascii.crc32(chunk_type + data) & 0xFFFFFFFF
    return length + chunk_type + data + crc.to_bytes(4, "big")


def write_png_gray8(out_path: Path, raw_gray8: bytes, width: int, height: int) -> None:
    if len(raw_gray8) != width * height:
        raise ValueError(f"Raw buffer size {len(raw_gray8)} != expected {width*height}")

    scanlines = bytearray()
    row_stride = width
    for y in range(height):
        scanlines.append(0)
        scanlines.extend(raw_gray8[y * row_stride:(y + 1) * row_stride])

    compressed = zlib.compress(bytes(scanlines), level=9)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = png_chunk(
        b"IHDR",
        width.to_bytes(4, "big")
        + height.to_bytes(4, "big")
        + b"\x08"  # bit depth
        + b"\x00"  # grayscale
        + b"\x00"  # compression
        + b"\x00"  # filter
        + b"\x00",  # no interlace
    )
    idat = png_chunk(b"IDAT", compressed)
    iend = png_chunk(b"IEND", b"")
    out_path.write_bytes(sig + ihdr + idat + iend)


def iter_b64_blocks(lines: Iterator[str]) -> Iterator[Tuple[int, str, list[str]]]:
    """
    Yields (block_index, kind, lines)
    kind is either "base" or "orig".
    """
    collecting = False
    buf: list[str] = []
    idx = 0
    kind = "base"
    for line in lines:
        s = line.strip()
        low = s.lower()
        if not collecting and low in ("begin base 64", "begin orig base 64"):
            collecting = True
            buf = []
            idx += 1
            kind = "orig" if "orig" in low else "base"
            continue
        if collecting and (
            (low == "end base 64" and kind == "base")
            or (low == "end orig base 64" and kind == "orig")
        ):
            yield idx, kind, buf
            collecting = False
            buf = []
            continue
        if collecting and s:
            buf.append(s)


def decode_block(lines: list[str]) -> bytes:
    parts: list[bytes] = []
    for ln in lines:
        try:
            parts.append(base64.b64decode(ln, validate=True))
        except binascii.Error:
            parts.append(base64.b64decode(ln))
    return b"".join(parts)


def main() -> int:
    args = parse_args()

    if not args.infile.is_file():
        print(f"Input file not found: {args.infile}", file=sys.stderr)
        return 2

    text = args.infile.read_text(encoding="utf-8", errors="replace").splitlines()
    saved = 0
    now = datetime.now().strftime("%Y%m%dT%H%M%S")

    for i, kind, b64_lines in iter_b64_blocks(iter(text)):
        raw = decode_block(b64_lines)

        if len(raw) != BYTES_PER_IMAGE:
            print(
                f"Block {i} ({kind}): size {len(raw)} bytes != expected {BYTES_PER_IMAGE}. Skipping.",
                file=sys.stderr,
            )
            continue

        serial = f"{i:03d}"
        suffix = "-orig" if kind == "orig" else ""
        filename = f"{args.basename}-{now}-{serial}{suffix}.png"
        out_path = Path.cwd() / filename

        try:
            write_png_gray8(out_path, raw, WIDTH, HEIGHT)
            print(f"Saved {out_path}")
            saved += 1
        except Exception as e:
            print(f"Block {i} ({kind}): failed to write PNG: {e}", file=sys.stderr)

    if saved == 0:
        print("No valid images were saved.", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
