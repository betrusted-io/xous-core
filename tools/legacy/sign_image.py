#! /usr/bin/env python3
import argparse
import sys
from Crypto.IO import PEM  # counter-intuitively, this is from pycryptodome: `pip3 install pycryptodome` (ubuntu) `pip install pycryptodome` (maybe others?)
# note: if you did the sensible thing and tried to install the crypto or pycrypto libraries first when you hit this error
# you have to uninstall those before putting in pycryptodome: `pip remove crypto`, and then `pip install pycryptodome` (or pip3 on ubuntu)
from nacl.signing import SigningKey # from pynacl package: `pip3 install pynacl`
from nacl.encoding import RawEncoder
import os.path

import binascii
import hashlib

DEVKEY_PATH='../devkey/dev.key'
SIGNER_VERSION=1

def blob_sign(source, output, key, defile=False):
    global SIGNER_VERSION
    with open(source, "rb") as source_f:
        source = list(source_f.read())
        source += int(SIGNER_VERSION).to_bytes(4, 'little') # protect the version number
        source += len(source).to_bytes(4, 'little') # append the length to the image, and sign that

        # NOTE NOTE NOTE
        # can't find a good ASN.1 ED25519 key decoder, just relying on the fact that the last 32 bytes are "always" the private key. always? the private key?
        signing_key = SigningKey(key[-32:], encoder=RawEncoder)

        signature = signing_key.sign(source, encoder=RawEncoder)
        print("signing {} bytes".format(len(source)))
        m = hashlib.sha512()
        m.update(bytearray(source))
        print("hash of {}".format(m.hexdigest()))
        print("signing key: [{}]".format(', '.join(hex(x) for x in key[-32:])))

        with open(output, "wb") as output_f:
            written = 0
            written += output_f.write(int(SIGNER_VERSION).to_bytes(4, 'little')) # version number record - mirrored inside the signed data, too
            written += output_f.write(len(source).to_bytes(4, 'little')) # record the length of the final signed record (which /also/ includes a length)
            written += output_f.write(signature.signature)
            print("signature: [{}]".format(', '.join(hex(x) for x in list(signature.signature))))
            output_f.write(bytearray([0] * (4096 - written))) # pad out to one page beyond
            message = bytearray(signature.message)
            if defile is True:
                print("WARNING: defiling the image. This corrupts the binary and should cause it to fail the signature check.")
                message[16778] ^= 0x1 # flip one bit at some random offset
            output_f.write(message) # the actual signed message

def main():
    global DEVKEY_PATH

    parser = argparse.ArgumentParser(description="Sign binary images for Precursor")
    parser.add_argument(
        "--loader-image", required=False, help="loader image", type=str, nargs='?', metavar=('loader image'), const='../target/riscv32imac-unknown-xous-elf/release/loader_presign.bin'
    )
    parser.add_argument(
        "--kernel-image", required=False, help="kernel image", type=str, nargs='?', metavar=('kernel image'), const='../target/riscv32imac-unknown-xous-elf/release/xous_presign.img'
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
        "--kernel-output", required=False, help="kernel output image", type=str, nargs='?', metavar=('kernel output image'),  const='../target/riscv32imac-unknown-none-elf/release/xous.img'
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
        blob_sign(args.loader_image, loader_output, loader_pkey, defile=args.defile)

    # for now, the kernel signing path is just signing a blob that is the overall binary
    # however, there is some aspiration to sign individual binaries, and to incorporate
    # signatures into the kernel tags. leave this path separate so there's an on-ramp
    # to add these features
    if args.kernel_image and (args.kernel_key is None):
        kernel_key = DEVKEY_PATH
    else:
        kernel_key = args.kernel_key
    if args.kernel_image and (args.kernel_output is None):
        kernel_output = '../target/riscv32imac-unknown-none-elf/release/xous.img'
    else:
        kernel_output = args.kernel_output

    if kernel_key is not None and kernel_key is not DEVKEY_PATH:
        with open(kernel_key) as kernel_f:
            kernel_pem = kernel_f.read()
            try:
                pem = PEM.decode(kernel_pem, None)
            except:
                passphrase = input("Enter kernel key passphrase: ")
                pem = PEM.decode(kernel_pem, passphrase)

            (kernel_pkey, pemtype, enc) = pem
            if pemtype != 'PRIVATE KEY':
                print("PEM type for kernel was not a private key. Aborting.")
                exit(1)
    else:
        kernel_pkey = None

    if kernel_pkey != None:
        blob_sign(args.kernel_image, kernel_output, kernel_pkey, defile=args.defile)

if __name__ == "__main__":
    main()
    exit(0)
