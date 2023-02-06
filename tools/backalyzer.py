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
    global MBBB_PAGES
    FSCB_PAGES = 16
    FSCB_LEN_PAGES = 2
    KEY_PAGES = 1
    global PAGE_SIZE
    global VPAGE_SIZE
    MAX_DICTS = 16383
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
    parser.add_argument(
        "-c", "--checksum-only", help="Only perform checks for media errors. Does not authenticate the backup; does not require passwords.", action="store_true"
    )

    args = parser.parse_args()
    if args.hosted == False and args.pin == None and args.checksum_only == False:
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
    # Like before with automatic language detection
    key = bip39_to_bits(mnemonic)
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
            PADDING = 4 # structure is padded to 8-byte boundary
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
            offset += PADDING
            check_region = backup[:offset]
            checksum = backup[offset:offset+CHECKSUM_LEN]

            # Implement backup integrity checking without passwords. The metadata about the
            # backup is "unchecked" because it's not coming from an encrypted region. Full restore
            # routines use the checked version, but the purpose of this flow is to provide
            # users with a way to confirm the integrity of backup files without having to
            # disclose sensitive information.
            i = 0
            unchecked_backup_version = int.from_bytes(backup[i:i+4], 'little')
            i += 96
            unchecked_checksum_region_len = int.from_bytes(backup[i:i+4], 'little') * 4096
            i += 4
            unchecked_total_checksums = int.from_bytes(backup[i:i+4], 'little')
            i += 4
            unchecked_header_total_size = int.from_bytes(backup[i:i+4], 'little')

            if args.checksum_only and (unchecked_backup_version == 0x10001):
                checksum_errors = False
                logging.info("Doing hash verification of pt+ct metadata based on unchecked plaintext parameters")
                hasher = SHA512.new(truncate="256")
                # this is where the backup upcode is located. It should be 1
                # check_region = bytearray(check_region)
                # check_region[144:148] = [1, 0, 0, 0] # should be at 0x90 offset
                hasher.update(check_region)
                computed_checksum = hasher.digest()
                if computed_checksum != checksum:
                    logging.error("Header failed hash integrity check!")
                    logging.error("Calculated: {}".format(computed_checksum.hex()))
                    logging.error("Expected:   {}".format(checksum.hex()))
                    exit(1)
                else:
                    logging.info("Header passed integrity check.")

                if unchecked_total_checksums != 0:
                    raw_checksums = backup[unchecked_header_total_size-(unchecked_total_checksums * 16):unchecked_header_total_size]
                    checksums = [raw_checksums[i:i+16] for i in range(0, len(raw_checksums), 16)]
                    check_block_num = 0
                    while check_block_num < unchecked_total_checksums:
                        hasher = SHA512.new(truncate="256")
                        hasher.update(
                            backup[
                                unchecked_header_total_size + check_block_num * unchecked_checksum_region_len:
                                unchecked_header_total_size + (check_block_num + 1) * unchecked_checksum_region_len
                            ])
                        sum = hasher.digest()
                        if sum[:16] != checksums[check_block_num]:
                            logging.error("Bad checksum on block {} at offset 0x{:x}".format(check_block_num, check_block_num * checksum_region_len))
                            logging.error("  Calculated: {}".format(sum[:16].hex()))
                            logging.error("  Expected:   {}".format(checksums[check_block_num].hex()))
                            checksum_errors = True
                        check_block_num += 1

                    if checksum_errors:
                        logging.error("Media errors were detected! Backup may be unusable.")
                        exit(1)
                    else:
                        logging.info("No media errors detected, {} blocks passed checksum tests".format(unchecked_total_checksums))
                        if args.checksum_only:
                            exit(0)
                else:
                    if args.checksum_only:
                        logging.error("Can't perform checksum verification on backups that do not include checksums")
                        exit(1)
                    else:
                        logging.info("Backup has no checksum block, skipping media integrity checks")
            elif args.checksum_only:
                logging.error("Can't perform checksum verification on backups with a version older than 1.1")
                exit(1)

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
            i += 4
            i += 36 # reserved
            op = int.from_bytes(pt_data[i:i+4], 'little')
            logging.info("Stored Backup Opcode: {}".format(op))
            i += 8 # padding because align=8

            if backup_version == 0x10001:
                checksum_errors = False
                logging.info("Doing hash verification of pt+ct metadata")
                hasher = SHA512.new(truncate="256")
                # this is where the backup upcode is located. It should be 1
                # check_region = bytearray(check_region)
                # check_region[144:148] = [1, 0, 0, 0] # should be at 0x90 offset
                hasher.update(check_region)
                computed_checksum = hasher.digest()
                if computed_checksum != checksum:
                    logging.error("Header failed hash integrity check!")
                    logging.error("Calculated: {}".format(computed_checksum.hex()))
                    logging.error("Expected:   {}".format(checksum.hex()))
                    exit(1)
                else:
                    logging.info("Header passed integrity check.")

                if total_checksums != 0:
                    raw_checksums = backup[header_total_size-(total_checksums * 16):header_total_size]
                    checksums = [raw_checksums[i:i+16] for i in range(0, len(raw_checksums), 16)]
                    check_block_num = 0
                    while check_block_num < total_checksums:
                        hasher = SHA512.new(truncate="256")
                        hasher.update(
                            backup[
                                header_total_size + check_block_num * checksum_region_len:
                                header_total_size + (check_block_num + 1) * checksum_region_len
                            ])
                        sum = hasher.digest()
                        if sum[:16] != checksums[check_block_num]:
                            logging.error("Bad checksum on block {} at offset 0x{:x}".format(check_block_num, check_block_num * checksum_region_len))
                            logging.error("  Calculated: {}".format(sum[:16].hex()))
                            logging.error("  Expected:   {}".format(checksums[check_block_num].hex()))
                            checksum_errors = True
                        check_block_num += 1

                    if checksum_errors:
                        logging.error("Media errors were detected! Backup may be unusable.")
                        exit(1)
                    else:
                        logging.info("No media errors detected, {} blocks passed checksum tests".format(total_checksums))
                        if args.checksum_only:
                            exit(0)
                else:
                    if args.checksum_only:
                        logging.error("Can't perform checksum verification on backups that do not include checksums")
                        exit(1)
                    else:
                        logging.info("Backup has no checksum block, skipping media integrity checks")
            elif args.checksum_only:
                logging.error("Can't perform checksum verification on backups with a version older than 1.1")
                exit(1)

            keyrom = pt_data[i:i+1024]
            keys = extract_keys(keyrom, backup[4096:], args.pin, basis_credentials=basis_credentials)

        pddb = backup[4096:]
        pddb_len = len(pddb)
        pddb_size_pages = pddb_len // PAGE_SIZE

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
