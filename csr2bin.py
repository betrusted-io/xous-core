#!/usr/bin/python3

import argparse
import hashlib

def main():
    parser = argparse.ArgumentParser(description="Convert csr.csv to binary image file")
    parser.add_argument(
        "-i", "--input-file", required=True, help="file containing CSV input", type=str
    )
    parser.add_argument(
        "-o", "--output-file", required=True, help="destination file for binary data", type=str
    )
    args = parser.parse_args()

    pad_to = 0x7FC0
    with open(args.input_file, "rb") as ifile:
        with open(args.output_file, "wb") as ofile:
            data = ifile.read() # read in the whole block of CSV data
            
            odata = bytearray()
            odata += len(data).to_bytes(4, 'little')
            odata += data
            padding = bytes([0xff]) * (pad_to - len(data) - 4)
            odata += padding

            hasher = hashlib.sha512()
            hasher.update(odata)
            digest = hasher.digest()
            odata += digest

            ofile.write(odata)
            

if __name__ == "__main__":
    main()
