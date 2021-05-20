#! /usr/bin/env python3

import hashlib
import binascii

m = hashlib.sha512()
m2 = hashlib.new('sha512_256')

# test string comes from the OpenTitan test bench
test_string = b"Every one suspects himself of at least one of the cardinal virtues, and this is mine: I am one of the few honest people that I have ever known"

m.update(test_string)
digest = m.digest()

print("const K_DATA: &'static [u8; {}] = b\"{}\";".format(len(test_string), test_string.decode('utf-8')))
print("const K_EXPECTED_DIGEST: [u8; 64] = [", end='')
i = 0
for byte in digest:
    if (i % 16) == 0:
        print("\n   ", end='')
    print("0x{:02x},".format(byte), end='')
    i = i + 1
print("\n];")

m2.update(test_string)
digest2 = m2.digest()

print("const K_EXPECTED_DIGEST_256: [u8; 32] = [", end='')
i = 0
for byte in digest2:
    if (i % 16) == 0:
        print("\n   ", end='')
    print("0x{:02x},".format(byte), end='')
    i = i + 1
print("\n];")
