#!/usr/bin/python3

import argparse
import numpy as np

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
    with open(ifile, "rb") as f:
        image = f.read()

        offset = image[0xa] # skip header
        length = len(image) - offset
        if length != 2050:
            print("Image is not a full frame: {}", length)
            exit(1)

        bitmap = []
        for index in range(2048):
            b = image[offset + index]
            bits = []
            for bit in range(8):
                if (b >> (7 - bit)) & 1 == 0:
                    bits += [0]
                else:
                    bits += [1]
            bitmap += bits

        if False:
            # Reshape the flat bitmap into a 2D array (row-normal)
            bitmap_2d = np.array(bitmap).reshape((128, 128))

            # Transpose the 2D array to convert it to column-normal
            transposed_bitmap_2d = bitmap_2d.T

            # Flatten the transposed 2D array back to a 1D list (column-normal)
            column_normal_bitmap = transposed_bitmap_2d.flatten()

            rotated = []
            for index in range(2048):
                bits = column_normal_bitmap[index * 8 : (index + 1) * 8]
                b = 0
                for (i, bit) in enumerate(bits):
                    if bit != 0:
                        b |= 1 << (7-i)
                rotated += [b]
        else:
            rotated = []
            for index in range(2048):
                rotated += [image[offset + index]]

        with open(ofile, "w") as output:
            output.write("#![cfg_attr(rustfmt, rustfmt_skip)]\n")
            output.write("pub const LOGO_MAP: [u8; 2048] = [")
            for index in range(2048):
                if index % 16 == 0:
                    output.write("\n")
                b = rotated[index]
                if index % 16 < 15:
                    output.write("0x{:02x}, ".format(b))
                else:
                    output.write("0x{:02x},".format(b))
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

