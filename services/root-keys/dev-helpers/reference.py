#!/usr/bin/python3

import argparse
from Crypto.Cipher import AES
from Crypto.Hash import SHA256
from Crypto.Random import get_random_bytes

import binascii
import logging
import sys

"""
Reverse the order of bits in a word that is bitwidth bits wide
"""
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

# assumes a, b are the same length eh?
def xor_bytes(a, b):
    i = 0
    y = bytearray()
    while i < len(a):
        y.extend((a[i] ^ b[i]).to_bytes(1, byteorder='little'))
        i = i + 1

    return bytes(y)

def patcher(bit_in, patch):
    bitstream = bit_in[64:-160]

    if len(patch) > 0:
        # find beginning of type2 area
        position = 0
        type = -1
        command = 0
        while type != 2:
            command = int.from_bytes(bitflip(bitstream[position:position+4]), byteorder='big')
            position = position + 4
            if (command & 0xE0000000) == 0x20000000:
                type = 1
            elif (command & 0xE0000000) == 0x40000000:
                type = 2
            else:
                type = -1
        count = 0x3ffffff & command  # not used, but handy to have around

        ostream = bytearray(bitstream)
        # position now sits at the top of the type2 region
        # apply patches to each frame specified, ignoring the "none" values
        for line in patch:
            d = line[1]
            for index in range(len(d)):
                if d[index].strip() != 'none':
                    data = bitflip(int(d[index],16).to_bytes(4, 'big'))
                    frame_num = int(line[0],16)
                    for b in range(4):
                        stream_pos = position + frame_num * 101 * 4 + index * 4 + b
                        ostream[stream_pos] = data[b]

        return bytes(ostream)
    else:
        return bitstream

def dumpframes(ofile, framestream):
    position = 0
    type = -1

    command = 0
    while type != 2:
        command = int.from_bytes(framestream[position:position+4], byteorder='big')
        position = position + 4
        if (command & 0xE0000000) == 0x20000000:
            type = 1
        elif (command & 0xE0000000) == 0x40000000:
            type = 2
        else:
            type = -1
    count = 0x3ffffff & command
    end = position + count
    framecount = 0

    while position < end:
        ofile.write('0x{:08x},'.format(framecount))
        framecount = framecount + 1
        for i in range(101):
            command = int.from_bytes(framestream[position:position + 4], byteorder='big')
            position = position + 4
            ofile.write(' 0x{:08x},'.format(command))
        ofile.write('\n')


def main():
    logging.basicConfig(stream=sys.stdout, level=logging.DEBUG)
    #ifile = 'bbram1_soc_csr.bin'
    ifile = '../../betrusted_soc_bbram.bin'

    #with open('bbram-test1.nky', "r") as nky:
    with open('dummy.nky', "r") as nky:
        for lines in nky:
            line = lines.split(' ')
            if line[1] == '0':
                nky_key = line[2].rstrip().rstrip(';')
            if line[1] == 'StartCBC':
                nky_iv = line[2].rstrip().rstrip(';')
            if line[1] == 'HMAC':
                nky_hmac = line[2].rstrip().rstrip(';')

    logging.debug("original key: %s", nky_key)
    logging.debug("original iv:   %s", nky_iv)
    logging.debug("original hmac: %s", nky_hmac)

    key_bytes = int(nky_key, 16).to_bytes(32, byteorder='big')

    # open the input file, and recover the plaintext
    with open(ifile, "rb") as f:
        binfile = f.read()
        for i in range(64):
            print(binascii.hexlify(binfile[i*4:((i+1)*4)]))

        # search for structure
        # 0x3001_6004 -> specifies the CBC key
        # 4 words of CBC IV
        # 0x3003_4001 -> ciphertext len
        # 1 word of ciphertext len
        # then ciphertext

        position = 0
        iv_pos = 0
        while position < len(binfile):
            cwd = int.from_bytes(binfile[position:position+4], 'big')
            if cwd == 0x3001_6004:
                iv_pos = position+4
            if cwd == 0x3003_4001:
                break
            position = position + 1

        position = position + 4

        ciphertext_len = 4* int.from_bytes(binfile[position:position+4], 'big')
        logging.debug("ciphertext len: %d", ciphertext_len)
        position = position + 4

        active_area = binfile[position : position+ciphertext_len]
        logging.debug("start of ciphertext: %d", position)

        iv_bytes = bitflip(binfile[iv_pos : iv_pos+0x10])  # note that the IV is embedded in the file
        logging.debug("recovered iv: %s", binascii.hexlify(iv_bytes))

        cipher = AES.new(key_bytes, AES.MODE_CBC, iv_bytes)
        logging.debug("first: %s", binascii.hexlify(bitflip(active_area[:16])))
        plain_bitstream = cipher.decrypt(bitflip(active_area))
        logging.debug("first: %s", binascii.hexlify(plain_bitstream[:16]))

        with open('plain.bin', 'wb') as plain_f:
            plain_f.write(bitflip(plain_bitstream))
        #for i in range(64):
        #    print(binascii.hexlify(bitflip(plain_bitstream[i*4:((i+1)*4)])))
        #print(binascii.hexlify(bitflip(plain_bitstream)))

        logging.debug("raw hmac: %s", binascii.hexlify(plain_bitstream[:64]))

        hmac = xor_bytes(plain_bitstream[:32], plain_bitstream[32:64])
        logging.debug("hmac: %s", binascii.hexlify(hmac))

        logging.debug("plaintext len: %d", len(plain_bitstream))
        logging.debug("initial plaintext: %s", binascii.hexlify(plain_bitstream[:256]))

        h1 = SHA256.new()
        k = 0
        while k < len(plain_bitstream) - 320 - 160:  # HMAC does /not/ cover the whole file, it stops 320 + 160 bytes short of the end
            h1.update(bitflip(plain_bitstream[k:k+16], 32))
            k = k + 16
        h1_digest = h1.digest()
        logging.debug("new digest1                  : %s", binascii.hexlify(h1_digest))
        logging.debug("new digest1 (in stored order): %s", binascii.hexlify(bitflip(h1_digest)))

        footer = int(0x3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A).to_bytes(32, byteorder='big')
        keyed_footer = xor_bytes(footer, hmac)
        logging.debug("hmac (flipped): %s", binascii.hexlify(hmac))
        logging.debug("masked_footer: %s", binascii.hexlify(keyed_footer))
        h2 = SHA256.new()
        h2.update(bitflip(keyed_footer))
        h2.update(bitflip(footer))
        #h2.update(keyed_footer)
        #h2.update(footer)
        h2.update(h1_digest)
        h2_digest = h2.digest()

        logging.debug("new digest2                  : %s", binascii.hexlify(h2_digest))
        logging.debug("new digest2 (in stored order): %s", binascii.hexlify(bitflip(h2_digest)))
        logging.debug("ref digest: %s", binascii.hexlify(plain_bitstream[-32:]))
        logging.debug("ref digest (flipped): %s", binascii.hexlify(bitflip(plain_bitstream[-32:])))
        logging.debug("ref ending: %s", binascii.hexlify(plain_bitstream[-196:]))

if __name__ == "__main__":
    main()