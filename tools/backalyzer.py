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
    # this is for an older set of CI tests from the pddbg.py script, that are not set up for this implementation
    set_ci_tests_flag(False)

    parser = argparse.ArgumentParser(description="Debug Backup Images")
    parser.add_argument(
        "-p", "--pin", help="Unlock PIN", type=str,
    )
    parser.add_argument(
        "--backup-key", help="Backup key as BIP-39 words", type=str, default="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
    )
    parser.add_argument(
        "--basis", type=str, help="Extra Bases to unlock, as `name:pass`. Each additional basis requires another --basis separator. Note that : is not legal to use in a Basis name.", action="append", nargs="+"
    )
    parser.add_argument(
        "--loglevel", help="set logging level (INFO/DEBUG/WARNING/ERROR)", type=str, default="INFO",
    )
    parser.add_argument(
        "-d", "--dump", help="Print data records in detail", action="store_true"
    )
    parser.add_argument(
        "--hosted", help="Analyze a hosted mode PDDB image.", action="store_true"
    )
    parser.add_argument(
        "-f", "--file", help="Input file (defaults to backup.pddb; note that hosted is in pddb-images/hosted.bin).", type=str, default="backup.pddb"
    )

    args = parser.parse_args()
    if args.hosted == False and args.pin == None:
        print("Unlock PIN argument is required, please specified with `-p [pin]`")
        exit(0)

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

    with open(args.file, "rb") as backup_file:
        global VERSION
        SYSTEM_BASIS = ".System"
        VERSION = 0x201.to_bytes(4, 'little') # update this manually here if the PDDB version gets bumped :P
        AAD = b"PDDB backup v0.1.0"
        backup = backup_file.read()
        if args.hosted == False:
            PT_HEADER_LEN = 4 + 16 * 4 + 4 + 8 + 64 + 4 + 4 # ignore the pt header
            NONCE_LEN = 12
            CT_LEN = PT_HEADER_LEN + 1024 + 64
            TAG_LEN = 16
            COMMIT_NONCE_LEN = 32
            COMMIT_LEN = 32
            CHECKSUM_LEN = 32
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
            offset += COMMIT_LEN
            check_region = backup[:offset]
            checksum = backup[offset:offset+CHECKSUM_LEN]

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
            backup_version = int.from_bytes(pt_data[i:i+4], 'little')
            logging.info("Backup version: 0x{:08x}".format(backup_version))
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
            checksum_region_len = int.from_bytes(pt_data[i:i+4], 'little') * 4096
            logging.info("Checksum region length: 0x{:x}".format(checksum_region_len))
            i += 4
            total_checksums = int.from_bytes(pt_data[i:i+4], 'little')
            logging.info("Number of checksums, including the header checksum itself: {}".format(total_checksums))
            i += 4
            header_total_size = int.from_bytes(pt_data[i:i+4], 'little')
            logging.info("Header total length in bytes: {}".format(header_total_size))
            i += 36 # reserved
            op = int.from_bytes(pt_data[i:i+4], 'little')
            logging.info("Stored Backup Opcode: {}".format(op))
            i += 8 # padding because align=8

            if backup_version == 0x10001:
                logging.info("Doing hash verification of pt+ct metadata")
                hasher = SHA512.new(truncate="256")
                hasher.update(check_region)
                computed_checksum = hasher.digest()
                if computed_checksum != checksum:
                    print("Header failed hash integrity check!")

                # TODO: sector checks

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
            logging.debug("private_key_enc: {}".format(list(get_key(8, keyrom, 32))))
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
        else:
            dna_int = 0
            user_key = bytes([0] * 32)
            pddb = backup

        pddb_len = len(pddb)
        pddb_size_pages = pddb_len // PAGE_SIZE
        logging.info("Database size: 0x{:x}".format(pddb_len))

        pt_len = pddb_size_pages * 16
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
        keys[SYSTEM_BASIS] = [key_pt, key_data]

        # extract the secret basis keys
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

        mbbb_offset = pddb_size_pages * Pte.PTE_LEN + PAGE_SIZE * KEY_PAGES
        if mbbb_offset & (PAGE_SIZE - 1) != 0:
            mbbb_offset = (mbbb_offset + PAGE_SIZE) & 0xFFFF_F000 # round up to nearest page
        logging.info("MBBB: 0x{:x}".format(mbbb_offset))

        img_index = 0
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
        tables = decode_pagetable(pddb, pddb_size_pages, keys, pddb[mbbb_offset:mbbb_offset + MBBB_PAGES * PAGE_SIZE], dna=dna_int, data=data)

        # iterate through found Bases and print their contents
        for name, key in keys.items():
            if name in tables:
                logging.info("Basis '{}', key_pt: {}, key_data: {}".format(name, key[0].hex(), key[1].hex()))
                v2p_table = tables[name][0]
                p2v_table = tables[name][1]

                basis_data = bytearray()
                pp_start = v2p_table[VPAGE_SIZE]
                pp_data = data[pp_start:pp_start + PAGE_SIZE]
                try:
                    pt_data = keycommit_decrypt(key[1], basis_aad(name, dna=dna_int), pp_data)
                    basis_data.extend(bytearray(pt_data))
                    logging.debug("decrypted vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))
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

                    if args.dump:
                        for bdict in basis_dicts.values():
                            logging.info(bdict.as_str())

                except ValueError:
                    logging.error("couldn't decrypt basis root vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))

if __name__ == "__main__":
    main()
    exit(0)
