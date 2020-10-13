#!/usr/bin/python3

import argparse

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
        length = len(image)

        with open(ofile, "w") as output:
            output.write("pub const LOGO_MAP: [u32; 11 * 536] = [\n")

            line = 536
            while line > 0:
                horiz = 0
                while horiz < 11:
                    position = offset + (line-1) * 44 + horiz * 4 
                    word = int.from_bytes(image[position:position + 4], byteorder='big')
                    word = int('{:032b}'.format(word)[::-1],2)
                    if horiz == 10:
                        word = (word & 0x0000FFFF);
                    output.write("0x{:08x}, ".format(word ^ 0xFFFFFFFF));
                    horiz = horiz + 1
                line = line - 1
                output.write("\n")
            
            output.write("];");


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
    
