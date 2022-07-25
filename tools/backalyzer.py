#! /usr/bin/env python3
import argparse
import logging

from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV
from Crypto.Hash import SHA512
import binascii
from datetime import datetime
import bcrypt
from cryptography.hazmat.primitives.keywrap import aes_key_unwrap_with_padding
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

from pddbcommon import *

from bip_utils import (
   Bip39MnemonicValidator, Bip39MnemonicDecoder
)

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
    global MBBB_PAGES
    FSCB_PAGES = 16
    FSCB_LEN_PAGES = 2
    KEY_PAGES = 1
    global PAGE_SIZE
    global VPAGE_SIZE
    MAX_DICTS = 16384

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
    parser.add_argument(
        "--loglevel", required=False, help="set logging level (INFO/DEBUG/WARNING/ERROR)", type=str, default="INFO",
    )

    args = parser.parse_args()
    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    basis_credentials = {}
    if args.basis:
        for pair in args.basis:
            credpair = pair[0].split(':', 1)
            if len(credpair) != 2:
                logging.error("Basis credential pair with name {} has a formatting problem, aborting!".format(credpair[0]))
                exit(1)
            basis_credentials[credpair[0]] = credpair[1]

    # insert your mnemonic here. This is the "zero-key" mnemonic.
    mnemonic = args.backup_key
    # Get if a mnemonic is valid with automatic language detection, return bool
    assert(Bip39MnemonicValidator().IsValid(mnemonic))

    # Like before with automatic language detection
    key = Bip39MnemonicDecoder().Decode(mnemonic)
    logging.debug("Using backup key: 0x{}".format(key.hex()))

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
        logging.debug('kcom_stored:  ' + binascii.hexlify(commit).decode('utf-8'))
        logging.debug('kcom_derived: ' + binascii.hexlify(k_com_derived).decode('utf-8'))

        if k_com_derived != commit:
            logging.error("Key commitment is incorrect")
            exit(1)

        cipher = AES_GCM_SIV(k_enc, nonce)
        try:
            pt_data = cipher.decrypt(ct + mac, AAD)
        except:
            logging.error("Ciphertext did not pass AES-GCM-SIV validation")
            exit(1)

        i = 0
        logging.info("Backup version: 0x{:08x}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        logging.info("Xous version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        logging.info("SOC version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        logging.info("EC version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        logging.info("WF200 version: {}".format(bytes_to_semverstr(pt_data[i:i+16])))
        i += 16
        i += 4 # padding because align=8
        ts = int.from_bytes(pt_data[i:i+8], 'little') / 1000
        logging.info("Timestamp: {} / {}".format(ts, datetime.utcfromtimestamp(ts).strftime('%Y-%m-%d %H:%M:%S')))
        i += 8
        logging.info("Language code: {}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        logging.info("Keyboard layout code: {}".format(int.from_bytes(pt_data[i:i+4], 'little')))
        i += 4
        logging.info("DNA: 0x{:x}".format(int.from_bytes(pt_data[i:i+8], 'little')))
        dna = pt_data[i:i+8]
        dna_int = int.from_bytes(dna, 'little')
        i += 8
        i += 48 # reserved
        op = int.from_bytes(pt_data[i:i+4], 'little')
        logging.info("Stored Backup Opcode: {}".format(op))
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
        pw_len += 1 # null terminate, so even the null password is one character long
        bcrypter = bcrypt.BCrypt()
        # logging.debug("{}".format(boot_pw_array[:pw_len]))
        logging.debug("user_key_enc: {}".format(list(user_key_enc)))
        logging.debug("salt: {}".format(list(pepper)))
        hashed_pw = bcrypter.crypt_raw(boot_pw_array[:pw_len], pepper, 7)
        logging.debug("hashed_pw: {}".format(list(hashed_pw)))
        hasher = SHA512.new(truncate="256")
        hasher.update(hashed_pw)
        user_pw = hasher.digest()

        user_key = []
        for (a, b) in zip(user_key_enc, user_pw):
            user_key += [a ^ b]
        logging.debug("user_key: {}".format(user_key))

        rollback_limit = 255 - int.from_bytes(keyrom[254 * 4 : 254 * 4 + 4], 'little')
        logging.info("rollback limit: {}".format(rollback_limit))
        for i in range(rollback_limit):
            hasher = SHA512.new(truncate="256")
            hasher.update(bytes(user_key))
            user_key = hasher.digest()

        logging.debug("hashed_key: {}".format(list(user_key)))

        pddb = backup[4096:]
        PDDB_A_LEN = 0x620_0000
        pt_len = (PDDB_A_LEN // 0x1000) * 16
        static_crypto_data = pddb[pt_len:pt_len + 0x1000]
        scd_ver = int.from_bytes(static_crypto_data[:4], 'little')
        if scd_ver != 2:
            logging.error("Static crypto data has wrong version {}", 2)
            exit(1)

        wrapped_key_pt = static_crypto_data[4:4+40]
        wrapped_key_data = static_crypto_data[4+40:4+40+40]
        pddb_salt = static_crypto_data[4+40+40:]

        # extract the .System key
        key_pt = aes_key_unwrap_with_padding(bytes(user_key), bytes(wrapped_key_pt))
        key_data = aes_key_unwrap_with_padding(bytes(user_key), bytes(wrapped_key_data))

        logging.debug("key_pt {}".format(key_pt))
        logging.debug("key_data {}".format(key_data))
        keys = {}
        keys['.System'] = [key_pt, key_data]


        for name, pw in basis_credentials.items():
            bname_copy = [0]*64
            plaintext_pw = [0]*73
            i = 0
            for c in list(name.encode('utf-8')):
                bname_copy[i] = c
                i += 1
            pw_len = 0
            for c in list(pw.encode('utf-8')):
                plaintext_pw[pw_len] = c
                pw_len += 1
            pw_len += 1 # null terminate
            # print(bname_copy)
            # print(plaintext_pw)
            hasher = SHA512.new(truncate="256")
            hasher.update(pddb_salt[32:])
            hasher.update(bytes(bname_copy))
            hasher.update(bytes(plaintext_pw))
            derived_salt = hasher.digest()

            bcrypter = bcrypt.BCrypt()
            hashed_pw = bcrypter.crypt_raw(plaintext_pw[:pw_len], derived_salt[:16], 7)
            hkdf = HKDF(algorithm=hashes.SHA256(), length=32, salt=pddb_salt[:32], info=b"pddb page table key")
            pt_key = hkdf.derive(hashed_pw)
            hkdf = HKDF(algorithm=hashes.SHA256(), length=32, salt=pddb_salt[:32], info=b"pddb data key")
            data_key = hkdf.derive(hashed_pw)
            keys[name] = [pt_key, data_key]

        # data_aad = SYSTEM_BASIS + PDDB_VERSION + dna

        # now that we have the credentials, extract the baseline image
        pddb_len = len(pddb)
        pddb_size_pages = pddb_len // PAGE_SIZE
        logging.info("Database size: 0x{:x}".format(pddb_len))

        mbbb_offset = pddb_size_pages * Pte.PTE_LEN + PAGE_SIZE * KEY_PAGES
        if mbbb_offset & (PAGE_SIZE - 1) != 0:
            mbbb_offset = (mbbb_offset + PAGE_SIZE) & 0xFFFF_F000 # round up to nearest page
        logging.info("MBBB: 0x{:x}".format(mbbb_offset))

        img_index = 0
        tables = decode_pagetable(pddb, pddb_size_pages, keys, pddb[mbbb_offset:mbbb_offset + MBBB_PAGES * PAGE_SIZE])
        img_index += pddb_size_pages * Pte.PTE_LEN
        if img_index & (PAGE_SIZE - 1) != 0:
            img_index = (img_index + PAGE_SIZE) & 0xFFFF_F000

        rawkeys = pddb[img_index : img_index + PAGE_SIZE * KEY_PAGES]
        logging.debug("Keys: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * KEY_PAGES

        mbbb = pddb[img_index : img_index + PAGE_SIZE * MBBB_PAGES]
        logging.debug("MBBB check: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * MBBB_PAGES

        fscb = decode_fscb(pddb[img_index: img_index + PAGE_SIZE * FSCB_PAGES], keys, FSCB_LEN_PAGES=FSCB_LEN_PAGES, dna=dna_int)
        logging.debug("FSCB: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * FSCB_PAGES

        logging.debug("Data: 0x{:x}".format(img_index))
        data = pddb[img_index:]

        for name, key in keys.items():
            if name in tables:
                logging.info("Basis '{}', key_pt: {}, key_data: {}".format(name, key[0].hex(), key[1].hex()))
                v2p_table = tables[name][0]
                p2v_table = tables[name][1]
                # v2p_table[0xfe0fe0] = 0x1200* 0x100

                basis_data = bytearray()
                pp_start = v2p_table[VPAGE_SIZE]
                # print("pp_start: {:x}".format(pp_start))
                pp_data = data[pp_start:pp_start + PAGE_SIZE]
                try:
                    pt_data = keycommit_decrypt(key[1], basis_aad(name, dna=dna_int), pp_data)
                    basis_data.extend(bytearray(pt_data))
                    logging.debug("decrypted vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))
                    # print([hex(x) for x in basis_data[:256]])
                    basis = Basis(basis_data)
                    logging.info(basis.as_str())

                    basis_dicts = {}
                    dicts_found = 0
                    dict_index = 0
                    while dict_index < MAX_DICTS and dicts_found < basis.num_dicts:
                        bdict = BasisDicts(dict_index, v2p_table, data, key[1], name, dna=dna_int)
                        if bdict.valid:
                            basis_dicts[bdict.name] = bdict
                            dicts_found += 1
                        dict_index += 1
                    if dicts_found != basis.num_dicts:
                        logging.error("Expected {} dictionaries, only found {}; searched {}".format(basis.num_dicts, dicts_found, dict_index))

                    logging.info(" Dictionaries: ")
                    for bdict in basis_dicts.values():
                        logging.info(bdict.as_str())

                    for bdict in basis_dicts.values():
                        logging.info("==================================================================")
                        logging.info("Dict {}".format(bdict.as_str()))

                except ValueError:
                    logging.error("couldn't decrypt basis root vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))





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
