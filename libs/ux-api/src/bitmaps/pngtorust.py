#!/usr/bin/python3

import argparse
import numpy as np
from PIL import Image

def bitflip(data_block, bitwidth=32):
    if bitwidth == 0:
        return data_block

    bytewidth = bitwidth // 8
    bitswapped = bytearray()

    i = 0
    while i < len(data_block):
        data = int.from_bytes(data_block[i:i+bytewidth], byteorder='big', signed=False)
        b = '{:0{width}b}'.format(data, width=bitwidth)
        bitswapped.extend(int(b[::-1], 2).to_bytes(bytewidth, byteorder='big'))
        i = i + bytewidth

    return bytes(bitswapped)


def convert(ifile, ofile):
    # Open the image and convert it to 1-bit mode (black & white)
    im = Image.open(ifile)
    im = im.convert("1")  # pixels will be either 0 (black) or 255 (white)

    # Get pixel data in row-major order
    pixels = list(im.getdata())

    packed = []
    current = 0
    count = 0

    # Process each pixel: assign bit value 1 for white (255) and 0 for black (0)
    for p in pixels:
        bit = 1 if p else 0
        # Place the bit in the current 32-bit integer at the position given by count
        current |= (bit << count)
        count += 1
        if count == 32:
            packed.append(current)
            current = 0
            count = 0

    # If there are remaining pixels that do not fill up a complete 32-bit word,
    # append the last partially-filled integer.
    if count > 0:
        packed.append(current)

    with open(ofile, "w") as output:
        output.write("#![cfg_attr(rustfmt, rustfmt_skip)]\n")
        output.write("pub const BITMAP: [u32; 512] = [")
        for index in range(512):
            if index % 16 == 0:
                output.write("\n")
            b = packed[index]
            if index % 16 < 15:
                output.write("0x{:08x}, ".format(b))
            else:
                output.write("0x{:08x},".format(b))
        output.write("\n];\n")


def main():
    parser = argparse.ArgumentParser(description="Convert BMP to rust header file")
    parser.add_argument(
        "-f", "--file", required=True, help="filename to process", type=str
    )
    parser.add_argument(
        "-o", "--output-file", required=False, help="name of output Rust file", type=str, default="logo.rs"
    )
    args = parser.parse_args()

    ifile = args.file

    convert(ifile, args.output_file)


if __name__ == "__main__":
    main()

