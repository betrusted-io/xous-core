#! /usr/bin/env python3
import argparse
from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV
from Crypto.Hash import SHA512
import binascii
from datetime import datetime
import bcrypt
import base64

from bip_utils import (
   Bip39MnemonicValidator, Bip39MnemonicDecoder
)

def keycommit_decrypt(key, aad, pp_data):
    # print([hex(d) for d in pp_data[:32]])
    nonce = pp_data[:12]
    ct = pp_data[12:12+4004]
    kcom_nonce = pp_data[12+4004:12+4004+32]
    kcom = pp_data[12+4004+32:12+4004+32+32]
    mac = pp_data[-16:]

    #print([hex(d) for d in kcom_nonce])
    # print([hex(d) for d in kcom])
    #h_test = SHA512.new(truncate="256")  # just something to make sure our hash functions are sane
    #h_test.update(bytes([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x01]))
    #print(h_test.hexdigest())

    h_enc = SHA512.new(truncate="256")
    h_enc.update(key)
    h_enc.update(bytes([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x01]))
    h_enc.update(kcom_nonce)
    k_enc = h_enc.digest()

    h_com = SHA512.new(truncate="256")
    h_com.update(key)
    h_com.update(bytes([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x02]))
    h_com.update(kcom_nonce)
    k_com_derived = h_com.digest()
    print('kcom_stored:  ' + binascii.hexlify(kcom).decode('utf-8'))
    print('kcom_derived: ' + binascii.hexlify(k_com_derived).decode('utf-8'))

    cipher = AES_GCM_SIV(k_enc, nonce)
    pt_data = cipher.decrypt(ct + mac, aad)
    if k_com_derived != kcom:
        print("basis failed key commit test")
        raise Exception(ValueError)
    return pt_data

def bytes_to_semverstr(b):
    maj = int.from_bytes(b[0:2], 'little')
    min = int.from_bytes(b[2:4], 'little')
    rev = int.from_bytes(b[4:6], 'little')
    extra = int.from_bytes(b[6:8], 'little')
    has_commit = int.from_bytes(b[12:16], 'little')
    if has_commit != 0:
        commit = int.from_bytes(b[8:12], 'little')
        return "v{}.{}.{}-{}-g{:x}".format(maj, min, rev, extra, commit)
    else:
        return "v{}.{}.{}-{}".format(maj, min, rev, extra)

def main():
    parser = argparse.ArgumentParser(description="Debug Backup Images")
    parser.add_argument(
        "--dna", required=False, help="SoC DNA", type=str, default="4cb5ce5458c85c"
    )
    args = parser.parse_args()

    # insert your mnemonic here. This is the "zero-key" mnemonic.
    mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
    # Get if a mnemonic is valid with automatic language detection, return bool
    assert(Bip39MnemonicValidator().IsValid(mnemonic))

    # Like before with automatic language detection
    key = Bip39MnemonicDecoder().Decode(mnemonic)
    print("Using backup key: 0x{}".format(key.hex()))

    with open("backup.pddb", "rb") as backup_file:
        SYSTEM_BASIS = b".System"
        PDDB_VERSION = 0x201.to_bytes(4, 'little') # update this manually here if the PDDB version gets bumped :P
        AAD = b"PDDB backup v0.1.0"
        backup = backup_file.read()
        PT_HEADER_LEN = 4 + 16 * 4 + 4 + 8 + 64 + 4 + 4 # ignore the pt header
        NONCE_LEN = 12
        CT_LEN = PT_HEADER_LEN + 1024 + 64
        TAG_LEN = 16
        COMMIT_NONCE_LEN = 32
        COMMIT_LEN = 32
        offset = PT_HEADER_LEN
        nonce = backup[offset:offset+NONCE_LEN]
        offset += NONCE_LEN
        ct = backup[offset:offset+CT_LEN]
        offset += CT_LEN
        mac = backup[offset:offset+TAG_LEN]
        offset += TAG_LEN
        commit_nonce = backup[offset:offset+COMMIT_NONCE_LEN]
        offset += COMMIT_NONCE_LEN
        commit = backup[offset:offset+COMMIT_LEN]

        h_enc = SHA512.new(truncate="256")
        h_enc.update(key)
        h_enc.update(bytes([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x01]))
        h_enc.update(commit_nonce)
        k_enc = h_enc.digest()

        h_com = SHA512.new(truncate="256")
        h_com.update(key)
        h_com.update(bytes([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x02]))
        h_com.update(commit_nonce)
        k_com_derived = h_com.digest()
        print('kcom_stored:  ' + binascii.hexlify(commit).decode('utf-8'))
        print('kcom_derived: ' + binascii.hexlify(k_com_derived).decode('utf-8'))

        if k_com_derived != commit:
            print("Key commitment is incorrect")
            exit(1)

        cipher = AES_GCM_SIV(k_enc, nonce)
        try:
            pt_data = cipher.decrypt(ct + mac, AAD)
        except:
            print("Ciphertext did not pass AES-GCM-SIV validation")
            exit(1)

        i = 0
        print("Backup version: 0x{:08x}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        print("Xous version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        print("SOC version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        print("EC version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        print("WF200 version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        i += 4 # padding because align=8
        ts = int.from_bytes(pt_data[i:i+8], 'little') / 1000
        print("Timestamp: {} / {}".format(ts, datetime.utcfromtimestamp(ts).strftime('%Y-%m-%d %H:%M:%S')))
        i += 8
        print("Language code: {}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        print("Keyboard layout code: {}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        print("DNA: 0x{:x}".format(int.from_bytes(pt_data[i:i+8], 'little')))
        i += 8
        i += 48 # reserved
        op = int.from_bytes(pt_data[i:i+4], 'little')
        print("Opcode: {}".format(op))
        i += 4 # padding because align=8

        keyrom = pt_data[i:i+1024]
        user_key = keyrom[160:160+32]
        pepper = bytearray(keyrom[992:992+16])
        pepper[0] = pepper[0] ^ 1 # encodes the "boot" password type into the pepper

        # python bcrypt is...trying to be too fancy. we just want a raw function, but this thing
        # wraps stuff up so it's in some format that I guess is "standard" for password files. but why...
        #bcrypt_salt = b'$2b$07$' + base64.b64encode(pepper)
        #boot_pw = input('Enter the boot PIN\n')
        # the boot_pw here needs to be byte-wise inserted into an all-0 array of length 72
        #hashed_pw_str = bcrypt.hashpw(bytes(boot_pw, 'utf-8'), bcrypt_salt)
        #print(hashed_pw_str)
        #print(hashed_pw_str.split(b'$')[3])
        #hashed_pw = base64.b64decode(hashed_pw_str.split(b'$')[3])
        #print("hashed pw: {} len:{}".format(hashed_pw.hex(), len(hashed_pw)))
        # INFO:root_keys::implementation: salt: [149, 12, 9, 231, 191, 61, 190, 215, 117, 183, 104, 89, 14, 26, 168, 74] (services\root-keys\src\implementation.rs:689)
        # INFO:root_keys::implementation: pw: "a" (services\root-keys\src\implementation.rs:690)
        # INFO:root_keys::implementation: hashed_pw: [108, 20, 51, 147, 158, 84, 12, 74, 1, 112, 35, 107, 63, 72, 141, 214, 21, 119, 230, 22, 165, 239, 96, 127] (services\root-keys\src\implementation.rs:692)

        # INFO:root_keys::implementation: salt: [150, 12, 9, 231, 191, 61, 190, 215, 117, 183, 104, 89, 14, 26, 168, 74] (services\root-keys\src\implementation.rs:684)
        # INFO:root_keys::implementation: hashed_pw: [103, 141, 7, 34, 227, 109, 40, 253, 84, 47, 92, 240, 152, 27, 135, 227, 102, 183, 83, 124, 185, 1, 73, 107] (services\root-keys\src\implementation.rs:693)

        # if i could get the 24-byte bcrypt raw password out of this, i'd then send this into a sha512/256 hash,
        # and then XOR that result with the user_key to get the true plaintext user_key

        # this user_key would then be used with the key wrapping algorithm to decrypt the PDDB's wrapped_key_pt and wrapped_key_data.

        pddb = backup[4096:]
        PDDB_A_LEN = 0x620_0000
        pt_len = (PDDB_A_LEN // 0x1000) * 16
        pt = pddb[pt_len]
        static_crypto_data = pddb[pt_len:pt_len + 0x1000]
        scd_ver = int.from_bytes(static_crypto_data[:4], 'little')
        if scd_ver != 2:
            print("Static crypto data has wrong version {}", 2)
            exit(1)
        wrapped_key_pt = static_crypto_data[4:4+40]
        wrapped_key_data = static_crypto_data[4+40:4+40+40]
        salt = static_crypto_data[4+40+40:]

        dna = int(args.dna, 16).to_bytes(8, 'little')
        data_aad = SYSTEM_BASIS + PDDB_VERSION + dna



if __name__ == "__main__":
    main()
    exit(0)
