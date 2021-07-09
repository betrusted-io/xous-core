#! /usr/bin/env python3
import argparse
import sys
from Crypto.IO import PEM
from nacl.signing import SigningKey
from nacl.encoding import RawEncoder
import os.path

import binascii

DEVKEY_PATH='../devkey/dev.key'
LOADER_VERSION=1

def loader_sign(source, output, key, defile=False):
    global LOADER_VERSION
    with open(source, "rb") as source_f:
        source = list(source_f.read())
        source += int(LOADER_VERSION).to_bytes(4, 'little') # protect the version number
        source += len(source).to_bytes(4, 'little') # append the length to the image, and sign that

        # NOTE NOTE NOTE
        # can't find a good ASN.1 ED25519 key decoder, just relying on the fact that the last 32 bytes are "always" the private key. always? the private key?
        signing_key = SigningKey(key[-32:], encoder=RawEncoder)

        signature = signing_key.sign(source, encoder=RawEncoder)

        with open(output, "wb") as output_f:
            written = 0
            written += output_f.write(int(LOADER_VERSION).to_bytes(4, 'little')) # version number record - mirrored inside the signed data, too
            written += output_f.write(len(source).to_bytes(4, 'little')) # record the length of the final signed record (which /also/ includes a length)
            written += output_f.write(signature.signature)
            output_f.write(bytearray([0] * (4096 - written))) # pad out to one page beyond
            message = bytearray(signature.message)
            if defile is True:
                print("WARNING: defiling the loader image. This corrupts the binary and should cause it to fail the signature check.")
                message[16778] ^= 0x1 # flip one bit at some random offset
            output_f.write(message) # the actual signed message

def main():
    global DEVKEY_PATH

    parser = argparse.ArgumentParser(description="Sign binary images for Precursor")
    parser.add_argument(
        "--loader-image", required=False, help="loader image", type=str, nargs='?', metavar=('loader image'), const='../target/riscv32imac-unknown-none-elf/release/loader_presign.bin'
    )
    parser.add_argument(
        "--kernel-image", required=False, help="kernel image", type=str, nargs='?', metavar=('kernel image'), const='../target/riscv32imac-unknown-none-elf/release/xous_presign.img'
    )
    parser.add_argument(
        "--loader-key", required=False, help="loader signing key", type=str, nargs='?', metavar=('loader signing key'), const=DEVKEY_PATH
    )
    parser.add_argument(
        "--kernel-key", required=False, help="kernel signing key", type=str, nargs='?', metavar=('kernel signing key'), const=DEVKEY_PATH
    )
    parser.add_argument(
        "--loader-output", required=False, help="loader output image", type=str, nargs='?', metavar=('loader output image'),  const='../target/riscv32imac-unknown-none-elf/release/loader.bin'
    )
    parser.add_argument(
        "--defile", help="patch the resulting image, to create a test file to catch signature failure", default=False, action="store_true"
    )
    args = parser.parse_args()
    if not len(sys.argv) > 1:
        print("No arguments specified, doing nothing. Use --help for more information.")
        exit(1)

    if args.loader_image and (args.loader_key is None):
        loader_key = DEVKEY_PATH
    else:
        loader_key = args.loader_key
    if args.loader_image and (args.loader_output is None):
        loader_output = '../target/riscv32imac-unknown-none-elf/release/loader.bin'
    else:
        loader_output = args.loader_output

    if loader_key is not None and loader_key is not DEVKEY_PATH:
        with open(loader_key) as loader_f:
            loader_pem = loader_f.read()
            try:
                pem = PEM.decode(loader_pem, None)
            except:
                passphrase = input("Enter loader key passphrase: ")
                pem = PEM.decode(loader_pem, passphrase)

            (loader_pkey, pemtype, enc) = pem
            if pemtype != 'PRIVATE KEY':
                print("PEM type for loader was not a private key. Aborting.")
                exit(1)
    else:
        loader_pkey = None

    if loader_pkey != None:
        loader_sign(args.loader_image, loader_output, loader_pkey, defile=args.defile)




    if args.kernel_image and (args.kernel_key is None):
        kernel_key = DEVKEY_PATH
    else:
        kernel_key = args.kernel_key

    if kernel_key is not None and kernel_key is not DEVKEY_PATH:
        kernel_pass = input("Enter loader key passphrase, enter if none: ")
        with open(kernel_key) as kernel_f:
            kernel_pem = kernel_f.read()
            try:
                pem = PEM.decode(kernel_pem, kernel_pass)
            except:
                passphrase = input("Enter kernel key passphrase: ")
                pem = PEM.decode(kernel_pem, passphrase)

            (kernel_pkey, pemtype, enc) = pem
            if pemtype != 'PRIVATE KEY':
                print("PEM type for kernel was not a private key. Aborting.")
                exit(1)
    else:
        kernel_pkey = None

if __name__ == "__main__":
    main()
    exit(0)
