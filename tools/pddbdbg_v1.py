#! /usr/bin/env python3
import argparse
import sys

from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV
from Crypto.Hash import SHA512
import binascii

import logging

SYSTEM_BASIS = '.System'
PAGE_SIZE = 4096
VPAGE_SIZE = 4064
MBBB_PAGES = 10
DO_CI_TESTS = True

# build a table mapping all non-printable characters to None
NOPRINT_TRANS_TABLE = {
    i: None for i in range(0, sys.maxunicode + 1) if not chr(i).isprintable()
}

def make_printable(s):
    global NOPRINT_TRANS_TABLE
    """Replace non-printable characters in a string."""

    # the translate method on str removes characters
    # that map to None from the string
    return s.translate(NOPRINT_TRANS_TABLE)

# from https://github.com/wc-duck/pymmh3/blob/master/pymmh3.py
def xrange( a, b, c ):
    return range( a, b, c )
def xencode(x):
    if isinstance(x, bytes) or isinstance(x, bytearray):
        return x
    else:
        return x.encode()

def mm3_hash( key, seed = 0x0 ):
    ''' Implements 32bit murmur3 hash. '''

    key = bytearray( xencode(key) )

    def fmix( h ):
        h ^= h >> 16
        h  = ( h * 0x85ebca6b ) & 0xFFFFFFFF
        h ^= h >> 13
        h  = ( h * 0xc2b2ae35 ) & 0xFFFFFFFF
        h ^= h >> 16
        return h

    length = len( key )
    nblocks = int( length / 4 )

    h1 = seed

    c1 = 0xcc9e2d51
    c2 = 0x1b873593

    # body
    for block_start in xrange( 0, nblocks * 4, 4 ):
        # ??? big endian?
        k1 = key[ block_start + 3 ] << 24 | \
             key[ block_start + 2 ] << 16 | \
             key[ block_start + 1 ] <<  8 | \
             key[ block_start + 0 ]

        k1 = ( c1 * k1 ) & 0xFFFFFFFF
        k1 = ( k1 << 15 | k1 >> 17 ) & 0xFFFFFFFF # inlined ROTL32
        k1 = ( c2 * k1 ) & 0xFFFFFFFF

        h1 ^= k1
        h1  = ( h1 << 13 | h1 >> 19 ) & 0xFFFFFFFF # inlined ROTL32
        h1  = ( h1 * 5 + 0xe6546b64 ) & 0xFFFFFFFF

    # tail
    tail_index = nblocks * 4
    k1 = 0
    tail_size = length & 3

    if tail_size >= 3:
        k1 ^= key[ tail_index + 2 ] << 16
    if tail_size >= 2:
        k1 ^= key[ tail_index + 1 ] << 8
    if tail_size >= 1:
        k1 ^= key[ tail_index + 0 ]

    if tail_size > 0:
        k1  = ( k1 * c1 ) & 0xFFFFFFFF
        k1  = ( k1 << 15 | k1 >> 17 ) & 0xFFFFFFFF # inlined ROTL32
        k1  = ( k1 * c2 ) & 0xFFFFFFFF
        h1 ^= k1

    #finalization
    return fmix( h1 ^ length )

    # weird, this code breaks things compared to the reference Rust implementation
    unsigned_val = fmix( h1 ^ length )
    if unsigned_val & 0x80000000 == 0:
        return unsigned_val
    else:
        return -( (unsigned_val ^ 0xFFFFFFFF) + 1 )

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
    args = parser.parse_args()

    if args.name == None:
        keyfile = './tools/pddb-images/pddb.key'
        imagefile = './tools/pddb-images/pddb.bin'
    else:
        keyfile = './tools/pddb-images/{}.key'.format(args.name)
        imagefile = './tools/pddb-images/{}.bin'.format(args.name)

    if args.dump:
        DO_CI_TESTS = False

    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    keys = {}
    with open(keyfile, 'rb') as key_f:
        raw_key = key_f.read()
        num_keys = int.from_bytes(raw_key[:4], 'little')
        for i in range(num_keys):
            name_all = raw_key[4 + i*96 : 4 + i*96 + 64]
            name_bytes = bytearray(b'')
            for b in name_all:
                if b != 0:
                    name_bytes.append(b)
                else:
                    break
            name = name_bytes.decode('utf8', errors='ignore')
            key = raw_key[4 + i*96 + 64 : 4 + i*96 + 96]
            keys[name] = key

    logging.info("Found basis keys:")
    logging.info(str(keys))

    # tunable parameters for a filesystem
    global MBBB_PAGES
    FSCB_PAGES = 16
    FSCB_LEN_PAGES = 2
    KEY_PAGES = 1
    global PAGE_SIZE
    global VPAGE_SIZE
    MAX_DICTS = 16384

    with open(imagefile, 'rb') as img_f:
        raw_img = img_f.read()
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
                logging.info("Basis {}, key: {}".format(name, key.hex()))
                v2p_table = tables[name][0]
                p2v_table = tables[name][1]
                # v2p_table[0xfe0fe0] = 0x1200* 0x100

                basis_data = bytearray()
                pp_start = v2p_table[VPAGE_SIZE]
                # print("pp_start: {:x}".format(pp_start))
                pp_data = data[pp_start:pp_start + PAGE_SIZE]
                try:
                    pt_data = keycommit_decrypt(key, basis_aad(name), pp_data)
                    basis_data.extend(bytearray(pt_data))
                    logging.debug("decrypted vpage @ {:x} ppage @ {:x}".format(VPAGE_SIZE, v2p_table[VPAGE_SIZE]))
                    # print([hex(x) for x in basis_data[:256]])
                    basis = Basis(basis_data)
                    logging.info(basis.as_str())

                    basis_dicts = {}
                    dicts_found = 0
                    dict_index = 0
                    while dict_index < MAX_DICTS and dicts_found < basis.num_dicts:
                        bdict = BasisDicts(dict_index, v2p_table, data, key, name)
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
                    if args.dump == False:
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

PRINTED_FULL = False
class KeyDescriptor:
    MAX_NAME_LEN = 95
    def __init__(self, record, v2p, disk, key, name):
        # print(record.hex())
        i = 0
        self.start = int.from_bytes(record[i:i+8], 'little')
        i += 8
        self.len = int.from_bytes(record[i:i+8], 'little')
        i += 8
        self.reserved = int.from_bytes(record[i:i+8], 'little')
        i += 8
        self.flags_code = int.from_bytes(record[i:i+4], 'little')
        i += 4
        self.age = int.from_bytes(record[i:i+4], 'little')
        i += 4
        self.name = record[i+1:i+1+record[i]].decode('utf8', errors='ignore')
        if self.flags_code & 1 != 0:
            self.valid = True
        else:
            self.valid = False
        if self.flags_code & 2 != 0:
            self.unresolved = True
        else:
            self.unresolved = False
        if self.valid:
            # print('valid key')
            page_addr = self.start
            self.data = bytearray()
            remaining = self.len
            read_start = self.start % VPAGE_SIZE # reads can start not page-aligned
            while page_addr < self.start + self.len:
                data_base = (page_addr // VPAGE_SIZE) * VPAGE_SIZE
                try:
                    pp_start = v2p[data_base]
                except KeyError:
                    logging.error("key {} is missing data allocation at va {:x}".format(self.name, data_base))
                    self.ci_ok = False
                    raise KeyError
                pp_data = disk[pp_start:pp_start + PAGE_SIZE]
                try:
                    cipher = AES_GCM_SIV(key, pp_data[:12])
                    pt_data = cipher.decrypt(pp_data[12:], basis_aad(name))[4:] # skip the journal, first 4 bytes
                    if read_start + remaining > VPAGE_SIZE:
                        read_end = VPAGE_SIZE
                    else:
                        read_end = read_start + remaining
                    self.data += pt_data[read_start:read_end]
                    bytes_read = read_end - read_start
                    remaining -= bytes_read
                    read_start = (read_start + bytes_read) % VPAGE_SIZE
                    if remaining > 0:
                        assert(read_start == 0)
                except ValueError:
                    logging.error("key: couldn't decrypt vpage @ {:x} ppage @ {:x}".format(page_addr), pp_start)
                page_addr += VPAGE_SIZE
            global DO_CI_TESTS
            if DO_CI_TESTS:
                # CI check -- if it doesn't pass, it doesn't mean we've failed -- could also just be a "normal" record that doesn't have the checksum appended
                check_data = self.data[:-4]
                while len(check_data) % 4 != 0:
                    check_data += bytes([0])
                checksum = mm3_hash(check_data)
                refcheck = int.from_bytes(self.data[len(self.data)-4:], 'little')
                if checksum == refcheck:
                    self.ci_ok = True
                else:
                    self.ci_ok = False
                    logging.error('checksum: {:x}, refchecksum: {:x}\n'.format(checksum, refcheck))
        else:
            # print('invalid key')
            pass


    def as_str(self, indent=''):
        PRINT_LEN = 64
        global PRINTED_FULL
        global DO_CI_TESTS
        desc = ''
        if self.start > 0x7e_fff02_0000:
            desc += indent + 'Start: 0x{:x} (lg)\n'.format(self.start)
        else:
            dict = (self.start - 0x3f_8000_0000) // 0xFE_0000
            pool = ((self.start - 0x3f_8000_0000) - dict * 0xFE_0000) // 0xFE0
            desc += indent + 'Start: 0x{:x} | dict_index {} | pool {}\n'.format(self.start, dict, pool)
        desc += indent + 'Len:   {}/0x{:x}\n'.format(self.len, self.reserved)
        desc += indent + 'Flags: '
        if self.valid:
            desc += 'VALID'
        if self.unresolved:
            desc += indent + 'UNRESOLVED'

        desc += ' | Age: {}'.format(self.age)
        if DO_CI_TESTS:
            desc += ' | CI OK: {}'.format(self.ci_ok)
        desc += '\n'
        if len(self.data) < PRINT_LEN:
            print_len = len(self.data)
            extra = ''
        else:
            if (PRINTED_FULL == False) and (self.ci_ok == False):
                print_len = len(self.data)
                extra = ''
                PRINTED_FULL = True
            else:
                print_len = PRINT_LEN
                extra = '...'
        desc += indent + 'Data (hex): {}{}\n'.format(self.data[:print_len].hex(), extra)
        desc += indent + 'Data (txt): {}{}'.format(make_printable(self.data[:print_len].decode('utf-8', errors='ignore')), extra) + '\n'
        return (desc)


class BasisDicts:
    DICT_VSTRIDE = 0xFE_0000
    KV_STRIDE = 127
    MAX_NAME_LEN = 111
    def __init__(self, index, v2p, disk, key, name):
        global PAGE_SIZE
        global VPAGE_SIZE
        dict_header_vaddr = self.DICT_VSTRIDE * (index + 1)
        self.index = index
        self.vaddr = dict_header_vaddr
        if dict_header_vaddr in v2p:
            self.valid = True
            pp = v2p[dict_header_vaddr]
            self.keys = {}
            try:
                logging.debug('dict decrypt @ pp {:x}, nonce: {}'.format(pp, disk[pp:pp+12].hex()))
                cipher = AES_GCM_SIV(key, disk[pp:pp+12])
                pt_data = cipher.decrypt(disk[pp+12:pp+PAGE_SIZE], basis_aad(name))
                # print('raw pt_data: {}'.format(pt_data[:127].hex()))
                i = 0
                self.journal = int.from_bytes(pt_data[i:i+4], 'little')
                i += 4
                self.flags = int.from_bytes(pt_data[i:i+4], 'little')
                i += 4
                self.age = int.from_bytes(pt_data[i:i+4], 'little')
                i += 4
                self.num_keys = int.from_bytes(pt_data[i:i+4], 'little')
                i += 4
                self.free_key_index = int.from_bytes(pt_data[i:i+4], 'little')
                i += 4
                self.name = pt_data[i+1:i+1+pt_data[i]].decode('utf8', errors='ignore')
                i += BasisDicts.MAX_NAME_LEN
                logging.info("decrypt dict '{}' with {} keys and {} free_key_index".format(self.name, self.num_keys, self.free_key_index))
                logging.debug("dict header len: {}".format(i-4)) # subtract 4 because of the journal
            except ValueError:
                logging.error("basisdicts: couldn't decrypt vpage @ {:x} ppage @ {:x}".format(dict_header_vaddr, v2p[dict_header_vaddr]))

            if self.num_keys > 0:
                keys_found = 0
                keys_tried = 1
                prev_index = 128 # hack to optimize effort by only decrypting a new page when the key index resets
                while (keys_found < self.num_keys) and keys_tried < 131071:
                    keyindex_start_vaddr = self.DICT_VSTRIDE * (index + 1) + (keys_tried * self.KV_STRIDE)
                    #if self.name == 'dict2' and keyindex_start_vaddr < (0x1fc2fa0 + 0xfe0):
                    #    try:
                    #        print('keys_tried {}'.format(keys_tried))
                    #        print('ki_vaddr {:x}'.format((keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE))
                    #        pp = v2p[(keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE]
                    #        print('ki_paddr {:x}'.format(pp))
                    #    except KeyError:
                    #        pass
                    try:
                        pp_start = v2p[(keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE]
                        pp_data = disk[pp_start:pp_start + PAGE_SIZE]
                    except KeyError:
                        # the page wasn't allocated, so let's just assume all the key entries are invalid, and were "tried"
                        keys_tried += VPAGE_SIZE // self.KV_STRIDE
                        continue
                    try:
                        key_index = 4 + keys_tried % (VPAGE_SIZE // self.KV_STRIDE) * self.KV_STRIDE
                        if key_index < prev_index:
                            cipher = AES_GCM_SIV(key, pp_data[:12])
                            pt_data = cipher.decrypt(pp_data[12:], basis_aad(name))
                        prev_index = key_index
                        #if self.name == 'dict2':
                        #    print('key_index {:x}/{}/{:x}/{:x}'.format(key_index, keys_tried, pp, (keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE))
                        #    print('{:x}'.format(v2p[(keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE]))
                        try:
                            maybe_key = KeyDescriptor(pt_data[key_index:key_index + self.KV_STRIDE], v2p, disk, key, name)
                            if maybe_key.valid:
                                self.keys[maybe_key.name] = maybe_key
                                keys_found += 1
                                #if self.name == 'dict2':
                                #    print('found {}: {}'.format(keys_found, maybe_key.name))
                        except KeyError:
                            logging.error("Key @ offset {} is missing data allocation".format(key_index))

                    except ValueError:
                        logging.error("key: couldn't decrypt vpage @ {:x} ppage @ {:x}".format(keyindex_start_vaddr, pp_start))
                    keys_tried += 1
                if keys_found < self.num_keys:
                    logging.error("Expected {} keys but only found {}".format(self.num_keys, keys_found))
                    self.found_all_keys = False
                else:
                    self.found_all_keys = True
                #if self.name == 'dict2':
                #    for v, p in v2p.items():
                #        print('map: v{:16x} | p{:8x}'.format(v, p))

        else:
            self.valid = False

    def as_str(self):
        desc = ''
        desc += '  Name: {} / index {} / vaddr: {:x}\n'.format(self.name, self.index, self.vaddr)
        desc += '  Age: {}\n'.format(self.age)
        desc += '  Flags: {}\n'.format(self.flags)
        desc += '  Key count: {}\n'.format(self.num_keys)
        desc += '  Free key index: {}\n'.format(self.free_key_index)
        desc += '  Keys:\n'
        for (name, key) in self.keys.items():
            desc += '    {}:\n'.format(name)
            desc += key.as_str('       ')

        return desc

    def ci_check(self):
        check_ok = True
        for (name, key) in self.keys.items():
            namecheck = name.split('|')
            if namecheck[1] != self.name:
                logging.warning(" Key/dict mismatch: {}/{}".format(key.name, self.name))
            keylen = int(namecheck[3][3:])
            if keylen > 12:
                if keylen != key.len and keylen + 4 != key.len: # the second case is what happens with the length extension test, it's actually not an error
                    logging.warning(" Key named len vs disk len mismatch: {}/{}".format(keylen, key.len))
            else:
                if key.len != 17 and key.len != 12: # 12 means we weren't extended. But, the test patch data extends a 12-length key to 13, so with the checksum its final length is 17.
                    logging.warning(" Key named len vs disk len mismatch: {}/{} (extension did not work correctly)".format(keylen, key.len))
            if key.ci_ok == False:
                logging.warning(" CI failed on key {}:".format(name))
                logging.warning(key.as_str('  '))
                check_ok = False
        if self.found_all_keys == False:
            logging.warning(' Dictionary was missing keys')
            check_ok = False
        if check_ok:
            logging.info(' Dictionary {} CI check: OK'.format(self.name))
        else:
            logging.error(' Dictionary {} CI check: FAIL'.format(self.name))


class Basis:
    MAX_NAME_LEN = 64
    def __init__(self, i_bytes):
        self.bytes = i_bytes
        i = 0
        self.journal = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.magic = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.version = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.age = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.num_dicts = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.name = i_bytes[i+1:i+1+i_bytes[i]].rstrip(b'\x00').decode('utf8', errors='ignore')
        i += Basis.MAX_NAME_LEN
        #self.prealloc_open_end = int.from_bytes(i_bytes[i:i+8], 'little')
        #i += 8
        #self.dict_ptr = int.from_bytes(i_bytes[i:i+8], 'little')
        #i += 8
        """ rkyv unpacker
        i = 0
        self.journal = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.rkyvpos = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        print("alloc on disk: {}".format(i_bytes[i:i+8].hex()))
        self.prealloc_open_end = int.from_bytes(i_bytes[i:i+8], 'little')
        i += 8
        self.age = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.num_dicts = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.version = int.from_bytes(i_bytes[i:i+2], 'little')
        i += 2
        self.magic = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.name = i_bytes[i:i+Basis.MAX_NAME_LEN].rstrip(b'\x00').decode('utf8', errors='ignore')
        i += Basis.MAX_NAME_LEN
        i += 10 # mysterious padding in the rkyv format!!
        self.dict_ptr = int.from_bytes(i_bytes[i:i+8], 'little')
        i += 8
        """

    def as_str(self):
        desc = ''
        desc += ' Journal rev: {}\n'.format(self.journal)
        #desc += ' Rkyv pos: {}\n'.format(self.rkyvpos)
        desc += ' Magic: {:x}\n'.format(self.magic)
        desc += ' Version: {:x}\n'.format(self.version)
        desc += ' Age: {:x}\n'.format(self.age)
        desc += ' Name: {}\n'.format(self.name)
        #desc += ' Alloc: {:x}\n'.format(self.prealloc_open_end)
        desc += ' NumDicts: {:x}\n'.format(self.num_dicts)
        #desc += ' DictPtr: {:x}\n'.format(self.dict_ptr)
        return desc

def basis_aad(name, version=0x01_01, dna=0):
    name_bytes = bytearray(name, 'utf-8')
    # name_bytes += bytearray(([0] * (Basis.MAX_NAME_LEN - len(name))))
    name_bytes += version.to_bytes(4, 'little')
    name_bytes += dna.to_bytes(8, 'little')

    return name_bytes


class Pte:
    PTE_LEN = 16
    def __init__(self, i_bytes):
        assert(len(i_bytes) == self.PTE_LEN)
        self.bytes = i_bytes

    def len_bytes(self):
        return(self.bytes.len())
    def as_bytes(self):
        return self.bytes

    def addr(self):
        return int.from_bytes(self.bytes[:7], 'little')
    def flags(self):
        if self.bytes[6] == 1:
            return 'CLN'
        elif self.bytes[6] == 2:
            return 'CHK'
        elif self.bytes[6] == 3:
            return 'COK'
        else:
            return 'INV'
    def nonce(self):
        return int.from_bytes(self.bytes[8:12], 'little')
    def checksum(self):
        return int.from_bytes(self.bytes[12:], 'little')

    # checks the checksum
    def is_valid(self):
        return (self.checksum() == mm3_hash(self.bytes[:12], self.nonce()))

    def as_str(self, ppage=None):
        #print('checksum: {:08x}, hash: {:08x}'.format(self.checksum(), mm3_hash(self.bytes[:12], self.nonce())))
        if ppage != None:
            return '{:08x}: a_{:012x} | f_{} | n_{:08x} | c_{:08x}'.format(ppage, self.addr(), self.flags(), self.nonce(), self.checksum())
        else:
            return 'a_{:012x} | f_{} | n_{:08x} | c_{:08x}'.format(self.addr(), self.flags(), self.nonce(), self.checksum())

def find_mbbb(mbbb):
    global MBBB_PAGES
    pages = [mbbb[i:i+PAGE_SIZE] for i in range(0, MBBB_PAGES * PAGE_SIZE, PAGE_SIZE)]
    candidates = []
    for page in pages:
        if bytearray(page[:16]) != bytearray([0xff] * 16):
            candidates.append(page)

    if len(candidates) > 1:
        logging.error("More than one MBBB found, this is an error!")
    else:
        return candidates[0]
    return None

def decode_pagetable(img, entries, keys, mbbb):
    global PAGE_SIZE
    key_table = {}
    for name, key in keys.items():
        logging.debug("Pages for basis {}".format(name))
        logging.debug("key: {}".format(key.hex()))
        cipher = AES.new(key, AES.MODE_ECB)
        pages = [img[i:i+PAGE_SIZE] for i in range(0, entries * Pte.PTE_LEN, PAGE_SIZE)]
        page_num = 0
        v2p_table = {}
        p2v_table = {}
        for page in pages:
            if bytearray(page[:16]) == bytearray([0xff] * 16):
                # decrypt from mbbb
                logging.debug("Found a blank PTE page, falling back to MBBB!")
                maybe_page = find_mbbb(mbbb)
                if maybe_page != None:
                    page = maybe_page
                else:
                    logging.warning("Blank entry in PTE found, but no corresponding MBBB entry found. Filesystem likely corrupted.")
            encrypted_ptes = [page[i:i+Pte.PTE_LEN] for i in range(0, PAGE_SIZE, Pte.PTE_LEN)]
            for pte in encrypted_ptes:
                maybe_pte = Pte(cipher.decrypt(pte))
                if maybe_pte.is_valid():
                    logging.debug(maybe_pte.as_str(page_num))
                    p_addr = page_num * (4096 // Pte.PTE_LEN)
                    if maybe_pte.addr() in v2p_table:
                        logging.warning("duplicate V2P PTE entry, evicting {:x}:{:x}", maybe_pte.addr(), p_addr)
                    v2p_table[maybe_pte.addr()] = p_addr
                    if p_addr in p2v_table:
                        logging.warning("duplicate P2V PTE entry, evicting {:x}:{:x}", p_addr, maybe_pte.addr())
                    p2v_table[p_addr] = maybe_pte.addr()

                page_num += Pte.PTE_LEN
            key_table[name] = [v2p_table, p2v_table]
    return key_table

class PhysPage:
    PP_LEN = 4
    def __init__(self, i_bytes):
        self.pp = int.from_bytes(i_bytes[:4], 'little')

    def page_number(self):
        return self.pp & 0xF_FFFF

    def clean(self):
        if (self.pp & 0x10_0000) != 0:
            return True
        else:
            return False

    def valid(self):
        if (self.pp & 0x20_0000) != 0:
            return True
        else:
            return False

    def space_state(self):
        code = (self.pp >> 22) & 3
        if code == 0:
            return 'FREE'
        elif code == 1:
            return 'MUSE'
        elif code == 2:
            return 'USED'
        else:
            return 'DIRT'

    def journal(self):
        return (self.pp >> 24) & 0xF

    def as_str(self):
        return 'pp_{:05x} | c_{} | v_{} | ss_{} | j_{:02}'.format(self.page_number(), self.clean(), self.valid(), self.space_state(), self.journal())

class Fscb:
    def __init__(self, i_bytes, FASTSPACE_PAGES=2):
        NONCE_LEN = 12
        TAG_LEN = 16
        PP_LEN = 4
        FREE_POOL_LEN = ((4096 * FASTSPACE_PAGES) - (NONCE_LEN + TAG_LEN)) // PP_LEN
        assert(len(i_bytes) == FREE_POOL_LEN * PP_LEN)
        raw_pps = [i_bytes[i:i+PP_LEN] for i in range(0, FREE_POOL_LEN * PP_LEN, PP_LEN)]
        self.free_space = {}
        for raw_pp in raw_pps:
            pp = PhysPage(raw_pp)
            if pp.valid():
                self.free_space[pp.page_number() * 4096] = pp

    def at_phys_addr(self, addr):
        try:
            return self.free_pool[addr]
        except:
            None

    def print(self):
        for pagenum, pp in self.free_space.items():
            logging.debug(pp.as_str())

    def try_replace(self, candidate):
        if candidate.is_valid():
            pp = candidate.get_pp()
            cur_pp = self.free_space[pp.page_number() * 4096]
            if pp.valid() and (cur_pp.journal() < pp.journal()):
                logging.debug("FSCB replace {} /with/ {}".format(cur_pp.as_str(), pp.as_str()))
                self.free_space[pp.page_number() * 4096] = pp
            elif cur_pp.journal() == pp.journal():
                logging.warning("Duplicate journal number found:\n   {} (in table)\n   {} (incoming)".format(cur_pp.as_str(), pp.as_str()))

    # this is the "AAD" used to encrypt the FastSpace
    def aad(version=0x01_01, dna=0):
        return bytearray([46, 70, 97, 115, 116, 83, 112, 97, 99, 101]) + version.to_bytes(4, 'little') + dna.to_bytes(8, 'little')

class SpaceUpdate:
    SU_LEN = 16
    def __init__(self, i_bytes):
        self.nonce = i_bytes[:8]
        self.page_number = PhysPage(i_bytes[8:12])
        self.checksum = int.from_bytes(i_bytes[12:], 'little')
        hash = mm3_hash(i_bytes[:12], int.from_bytes(i_bytes[4:8], 'big'))
        if self.checksum == hash:
            self.valid = True
        else:
            self.valid = False

    def is_valid(self):
        return self.valid

    def get_pp(self):
        return self.page_number

# img is clipped to just the fscb region of the overall disk image
def decode_fscb(img, keys, FSCB_LEN_PAGES=2):
    global SYSTEM_BASIS
    fscb = None
    fscb_count = 0
    if SYSTEM_BASIS in keys:
        key = keys[SYSTEM_BASIS]
        pages = [img[i:i+4096] for i in range(0, len(img), 4096)]
        space_update = []
        fscb_start = None
        pg = 0
        for page in pages:
            if bytearray(page[:32]) == bytearray([0xff] * 32):
                # page is blank
                pass
            elif bytearray(page[:16]) == bytearray([0xff] * 16):
                space_update.append(page[16:])
            else:
                if fscb_start == None:
                    fscb_start = pg
            pg += 1

        if fscb_start:
            logging.debug("Found FSCB at {:x}".format(fscb_start))
            fscb_enc = img[fscb_start * 4096 : (fscb_start + FSCB_LEN_PAGES) * 4096]
            # print("data: {}".format(fscb_enc.hex()))
            try:
                nonce = fscb_enc[:12]
                logging.info("key: {}, nonce: {}".format(key.hex(), nonce.hex()))
                cipher = AES_GCM_SIV(key, nonce)
                logging.info("aad: {}".format(Fscb.aad().hex()))
                logging.info("mac: {}".format(fscb_enc[-16:].hex()))
                #print("data: {}".format(fscb_enc[12:-16].hex()))
                fscb_dec = cipher.decrypt(fscb_enc[12:], Fscb.aad())
                fscb = Fscb(fscb_dec, FASTSPACE_PAGES=FSCB_LEN_PAGES)
            except KeyError:
                logging.error("couldn't decrypt")
                fscb = None

        if fscb != None:
            logging.debug("key: {}".format(key.hex()))
            cipher = AES.new(key, AES.MODE_ECB)
            for update in space_update:
                entries = [update[i:i+SpaceUpdate.SU_LEN] for i in range(0, len(update), SpaceUpdate.SU_LEN)]
                for entry in entries:
                    if entry == bytearray([0xff] * 16):
                        break
                    else:
                        fscb.try_replace(SpaceUpdate(cipher.decrypt(entry)))
                        fscb_count += 1
    logging.info("Total FSCB entries found: {}".format(fscb_count))
    return fscb

if __name__ == "__main__":
    main()
    exit(0)
