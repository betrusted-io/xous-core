#!/bin/bash
# export.sh - run as part of build or manually

set -e
for svg in *.svg; do
    base=$(basename "$svg" .svg)

    # Export at native 128x128 — no scaling = no interpolation
    inkscape "$svg" \
        --export-type=png \
        --export-width=128 \
        --export-height=128 \
        --export-text-to-path \
        -o "/tmp/${base}.png"

    # Hard threshold to pure 1-bit B&W — no dithering
    convert "/tmp/${base}.png" \
        -threshold 50% \
        -monochrome \
        -depth 1 \
        "${base}.png"

    python3 ./pngtorust.py -f "${base}.png" -o "${base}.rs"
done
