#! /usr/bin/env python3
import argparse
from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV
from Crypto.Hash import SHA512
import binascii
from datetime import datetime
import bcrypt
from cryptography.hazmat.primitives.keywrap import aes_key_unwrap_with_padding, aes_key_wrap_with_padding

from pddbcommon import *

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

def get_key(index, keyrom, length):
    ret = []
    for offset in range(length // 4):
        word = int.from_bytes(keyrom[(index + offset) * 4: (index + offset) * 4 + 4], 'big')
        ret += list(word.to_bytes(4, 'little'))
    return ret

def main():
    parser = argparse.ArgumentParser(description="Debug Backup Images")
    parser.add_argument(
        "-p", "--pin", required=True, help="unlock PIN", type=str,
    )
    parser.add_argument(
        "--backup-key", required=False, help="Backup key as BIP-39 words", type=str, default="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
    )
    parser.add_argument(
        "--basis", required=False, type=str, help="Extra Bases to unlock, as `name:pass`. Each additional basis requires another --basis separator. Note that : is not legal to use in a Basis name.", action="append", nargs="+"
    )

    args = parser.parse_args()

    print(args.basis)

    # insert your mnemonic here. This is the "zero-key" mnemonic.
    mnemonic = args.backup_key
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
        dna = pt_data[i:i+8]
        i += 8
        i += 48 # reserved
        op = int.from_bytes(pt_data[i:i+4], 'little')
        print("Opcode: {}".format(op))
        i += 8 # padding because align=8

        keyrom = pt_data[i:i+1024]
        user_key_enc = get_key(40, keyrom, 32)
        pepper = get_key(248, keyrom, 16)
        pepper[0] = pepper[0] ^ 1 # encodes the "boot" password type into the pepper

        # acquire and massage the password so that we can decrypt the encrypted user key
        boot_pw = args.pin
        boot_pw_array = [0] * 73
        pw_len = 0
        for b in bytes(boot_pw.encode('utf-8')):
            boot_pw_array[pw_len] = b
            pw_len += 1
        pw_len += 1
        bcrypter = bcrypt.BCrypt()
        print("{}".format(boot_pw_array[:pw_len]))
        print("user_key_enc: {}".format(list(user_key_enc)))
        print("salt: {}".format(list(pepper)))
        hashed_pw = bcrypter.crypt_raw(boot_pw_array[:pw_len], pepper, 7)
        print("hashed_pw: {}".format(list(hashed_pw)))
        hasher = SHA512.new(truncate="256")
        hasher.update(hashed_pw)
        user_pw = hasher.digest()

        user_key = []
        for (a, b) in zip(user_key_enc, user_pw):
            user_key += [a ^ b]
        print("user_key: {}".format(user_key))

        rollback_limit = 255 - int.from_bytes(keyrom[254 * 4 : 254 * 4 + 4], 'little')
        print("rollback limit: {}".format(rollback_limit))
        for i in range(rollback_limit):
            hasher = SHA512.new(truncate="256")
            hasher.update(bytes(user_key))
            user_key = hasher.digest()

        print("hashed_key: {}".format(list(user_key)))

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
        pddb_salt = static_crypto_data[4+40+40:]

        print(len(wrapped_key_pt))
        print("decap input: {}".format(list(wrapped_key_pt)))

        # inverse test
        to_wrap = [5, 34, 117, 252, 137, 19, 29, 131, 64, 99, 195, 103, 228, 67, 165, 53, 179, 207, 169, 87, 232, 122, 52, 219, 202, 54, 210, 9, 233, 198, 38, 34]
        wrap_key = [113, 227, 211, 48, 222, 183, 93, 94, 212, 86, 127, 203, 43, 125, 76, 117, 10, 249, 161, 96, 84, 206, 90, 170, 26, 207, 205, 203, 122, 177, 77, 171]
        wrapped_key = aes_key_wrap_with_padding(bytes(wrap_key), bytes(to_wrap))
        print("wrapped_key: {}".format(list(wrapped_key)))

        key_pt = aes_key_unwrap_with_padding(bytes(user_key), bytes(wrapped_key_pt))
        key_data = aes_key_unwrap_with_padding(bytes(user_key), bytes(wrapped_key_data))

        print("key_pt {}".format(key_pt))
        print("key_data {}".format(key_data))

        data_aad = SYSTEM_BASIS + PDDB_VERSION + dna

        # INFO:root_keys::implementation: pw: a, salt: [187, 174, 50, 20, 242, 60, 232, 34, 102, 172, 228, 51, 131, 21, 250, 2] (services\root-keys\src\implementation.rs:698)
        # INFO:root_keys::implementation: hashed_pw: [115, 198, 244, 53, 109, 213, 137, 82, 182, 170, 10, 46, 17, 117, 175, 42, 179, 60, 9, 238, 104, 66, 199, 143] (services\root-keys\src\implementation.rs:700)
        # INFO:root_keys::implementation: bcrypt cost: 7 time: 666ms (services\root-keys\src\implementation.rs:702)
        # INFO:root_keys::implementation: root user key_enc: [8, 40, 241, 188, 179, 66, 205, 83, 237, 99, 54, 179, 131, 178, 105, 178, 163, 131, 208, 202, 167, 153, 226, 63, 255, 245, 63, 43, 10, 43, 102, 193] (services\root-keys\src\implementation.rs:405)
        # INFO:root_keys::implementation: root user pw: [7, 166, 166, 123, 234, 15, 26, 164, 93, 8, 110, 52, 136, 211, 111, 187, 12, 106, 243, 154, 226, 207, 203, 28, 89, 6, 95, 110, 102, 108, 221, 239] (services\root-keys\src\implementation.rs:406)
        # INFO:root_keys::implementation: root user key: [15, 142, 87, 199, 89, 77, 215, 247, 176, 107, 88, 135, 11, 97, 6, 9, 175, 233, 35, 80, 69, 86, 41, 35, 166, 243, 96, 69, 108, 71, 187, 46] (services\root-keys\src\implementation.rs:423)
        # INFO:root_keys::implementation: root user key (anti-rollback): [113, 227, 211, 48, 222, 183, 93, 94, 212, 86, 127, 203, 43, 125, 76, 117, 10, 249, 161, 96, 84, 206, 90, 170, 26, 207, 205, 203, 122, 177, 77, 171] (services\root-keys\src\implementation.rs:427)
        # INFO:root_keys::implementation: decap key: [113, 227, 211, 48, 222, 183, 93, 94, 212, 86, 127, 203, 43, 125, 76, 117, 10, 249, 161, 96, 84, 206, 90, 170, 26, 207, 205, 203, 122, 177, 77, 171] (services\root-keys\src\implementation.rs:447)
        # INFO:root_keys::implementation: decap input: [23, 79, 17, 217, 14, 128, 111, 119, 202, 244, 176, 30, 97, 197, 122, 201, 185, 36, 64, 229, 230, 66, 53, 107, 135, 71, 77, 9, 107, 201, 141, 217, 142, 232, 62, 64, 4, 39, 89, 138] / len 40 expected_len 32 (services\root-keys\src\implementation.rs:448)
        # INFO:root_keys::implementation: uwrapped: [5, 34, 117, 252, 137, 19, 29, 131, 64, 99, 195, 103, 228, 67, 165, 53, 179, 207, 169, 87, 232, 122, 52, 219, 202, 54, 210, 9, 233, 198, 38, 34] (services\root-keys\src\implementation.rs:451)
        # INFO:root_keys::implementation: root user key_enc: [8, 40, 241, 188, 179, 66, 205, 83, 237, 99, 54, 179, 131, 178, 105, 178, 163, 131, 208, 202, 167, 153, 226, 63, 255, 245, 63, 43, 10, 43, 102, 193] (services\root-keys\src\implementation.rs:405)
        # INFO:root_keys::implementation: root user pw: [7, 166, 166, 123, 234, 15, 26, 164, 93, 8, 110, 52, 136, 211, 111, 187, 12, 106, 243, 154, 226, 207, 203, 28, 89, 6, 95, 110, 102, 108, 221, 239] (services\root-keys\src\implementation.rs:406)
        # INFO:root_keys::implementation: root user key: [15, 142, 87, 199, 89, 77, 215, 247, 176, 107, 88, 135, 11, 97, 6, 9, 175, 233, 35, 80, 69, 86, 41, 35, 166, 243, 96, 69, 108, 71, 187, 46] (services\root-keys\src\implementation.rs:423)
        # INFO:root_keys::implementation: root user key (anti-rollback): [113, 227, 211, 48, 222, 183, 93, 94, 212, 86, 127, 203, 43, 125, 76, 117, 10, 249, 161, 96, 84, 206, 90, 170, 26, 207, 205, 203, 122, 177, 77, 171] (services\root-keys\src\implementation.rs:427)
        # INFO:root_keys::implementation: decap key: [113, 227, 211, 48, 222, 183, 93, 94, 212, 86, 127, 203, 43, 125, 76, 117, 10, 249, 161, 96, 84, 206, 90, 170, 26, 207, 205, 203, 122, 177, 77, 171] (services\root-keys\src\implementation.rs:447)
        # INFO:root_keys::implementation: decap input: [150, 23, 244, 163, 140, 35, 103, 135, 2, 199, 211, 225, 18, 89, 202, 168, 22, 229, 21, 183, 75, 217, 25, 164, 134, 193, 60, 181, 200, 247, 29, 113, 107, 112, 210, 100, 59, 244, 231, 135] / len 40 expected_len 32 (services\root-keys\src\implementation.rs:448)
        # INFO:root_keys::implementation: uwrapped: [115, 16, 142, 156, 135, 208, 18, 140, 194, 122, 179, 87, 211, 176, 49, 32, 249, 16, 131, 133, 46, 219, 81, 125, 238, 63, 26, 127, 163, 45, 88, 16] (services\root-keys\src\implementation.rs:451)

        # python bcrypt is...trying to be too fancy. we just want a raw function, but this thing
        # wraps stuff up so it's in some format that I guess is "standard" for password files. but why...
        #bcrypt_salt = b'$2b$07$' + base64.b64encode(pepper)
        # the boot_pw here needs to be byte-wise inserted into an all-0 array of length 72
        #hashed_pw_str = bcrypt.hashpw(bytes(boot_pw, 'utf-8'), bcrypt_salt)

        #print(hashed_pw_str)
        #print(hashed_pw_str.split(b'$')[3])
        #hashed_pw = base64.b64decode(hashed_pw_str.split(b'$')[3])
        #print("hashed pw: {} len:{}".format(hashed_pw.hex(), len(hashed_pw)))

        # INFO:pddb::backend::hw: creating salt (services\pddb\src\backend\hw.rs:2247)
        # INFO:pddb::backend::hw: salt: [121, 176, 9, 67, 182, 238, 202, 42, 169, 251, 25, 48, 238, 22, 232, 180] (services\pddb\src\backend\hw.rs:2267)
        # INFO:pddb::backend::hw: password: "test" (services\pddb\src\backend\hw.rs:2268)
        # INFO:pddb::backend::bcrypt: cost 7 (services\pddb\src\backend\bcrypt.rs:47)
        # INFO:pddb::backend::bcrypt: salt [121, 176, 9, 67, 182, 238, 202, 42, 169, 251, 25, 48, 238, 22, 232, 180] (services\pddb\src\backend\bcrypt.rs:48)
        # INFO:pddb::backend::bcrypt: pt_copy [116, 101, 115, 116, 0] (services\pddb\src\backend\bcrypt.rs:49)
        # INFO:pddb::backend::bcrypt: output [127, 123, 245, 19, 160, 172, 201, 121, 134, 177, 237, 252, 187, 34, 13, 176, 107, 185, 24, 63, 89, 28, 74, 207] (services\pddb\src\backend\bcrypt.rs:79)
        # INFO:pddb::backend::hw: hashed_password: [127, 123, 245, 19, 160, 172, 201, 121, 134, 177, 237, 252, 187, 34, 13, 176, 107, 185, 24, 63, 89, 28, 74, 207] (services\pddb\src\backend\hw.rs:2270)
        # INFO:pddb::backend::hw: derived bcrypt password in 747ms (services\pddb\src\backend\hw.rs:2272)

        # INFO:root_keys::implementation: salt: [149, 12, 9, 231, 191, 61, 190, 215, 117, 183, 104, 89, 14, 26, 168, 74] (services\root-keys\src\implementation.rs:689)
        # INFO:root_keys::implementation: pw: "a" (services\root-keys\src\implementation.rs:690)
        # INFO:root_keys::implementation: hashed_pw: [108, 20, 51, 147, 158, 84, 12, 74, 1, 112, 35, 107, 63, 72, 141, 214, 21, 119, 230, 22, 165, 239, 96, 127] (services\root-keys\src\implementation.rs:692)

        # INFO:root_keys::implementation: salt: [150, 12, 9, 231, 191, 61, 190, 215, 117, 183, 104, 89, 14, 26, 168, 74] (services\root-keys\src\implementation.rs:684)
        # INFO:root_keys::implementation: hashed_pw: [103, 141, 7, 34, 227, 109, 40, 253, 84, 47, 92, 240, 152, 27, 135, 227, 102, 183, 83, 124, 185, 1, 73, 107] (services\root-keys\src\implementation.rs:693)

        # if i could get the 24-byte bcrypt raw password out of this, i'd then send this into a sha512/256 hash,
        # and then XOR that result with the user_key to get the true plaintext user_key

        # this user_key would then be used with the key wrapping algorithm to decrypt the PDDB's wrapped_key_pt and wrapped_key_data.

if __name__ == "__main__":
    main()
    exit(0)
