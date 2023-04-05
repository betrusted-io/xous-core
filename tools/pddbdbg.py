#! /usr/bin/env python3
import argparse
import logging

from pddbcommon import *

def main():
    global DO_CI_TESTS
    parser = argparse.ArgumentParser(description="Debug PDDB Images")
    parser.add_argument(
        "--name", required=False, help="pddb disk image root name", type=str, nargs='?', metavar=('name'), const='./pddb'
    )
    parser.add_argument(
        "--loglevel", required=False, help="set logging level (INFO/DEBUG/WARNING/ERROR)", type=str, default="INFO",
    )
    parser.add_argument(
        "--dump", required=False, help="Only dump the image, skip the automated CI checks", action="store_true"
    )
    parser.add_argument(
        "--renode", required=False, help="Override flex-size settings and read a Renode bin file", action="store_true"
    )
    parser.add_argument(
        "--smalldb", required=False, help="Override default size of PDDB and set it to 4MiB (for images built to that size only)", action="store_true"
    )
    parser.add_argument(
        "--basis", type=str, help="Extra Bases to unlock, as `name:pass`. Each additional basis requires another --basis separator. Note that : is not legal to use in a Basis name.", action="append", nargs="+"
    )
    parser.add_argument(
        "-p", "--pin", help="Unlock PIN", type=str, default='a'
    )
    parser.add_argument(
        "--ci", help="Run in CI test mode. Requires all keys to have a specific checksum structure for automated checking.", action="store_true"
    )
    args = parser.parse_args()

    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if args.dump:
        numeric_level = getattr(logging, 'DEBUG', None)
    else:
        numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    set_ci_tests_flag(args.ci)

    if args.renode is True:
        keyfile = "./emulation/renode-keybox.bin"
        imagefile = "./tools/pddb-images/renode.bin"
        basis_credentials = []
        if args.basis:
            for pair in args.basis:
                credpair = pair[0].split(':', 1)
                if len(credpair) != 2:
                    logging.error("Basis credential pair with name {} has a formatting problem, aborting!".format(credpair[0]))
                    exit(1)
                basis_credentials += [credpair]

        with open(keyfile, 'rb') as key_f:
            keyrom = key_f.read()
            with open(imagefile, 'rb') as img_f:
                raw_img = img_f.read()
                if args.smalldb:
                    raw_img = raw_img[0x01D80000:0x1D8_0000 + 1024 * 1024 * 4]
                else:
                    raw_img = raw_img[0x01D80000:0x07F80000]
                keys = extract_keys(keyrom, raw_img, args.pin, basis_credentials)
    else:
        if args.name == None:
            keyfile = './tools/pddb-images/pddb.key'
            imagefile = './tools/pddb-images/pddb.bin'
        else:
            keyfile = './tools/pddb-images/{}.key'.format(args.name)
            imagefile = './tools/pddb-images/{}.bin'.format(args.name)

        if args.dump:
            DO_CI_TESTS = False

        keys = {}
        with open(keyfile, 'rb') as key_f:
            raw_key = key_f.read()
            num_keys = int.from_bytes(raw_key[:4], 'little')
            for i in range(num_keys):
                name_all = raw_key[4 + i*128 : 4 + i*128 + 64]
                name_bytes = bytearray(b'')
                for b in name_all:
                    if b != 0:
                        name_bytes.append(b)
                    else:
                        break
                name = name_bytes.decode('utf8', errors='ignore')
                key_data = raw_key[4 + i*128 + 64 : 4 + i*128 + 96]
                key_pt = raw_key[4 + i*128 + 96 : 4 + i*128 + 128]
                keys[name] = [key_pt, key_data]

    logging.info("Found basis keys (pt, data):")
    logging.info(str(keys))

    # tunable parameters for a filesystem
    global MBBB_PAGES
    FSCB_PAGES = 16
    FSCB_LEN_PAGES = 2
    KEY_PAGES = 1
    global PAGE_SIZE
    global VPAGE_SIZE
    MAX_DICTS = 16383

    with open(imagefile, 'rb') as img_f:
        raw_img = img_f.read()
        if args.renode:
            if args.smalldb:
                raw_img = raw_img[0x01D80000:0x1D8_0000 + 1024 * 1024 * 4]
            else:
                raw_img = raw_img[0x01D80000:0x07F80000]
        pddb_len = len(raw_img)
        pddb_size_pages = pddb_len // PAGE_SIZE
        logging.info("Disk size: 0x{:x}".format(pddb_len))

        mbbb_offset = pddb_size_pages * Pte.PTE_LEN + PAGE_SIZE * KEY_PAGES
        if mbbb_offset & (PAGE_SIZE - 1) != 0:
            mbbb_offset = (mbbb_offset + PAGE_SIZE) & 0xFFFF_F000 # round up to nearest page
        logging.info("MBBB: 0x{:x}".format(mbbb_offset))

        img_index = 0
        tables = decode_pagetable(raw_img, pddb_size_pages, keys, raw_img[mbbb_offset:mbbb_offset + MBBB_PAGES * PAGE_SIZE])
        img_index += pddb_size_pages * Pte.PTE_LEN
        if img_index & (PAGE_SIZE - 1) != 0:
            img_index = (img_index + PAGE_SIZE) & 0xFFFF_F000

        rawkeys = raw_img[img_index : img_index + PAGE_SIZE * KEY_PAGES]
        logging.debug("Keys: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * KEY_PAGES

        mbbb = raw_img[img_index : img_index + PAGE_SIZE * MBBB_PAGES]
        logging.debug("MBBB check: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * MBBB_PAGES

        fscb = decode_fscb(raw_img[img_index: img_index + PAGE_SIZE * FSCB_PAGES], keys, FSCB_LEN_PAGES=FSCB_LEN_PAGES)
        logging.debug("FSCB: 0x{:x}".format(img_index))
        img_index += PAGE_SIZE * FSCB_PAGES

        logging.debug("Data: 0x{:x}".format(img_index))
        data = raw_img[img_index:]

        for name, key in keys.items():
            if name in tables:
                logging.info("Basis {}, key_pt: {}, key_data: {}".format(name, key[0].hex(), key[1].hex()))
                v2p_table = tables[name][0]
                p2v_table = tables[name][1]
                # v2p_table[0xfe0fe0] = 0x1200* 0x100

                basis_data = bytearray()
                pp_start = v2p_table[VPAGE_SIZE]
                # print("pp_start: {:x}".format(pp_start))
                pp_data = data[pp_start:pp_start + PAGE_SIZE]
                try:
                    pt_data = keycommit_decrypt(key[1], basis_aad(name), pp_data)
                    basis_data.extend(bytearray(pt_data))
                    logging.debug("decrypted vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))
                    # print([hex(x) for x in basis_data[:256]])
                    basis = Basis(basis_data)
                    logging.info(basis.as_str())

                    basis_dicts = {}
                    dicts_found = 0
                    dict_index = 0
                    while dict_index < MAX_DICTS and dicts_found < basis.num_dicts:
                        bdict = BasisDicts(dict_index, v2p_table, data, key[1], name)
                        if bdict.valid:
                            basis_dicts[bdict.name] = bdict
                            dicts_found += 1
                        dict_index += 1
                    if dicts_found != basis.num_dicts:
                        logging.error("Expected {} dictionaries, only found {}; searched {}".format(basis.num_dicts, dicts_found, dict_index))
                        found_all_dicts = False
                    else:
                        found_all_dicts = True

                    logging.debug(" Dictionaries: ")
                    for bdict in basis_dicts.values():
                        logging.debug(bdict.as_str())

                    #d2 = basis_dicts["dict2"]
                    #logging.info("listing {} keys".format(len(d2.keys)))
                    #for key in d2.keys:
                    #    logging.info("{}".format(key))
                    if args.ci == True:
                        logging.info("CI checks:")
                        for bdict in basis_dicts.values():
                            bdict.ci_check()
                        if found_all_dicts:
                            logging.info("All dicts were found.") # this message is searched for in CI, don't change it
                        else:
                            logging.error("Missing dictionaries, something is wrong.")
                    else:
                        for bdict in basis_dicts.values():
                            logging.info("==================================================================")
                            logging.info("Dict {}".format(bdict.as_str()))

                except ValueError:
                    logging.error("couldn't decrypt basis root vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))

if __name__ == "__main__":
    main()
    exit(0)
