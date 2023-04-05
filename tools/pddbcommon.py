import sys
from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV
from Crypto.Hash import SHA512
from Crypto.Hash import SHA256
import binascii
import bcrypt
from cryptography.hazmat.primitives.keywrap import aes_key_unwrap_with_padding
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
import re

import logging

SYSTEM_BASIS = '.System'
PAGE_SIZE = 4096
VPAGE_SIZE = 4064
MBBB_PAGES = 10
DO_CI_TESTS = True
VERSION = 0x02_01
DNA=0
#DNA=0x1_0000_0000

# build a table mapping all non-printable characters to None
NOPRINT_TRANS_TABLE = {
    i: None for i in range(0, sys.maxunicode + 1) if not chr(i).isprintable()
}

def set_ci_tests_flag(setting):
    global DO_CI_TESTS
    DO_CI_TESTS = setting

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
    logging.debug('kcom_stored:  ' + binascii.hexlify(kcom).decode('utf-8'))
    logging.debug('kcom_derived: ' + binascii.hexlify(k_com_derived).decode('utf-8'))

    cipher = AES_GCM_SIV(k_enc, nonce)
    pt_data = cipher.decrypt(ct + mac, aad)
    if k_com_derived != kcom:
        logging.error("basis failed key commit test")
        raise Exception(ValueError)
    else:
        logging.debug("basis passed commitment test")
    return pt_data

def basis_aad(name, version=VERSION, dna=DNA):
    name_bytes = bytearray(name, 'utf-8')
    # name_bytes += bytearray(([0] * (Basis.MAX_NAME_LEN - len(name))))
    name_bytes += version.to_bytes(4, 'little')
    name_bytes += dna.to_bytes(8, 'little')

    return name_bytes

PRINTED_FULL = False
class KeyDescriptor:
    MAX_NAME_LEN = 95
    def __init__(self, record, v2p, disk, key, name, dna=DNA):
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
                    pt_data = cipher.decrypt(pp_data[12:], basis_aad(name, dna=dna))[4:] # skip the journal, first 4 bytes
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
                    logging.error("key: couldn't decrypt vpage @ {:x} ppage @ {:x} data {}".format(page_addr, pp_start, pp_data.hex()))
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
            if DO_CI_TESTS and (PRINTED_FULL == False) and (self.ci_ok == False):
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
    def __init__(self, index, v2p, disk, key, name, dna=DNA):
        global PAGE_SIZE
        global VPAGE_SIZE
        dict_header_vaddr = self.DICT_VSTRIDE * (index + 1)
        self.index = index
        self.vaddr = dict_header_vaddr
        self.num_keys = 0
        if dict_header_vaddr in v2p:
            self.valid = True
            pp = v2p[dict_header_vaddr]
            self.keys = {}
            try:
                logging.debug('dict decrypt @ pp {:x} va {:x}, nonce: {}'.format(pp, dict_header_vaddr, disk[pp:pp+12].hex()))
                cipher = AES_GCM_SIV(key, disk[pp:pp+12])
                pt_data = cipher.decrypt(disk[pp+12:pp+PAGE_SIZE], basis_aad(name, dna=dna))
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
                logging.error("\n") # make some whitespace so this stands out in the logs
                logging.error("**** basisdicts: encountered an invalid dict root record. Data loss may have occurred!")
                logging.error("**** couldn't decrypt vpage @ {:x} ppage @ {:x} in basis {}, aad {}".format(dict_header_vaddr, v2p[dict_header_vaddr], name, basis_aad(name, dna=dna).hex()))
                logging.error("******  partial dump: {}...{} len {}\n".format(disk[pp:pp + 48].hex(), disk[pp + 4048:pp + 4096].hex(), len(disk[pp:pp + 4096])))
                self.valid = False

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
                            pt_data = cipher.decrypt(pp_data[12:], basis_aad(name, dna=dna))
                        prev_index = key_index
                        #if self.name == 'dict2':
                        #    print('key_index {:x}/{}/{:x}/{:x}'.format(key_index, keys_tried, pp, (keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE))
                        #    print('{:x}'.format(v2p[(keyindex_start_vaddr // VPAGE_SIZE) * VPAGE_SIZE]))
                        try:
                            maybe_key = KeyDescriptor(pt_data[key_index:key_index + self.KV_STRIDE], v2p, disk, key, name, dna=dna)
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
        return int.from_bytes(self.bytes[:7], 'little') * VPAGE_SIZE
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
    elif len(candidates) == 1:
        return candidates[0]
    else:
        return None

def decode_pagetable(img, entries, keys, mbbb, dna=DNA, data=None):
    global PAGE_SIZE
    key_table = {}
    for name, key in keys.items():
        logging.debug("Pages for basis '{}'".format(name))
        logging.debug("key: {}".format(key[0].hex()))
        cipher = AES.new(key[0], AES.MODE_ECB)
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
                    logging.debug(maybe_pte.as_str(page_num // Pte.PTE_LEN))
                    p_addr = page_num * (4096 // Pte.PTE_LEN)
                    if maybe_pte.addr() in v2p_table:
                        logging.info("duplicate V2P PTE entry {:x}:{:x}<->{:x}".format(maybe_pte.addr(), v2p_table[maybe_pte.addr()], p_addr))
                        if data == None:
                            logging.warn("No data section passed, ignoring")
                        else:
                            prev_data = data[v2p_table[maybe_pte.addr()]:v2p_table[maybe_pte.addr()] + PAGE_SIZE]
                            new_data = data[p_addr:p_addr + PAGE_SIZE]
                            if maybe_pte.addr() == 0xFE0:
                                # it's a root page table
                                logging.debug("Try to resolve PTE conflict")
                                try:
                                    prev = keycommit_decrypt(key[1], basis_aad(name, dna=dna), prev_data)
                                except ValueError:
                                    logging.error("Previously indexed PTE is bogus, using new entry without checking")
                                    p2v_table[p_addr] = maybe_pte.addr()
                                    continue
                                try:
                                    new = keycommit_decrypt(key[1], basis_aad(name, dna=dna), new_data)
                                except ValueError:
                                    logging.error("Suggested new PTE is invalid. Retaining existing mapping")
                                    continue
                                prev_journal = int.from_bytes(prev[:4], 'little')
                                new_journal = int.from_bytes(new[:4], 'little')
                            else:
                                # it's data
                                logging.debug("Try to resolve data conflict")
                                prev_cipher = AES_GCM_SIV(key[1], prev_data[:12])
                                try:
                                    prev_data = prev_cipher.decrypt(prev_data[12:], basis_aad(name, dna=dna))
                                except ValueError:
                                    logging.error("Previously indexed data is bogus, using new entry without checking")
                                    p2v_table[p_addr] = maybe_pte.addr()
                                    continue
                                new_cipher = AES_GCM_SIV(key[1], new_data[:12])
                                try:
                                    new_data = new_cipher.decrypt(new_data[12:], basis_aad(name, dna=dna))
                                except ValueError:
                                    logging.error("Suggested new data is invalid. Retaining existing mapping")
                                    continue
                                prev_journal = int.from_bytes(prev_data[:4], 'little')
                                new_journal = int.from_bytes(new_data[:4], 'little')
                            if new_journal > prev_journal:
                                logging.info("New entry has journal {}, prev is {}; evicting prev".format(new_journal, prev_journal))
                                p2v_table[p_addr] = maybe_pte.addr()
                                continue
                            elif new_journal == prev_journal:
                                logging.warn("Journal numbers conflict; arbitrarily sticking with the first entry found")
                            else:
                                logging.info("New entry has journal {}, prev is {}; keeping prev".format(new_journal, prev_journal))
                                continue
                    else:
                        v2p_table[maybe_pte.addr()] = p_addr
                    if p_addr in p2v_table:
                        logging.warning("duplicate P2V PTE entry, evicting {:x}:{:x}".format(p_addr, p2v_table[p_addr]))
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

    def len(self):
        return len(self.free_space)

    def print(self):
        for pagenum, pp in self.free_space.items():
            logging.debug(pp.as_str())

    def try_replace(self, candidate):
        if candidate.is_valid():
            pp = candidate.get_pp()
            try:
                cur_pp = self.free_space[pp.page_number() * 4096]
            except KeyError:
                logging.info("SpaceUpdate with no FSCB entry; this is normal after the FSCB is regenerated: {}".format(pp.as_str()))
                return
            if pp.valid() and (cur_pp.journal() < pp.journal()):
                logging.debug("FSCB replace {} /with/ {}".format(cur_pp.as_str(), pp.as_str()))
                self.free_space[pp.page_number() * 4096] = pp
            elif cur_pp.journal() == pp.journal():
                logging.warning("Duplicate journal number found:\n   {} (in table)\n   {} (incoming)".format(cur_pp.as_str(), pp.as_str()))

    # this is the "AAD" used to encrypt the FastSpace
    def aad(version=VERSION, dna=DNA):
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
def decode_fscb(img, keys, FSCB_LEN_PAGES=2, dna=DNA):
    global SYSTEM_BASIS
    fscb = None
    spaceupdate_count = 0
    if SYSTEM_BASIS in keys:
        key = keys[SYSTEM_BASIS]
        pages = [img[i:i+4096] for i in range(0, len(img), 4096)]
        space_update = []
        fscb_start = None
        pg = 0
        for page in pages:
            logging.debug("FSCB page {}: {}".format(pg, page[:40].hex()))
            if bytearray(page[:32]) == bytearray([0xff] * 32):
                logging.debug("  ...page is blank")
                # page is blank
                pass
            elif bytearray(page[:16]) == bytearray([0xff] * 16):
                logging.debug("  ...page is spaceupdate")
                space_update.append(page[16:])
            else:
                if fscb_start == None:
                    logging.debug("  ...page is start of FSCB")
                    fscb_start = pg
                else:
                    logging.debug("  ...page is more FSCB data")
            pg += 1

        if fscb_start is not None:
            logging.debug("Found FSCB at {:x}".format(fscb_start))
            fscb_enc = img[fscb_start * 4096 : (fscb_start + FSCB_LEN_PAGES) * 4096]
            # print("data: {}".format(fscb_enc.hex()))
            try:
                nonce = fscb_enc[:12]
                logging.info("key: {}, nonce: {}".format(key[1].hex(), nonce.hex()))
                cipher = AES_GCM_SIV(key[1], nonce)
                logging.info("aad: {}".format(Fscb.aad(dna=dna).hex()))
                logging.info("mac: {}".format(fscb_enc[-16:].hex()))
                fscb_dec = cipher.decrypt(fscb_enc[12:], Fscb.aad(dna=dna))
                fscb = Fscb(fscb_dec, FASTSPACE_PAGES=FSCB_LEN_PAGES)
            except KeyError:
                logging.error("couldn't decrypt")
                fscb = None

        if fscb != None:
            logging.info("Total FreeSpace entries found: {}".format(fscb.len()))
            logging.debug("key: {}".format(key[1].hex()))
            cipher = AES.new(key[0], AES.MODE_ECB)
            for update in space_update:
                entries = [update[i:i+SpaceUpdate.SU_LEN] for i in range(0, len(update), SpaceUpdate.SU_LEN)]
                for entry in entries:
                    if entry == bytearray([0xff] * 16):
                        break
                    else:
                        fscb.try_replace(SpaceUpdate(cipher.decrypt(entry)))
                        spaceupdate_count += 1
    logging.info("Total SpaceUpdate entries found: {}".format(spaceupdate_count))
    return fscb


# # # code to extract system basis given a keybox
def get_key(index, keyrom, length):
    ret = []
    for offset in range(length // 4):
        word = int.from_bytes(keyrom[(index + offset) * 4: (index + offset) * 4 + 4], 'big')
        ret += list(word.to_bytes(4, 'little'))
    return ret

# Arguments:
#   - keyrom is the binary image of the key box, in plaintext
#   - pddb is the PDDB binary image, starting from the beginning of the PDDB itself (no backup headers, etc.)
#   - boot_pw is the boot pin
#   - basis_credentials is a list of the secret Bases
# Returns:
#   - A dictionary of keys, by Basis name
def extract_keys(keyrom, pddb, boot_pw, basis_credentials=[]):
    user_key_enc = get_key(40, keyrom, 32)
    pepper = get_key(248, keyrom, 16)
    pepper[0] = pepper[0] ^ 1 # encodes the "boot" password type into the pepper

    if boot_pw is not None:
        # acquire and massage the password so that we can decrypt the encrypted user key
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
    else:
        user_key = [0] * 32

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
    for [name, pw] in basis_credentials:
        logging.debug("extracting basis: {}:{}".format(name, pw))
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
        keys[name + ':' + pw] = [pt_key, data_key]

    return keys

def bip39_to_bits(phrase):
    BIP39_TABLE_EN = [
        "abandon", "ability", "able", "about", "above", "absent", "absorb",
        "abstract", "absurd", "abuse", "access", "accident", "account",
        "accuse", "achieve", "acid", "acoustic", "acquire", "across", "act",
        "action", "actor", "actress", "actual", "adapt", "add", "addict",
        "address", "adjust", "admit", "adult", "advance", "advice", "aerobic",
        "affair", "afford", "afraid", "again", "age", "agent", "agree",
        "ahead", "aim", "air", "airport", "aisle", "alarm", "album",
        "alcohol", "alert", "alien", "all", "alley", "allow", "almost",
        "alone", "alpha", "already", "also", "alter", "always", "amateur",
        "amazing", "among", "amount", "amused", "analyst", "anchor",
        "ancient", "anger", "angle", "angry", "animal", "ankle", "announce",
        "annual", "another", "answer", "antenna", "antique", "anxiety", "any",
        "apart", "apology", "appear", "apple", "approve", "april", "arch",
        "arctic", "area", "arena", "argue", "arm", "armed", "armor", "army",
        "around", "arrange", "arrest", "arrive", "arrow", "art", "artefact",
        "artist", "artwork", "ask", "aspect", "assault", "asset", "assist",
        "assume", "asthma", "athlete", "atom", "attack", "attend", "attitude",
        "attract", "auction", "audit", "august", "aunt", "author", "auto",
        "autumn", "average", "avocado", "avoid", "awake", "aware", "away",
        "awesome", "awful", "awkward", "axis", "baby", "bachelor", "bacon",
        "badge", "bag", "balance", "balcony", "ball", "bamboo", "banana",
        "banner", "bar", "barely", "bargain", "barrel", "base", "basic",
        "basket", "battle", "beach", "bean", "beauty", "because", "become",
        "beef", "before", "begin", "behave", "behind", "believe", "below",
        "belt", "bench", "benefit", "best", "betray", "better", "between",
        "beyond", "bicycle", "bid", "bike", "bind", "biology", "bird",
        "birth", "bitter", "black", "blade", "blame", "blanket", "blast",
        "bleak", "bless", "blind", "blood", "blossom", "blouse", "blue",
        "blur", "blush", "board", "boat", "body", "boil", "bomb", "bone",
        "bonus", "book", "boost", "border", "boring", "borrow", "boss",
        "bottom", "bounce", "box", "boy", "bracket", "brain", "brand",
        "brass", "brave", "bread", "breeze", "brick", "bridge", "brief",
        "bright", "bring", "brisk", "broccoli", "broken", "bronze", "broom",
        "brother", "brown", "brush", "bubble", "buddy", "budget", "buffalo",
        "build", "bulb", "bulk", "bullet", "bundle", "bunker", "burden",
        "burger", "burst", "bus", "business", "busy", "butter", "buyer",
        "buzz", "cabbage", "cabin", "cable", "cactus", "cage", "cake", "call",
        "calm", "camera", "camp", "can", "canal", "cancel", "candy", "cannon",
        "canoe", "canvas", "canyon", "capable", "capital", "captain", "car",
        "carbon", "card", "cargo", "carpet", "carry", "cart", "case", "cash",
        "casino", "castle", "casual", "cat", "catalog", "catch", "category",
        "cattle", "caught", "cause", "caution", "cave", "ceiling", "celery",
        "cement", "census", "century", "cereal", "certain", "chair", "chalk",
        "champion", "change", "chaos", "chapter", "charge", "chase", "chat",
        "cheap", "check", "cheese", "chef", "cherry", "chest", "chicken",
        "chief", "child", "chimney", "choice", "choose", "chronic", "chuckle",
        "chunk", "churn", "cigar", "cinnamon", "circle", "citizen", "city",
        "civil", "claim", "clap", "clarify", "claw", "clay", "clean", "clerk",
        "clever", "click", "client", "cliff", "climb", "clinic", "clip",
        "clock", "clog", "close", "cloth", "cloud", "clown", "club", "clump",
        "cluster", "clutch", "coach", "coast", "coconut", "code", "coffee",
        "coil", "coin", "collect", "color", "column", "combine", "come",
        "comfort", "comic", "common", "company", "concert", "conduct",
        "confirm", "congress", "connect", "consider", "control", "convince",
        "cook", "cool", "copper", "copy", "coral", "core", "corn", "correct",
        "cost", "cotton", "couch", "country", "couple", "course", "cousin",
        "cover", "coyote", "crack", "cradle", "craft", "cram", "crane",
        "crash", "crater", "crawl", "crazy", "cream", "credit", "creek",
        "crew", "cricket", "crime", "crisp", "critic", "crop", "cross",
        "crouch", "crowd", "crucial", "cruel", "cruise", "crumble", "crunch",
        "crush", "cry", "crystal", "cube", "culture", "cup", "cupboard",
        "curious", "current", "curtain", "curve", "cushion", "custom", "cute",
        "cycle", "dad", "damage", "damp", "dance", "danger", "daring", "dash",
        "daughter", "dawn", "day", "deal", "debate", "debris", "decade",
        "december", "decide", "decline", "decorate", "decrease", "deer",
        "defense", "define", "defy", "degree", "delay", "deliver", "demand",
        "demise", "denial", "dentist", "deny", "depart", "depend", "deposit",
        "depth", "deputy", "derive", "describe", "desert", "design", "desk",
        "despair", "destroy", "detail", "detect", "develop", "device",
        "devote", "diagram", "dial", "diamond", "diary", "dice", "diesel",
        "diet", "differ", "digital", "dignity", "dilemma", "dinner",
        "dinosaur", "direct", "dirt", "disagree", "discover", "disease",
        "dish", "dismiss", "disorder", "display", "distance", "divert",
        "divide", "divorce", "dizzy", "doctor", "document", "dog", "doll",
        "dolphin", "domain", "donate", "donkey", "donor", "door", "dose",
        "double", "dove", "draft", "dragon", "drama", "drastic", "draw",
        "dream", "dress", "drift", "drill", "drink", "drip", "drive", "drop",
        "drum", "dry", "duck", "dumb", "dune", "during", "dust", "dutch",
        "duty", "dwarf", "dynamic", "eager", "eagle", "early", "earn",
        "earth", "easily", "east", "easy", "echo", "ecology", "economy",
        "edge", "edit", "educate", "effort", "egg", "eight", "either",
        "elbow", "elder", "electric", "elegant", "element", "elephant",
        "elevator", "elite", "else", "embark", "embody", "embrace", "emerge",
        "emotion", "employ", "empower", "empty", "enable", "enact", "end",
        "endless", "endorse", "enemy", "energy", "enforce", "engage",
        "engine", "enhance", "enjoy", "enlist", "enough", "enrich", "enroll",
        "ensure", "enter", "entire", "entry", "envelope", "episode", "equal",
        "equip", "era", "erase", "erode", "erosion", "error", "erupt",
        "escape", "essay", "essence", "estate", "eternal", "ethics",
        "evidence", "evil", "evoke", "evolve", "exact", "example", "excess",
        "exchange", "excite", "exclude", "excuse", "execute", "exercise",
        "exhaust", "exhibit", "exile", "exist", "exit", "exotic", "expand",
        "expect", "expire", "explain", "expose", "express", "extend", "extra",
        "eye", "eyebrow", "fabric", "face", "faculty", "fade", "faint",
        "faith", "fall", "false", "fame", "family", "famous", "fan", "fancy",
        "fantasy", "farm", "fashion", "fat", "fatal", "father", "fatigue",
        "fault", "favorite", "feature", "february", "federal", "fee", "feed",
        "feel", "female", "fence", "festival", "fetch", "fever", "few",
        "fiber", "fiction", "field", "figure", "file", "film", "filter",
        "final", "find", "fine", "finger", "finish", "fire", "firm", "first",
        "fiscal", "fish", "fit", "fitness", "fix", "flag", "flame", "flash",
        "flat", "flavor", "flee", "flight", "flip", "float", "flock", "floor",
        "flower", "fluid", "flush", "fly", "foam", "focus", "fog", "foil",
        "fold", "follow", "food", "foot", "force", "forest", "forget", "fork",
        "fortune", "forum", "forward", "fossil", "foster", "found", "fox",
        "fragile", "frame", "frequent", "fresh", "friend", "fringe", "frog",
        "front", "frost", "frown", "frozen", "fruit", "fuel", "fun", "funny",
        "furnace", "fury", "future", "gadget", "gain", "galaxy", "gallery",
        "game", "gap", "garage", "garbage", "garden", "garlic", "garment",
        "gas", "gasp", "gate", "gather", "gauge", "gaze", "general", "genius",
        "genre", "gentle", "genuine", "gesture", "ghost", "giant", "gift",
        "giggle", "ginger", "giraffe", "girl", "give", "glad", "glance",
        "glare", "glass", "glide", "glimpse", "globe", "gloom", "glory",
        "glove", "glow", "glue", "goat", "goddess", "gold", "good", "goose",
        "gorilla", "gospel", "gossip", "govern", "gown", "grab", "grace",
        "grain", "grant", "grape", "grass", "gravity", "great", "green",
        "grid", "grief", "grit", "grocery", "group", "grow", "grunt", "guard",
        "guess", "guide", "guilt", "guitar", "gun", "gym", "habit", "hair",
        "half", "hammer", "hamster", "hand", "happy", "harbor", "hard",
        "harsh", "harvest", "hat", "have", "hawk", "hazard", "head", "health",
        "heart", "heavy", "hedgehog", "height", "hello", "helmet", "help",
        "hen", "hero", "hidden", "high", "hill", "hint", "hip", "hire",
        "history", "hobby", "hockey", "hold", "hole", "holiday", "hollow",
        "home", "honey", "hood", "hope", "horn", "horror", "horse",
        "hospital", "host", "hotel", "hour", "hover", "hub", "huge", "human",
        "humble", "humor", "hundred", "hungry", "hunt", "hurdle", "hurry",
        "hurt", "husband", "hybrid", "ice", "icon", "idea", "identify",
        "idle", "ignore", "ill", "illegal", "illness", "image", "imitate",
        "immense", "immune", "impact", "impose", "improve", "impulse", "inch",
        "include", "income", "increase", "index", "indicate", "indoor",
        "industry", "infant", "inflict", "inform", "inhale", "inherit",
        "initial", "inject", "injury", "inmate", "inner", "innocent", "input",
        "inquiry", "insane", "insect", "inside", "inspire", "install",
        "intact", "interest", "into", "invest", "invite", "involve", "iron",
        "island", "isolate", "issue", "item", "ivory", "jacket", "jaguar",
        "jar", "jazz", "jealous", "jeans", "jelly", "jewel", "job", "join",
        "joke", "journey", "joy", "judge", "juice", "jump", "jungle",
        "junior", "junk", "just", "kangaroo", "keen", "keep", "ketchup",
        "key", "kick", "kid", "kidney", "kind", "kingdom", "kiss", "kit",
        "kitchen", "kite", "kitten", "kiwi", "knee", "knife", "knock", "know",
        "lab", "label", "labor", "ladder", "lady", "lake", "lamp", "language",
        "laptop", "large", "later", "latin", "laugh", "laundry", "lava",
        "law", "lawn", "lawsuit", "layer", "lazy", "leader", "leaf", "learn",
        "leave", "lecture", "left", "leg", "legal", "legend", "leisure",
        "lemon", "lend", "length", "lens", "leopard", "lesson", "letter",
        "level", "liar", "liberty", "library", "license", "life", "lift",
        "light", "like", "limb", "limit", "link", "lion", "liquid", "list",
        "little", "live", "lizard", "load", "loan", "lobster", "local",
        "lock", "logic", "lonely", "long", "loop", "lottery", "loud",
        "lounge", "love", "loyal", "lucky", "luggage", "lumber", "lunar",
        "lunch", "luxury", "lyrics", "machine", "mad", "magic", "magnet",
        "maid", "mail", "main", "major", "make", "mammal", "man", "manage",
        "mandate", "mango", "mansion", "manual", "maple", "marble", "march",
        "margin", "marine", "market", "marriage", "mask", "mass", "master",
        "match", "material", "math", "matrix", "matter", "maximum", "maze",
        "meadow", "mean", "measure", "meat", "mechanic", "medal", "media",
        "melody", "melt", "member", "memory", "mention", "menu", "mercy",
        "merge", "merit", "merry", "mesh", "message", "metal", "method",
        "middle", "midnight", "milk", "million", "mimic", "mind", "minimum",
        "minor", "minute", "miracle", "mirror", "misery", "miss", "mistake",
        "mix", "mixed", "mixture", "mobile", "model", "modify", "mom",
        "moment", "monitor", "monkey", "monster", "month", "moon", "moral",
        "more", "morning", "mosquito", "mother", "motion", "motor",
        "mountain", "mouse", "move", "movie", "much", "muffin", "mule",
        "multiply", "muscle", "museum", "mushroom", "music", "must", "mutual",
        "myself", "mystery", "myth", "naive", "name", "napkin", "narrow",
        "nasty", "nation", "nature", "near", "neck", "need", "negative",
        "neglect", "neither", "nephew", "nerve", "nest", "net", "network",
        "neutral", "never", "news", "next", "nice", "night", "noble", "noise",
        "nominee", "noodle", "normal", "north", "nose", "notable", "note",
        "nothing", "notice", "novel", "now", "nuclear", "number", "nurse",
        "nut", "oak", "obey", "object", "oblige", "obscure", "observe",
        "obtain", "obvious", "occur", "ocean", "october", "odor", "off",
        "offer", "office", "often", "oil", "okay", "old", "olive", "olympic",
        "omit", "once", "one", "onion", "online", "only", "open", "opera",
        "opinion", "oppose", "option", "orange", "orbit", "orchard", "order",
        "ordinary", "organ", "orient", "original", "orphan", "ostrich",
        "other", "outdoor", "outer", "output", "outside", "oval", "oven",
        "over", "own", "owner", "oxygen", "oyster", "ozone", "pact", "paddle",
        "page", "pair", "palace", "palm", "panda", "panel", "panic",
        "panther", "paper", "parade", "parent", "park", "parrot", "party",
        "pass", "patch", "path", "patient", "patrol", "pattern", "pause",
        "pave", "payment", "peace", "peanut", "pear", "peasant", "pelican",
        "pen", "penalty", "pencil", "people", "pepper", "perfect", "permit",
        "person", "pet", "phone", "photo", "phrase", "physical", "piano",
        "picnic", "picture", "piece", "pig", "pigeon", "pill", "pilot",
        "pink", "pioneer", "pipe", "pistol", "pitch", "pizza", "place",
        "planet", "plastic", "plate", "play", "please", "pledge", "pluck",
        "plug", "plunge", "poem", "poet", "point", "polar", "pole", "police",
        "pond", "pony", "pool", "popular", "portion", "position", "possible",
        "post", "potato", "pottery", "poverty", "powder", "power", "practice",
        "praise", "predict", "prefer", "prepare", "present", "pretty",
        "prevent", "price", "pride", "primary", "print", "priority", "prison",
        "private", "prize", "problem", "process", "produce", "profit",
        "program", "project", "promote", "proof", "property", "prosper",
        "protect", "proud", "provide", "public", "pudding", "pull", "pulp",
        "pulse", "pumpkin", "punch", "pupil", "puppy", "purchase", "purity",
        "purpose", "purse", "push", "put", "puzzle", "pyramid", "quality",
        "quantum", "quarter", "question", "quick", "quit", "quiz", "quote",
        "rabbit", "raccoon", "race", "rack", "radar", "radio", "rail", "rain",
        "raise", "rally", "ramp", "ranch", "random", "range", "rapid", "rare",
        "rate", "rather", "raven", "raw", "razor", "ready", "real", "reason",
        "rebel", "rebuild", "recall", "receive", "recipe", "record",
        "recycle", "reduce", "reflect", "reform", "refuse", "region",
        "regret", "regular", "reject", "relax", "release", "relief", "rely",
        "remain", "remember", "remind", "remove", "render", "renew", "rent",
        "reopen", "repair", "repeat", "replace", "report", "require",
        "rescue", "resemble", "resist", "resource", "response", "result",
        "retire", "retreat", "return", "reunion", "reveal", "review",
        "reward", "rhythm", "rib", "ribbon", "rice", "rich", "ride", "ridge",
        "rifle", "right", "rigid", "ring", "riot", "ripple", "risk", "ritual",
        "rival", "river", "road", "roast", "robot", "robust", "rocket",
        "romance", "roof", "rookie", "room", "rose", "rotate", "rough",
        "round", "route", "royal", "rubber", "rude", "rug", "rule", "run",
        "runway", "rural", "sad", "saddle", "sadness", "safe", "sail",
        "salad", "salmon", "salon", "salt", "salute", "same", "sample",
        "sand", "satisfy", "satoshi", "sauce", "sausage", "save", "say",
        "scale", "scan", "scare", "scatter", "scene", "scheme", "school",
        "science", "scissors", "scorpion", "scout", "scrap", "screen",
        "script", "scrub", "sea", "search", "season", "seat", "second",
        "secret", "section", "security", "seed", "seek", "segment", "select",
        "sell", "seminar", "senior", "sense", "sentence", "series", "service",
        "session", "settle", "setup", "seven", "shadow", "shaft", "shallow",
        "share", "shed", "shell", "sheriff", "shield", "shift", "shine",
        "ship", "shiver", "shock", "shoe", "shoot", "shop", "short",
        "shoulder", "shove", "shrimp", "shrug", "shuffle", "shy", "sibling",
        "sick", "side", "siege", "sight", "sign", "silent", "silk", "silly",
        "silver", "similar", "simple", "since", "sing", "siren", "sister",
        "situate", "six", "size", "skate", "sketch", "ski", "skill", "skin",
        "skirt", "skull", "slab", "slam", "sleep", "slender", "slice",
        "slide", "slight", "slim", "slogan", "slot", "slow", "slush", "small",
        "smart", "smile", "smoke", "smooth", "snack", "snake", "snap",
        "sniff", "snow", "soap", "soccer", "social", "sock", "soda", "soft",
        "solar", "soldier", "solid", "solution", "solve", "someone", "song",
        "soon", "sorry", "sort", "soul", "sound", "soup", "source", "south",
        "space", "spare", "spatial", "spawn", "speak", "special", "speed",
        "spell", "spend", "sphere", "spice", "spider", "spike", "spin",
        "spirit", "split", "spoil", "sponsor", "spoon", "sport", "spot",
        "spray", "spread", "spring", "spy", "square", "squeeze", "squirrel",
        "stable", "stadium", "staff", "stage", "stairs", "stamp", "stand",
        "start", "state", "stay", "steak", "steel", "stem", "step", "stereo",
        "stick", "still", "sting", "stock", "stomach", "stone", "stool",
        "story", "stove", "strategy", "street", "strike", "strong",
        "struggle", "student", "stuff", "stumble", "style", "subject",
        "submit", "subway", "success", "such", "sudden", "suffer", "sugar",
        "suggest", "suit", "summer", "sun", "sunny", "sunset", "super",
        "supply", "supreme", "sure", "surface", "surge", "surprise",
        "surround", "survey", "suspect", "sustain", "swallow", "swamp",
        "swap", "swarm", "swear", "sweet", "swift", "swim", "swing", "switch",
        "sword", "symbol", "symptom", "syrup", "system", "table", "tackle",
        "tag", "tail", "talent", "talk", "tank", "tape", "target", "task",
        "taste", "tattoo", "taxi", "teach", "team", "tell", "ten", "tenant",
        "tennis", "tent", "term", "test", "text", "thank", "that", "theme",
        "then", "theory", "there", "they", "thing", "this", "thought",
        "three", "thrive", "throw", "thumb", "thunder", "ticket", "tide",
        "tiger", "tilt", "timber", "time", "tiny", "tip", "tired", "tissue",
        "title", "toast", "tobacco", "today", "toddler", "toe", "together",
        "toilet", "token", "tomato", "tomorrow", "tone", "tongue", "tonight",
        "tool", "tooth", "top", "topic", "topple", "torch", "tornado",
        "tortoise", "toss", "total", "tourist", "toward", "tower", "town",
        "toy", "track", "trade", "traffic", "tragic", "train", "transfer",
        "trap", "trash", "travel", "tray", "treat", "tree", "trend", "trial",
        "tribe", "trick", "trigger", "trim", "trip", "trophy", "trouble",
        "truck", "true", "truly", "trumpet", "trust", "truth", "try", "tube",
        "tuition", "tumble", "tuna", "tunnel", "turkey", "turn", "turtle",
        "twelve", "twenty", "twice", "twin", "twist", "two", "type",
        "typical", "ugly", "umbrella", "unable", "unaware", "uncle",
        "uncover", "under", "undo", "unfair", "unfold", "unhappy", "uniform",
        "unique", "unit", "universe", "unknown", "unlock", "until", "unusual",
        "unveil", "update", "upgrade", "uphold", "upon", "upper", "upset",
        "urban", "urge", "usage", "use", "used", "useful", "useless", "usual",
        "utility", "vacant", "vacuum", "vague", "valid", "valley", "valve",
        "van", "vanish", "vapor", "various", "vast", "vault", "vehicle",
        "velvet", "vendor", "venture", "venue", "verb", "verify", "version",
        "very", "vessel", "veteran", "viable", "vibrant", "vicious",
        "victory", "video", "view", "village", "vintage", "violin", "virtual",
        "virus", "visa", "visit", "visual", "vital", "vivid", "vocal",
        "voice", "void", "volcano", "volume", "vote", "voyage", "wage",
        "wagon", "wait", "walk", "wall", "walnut", "want", "warfare", "warm",
        "warrior", "wash", "wasp", "waste", "water", "wave", "way", "wealth",
        "weapon", "wear", "weasel", "weather", "web", "wedding", "weekend",
        "weird", "welcome", "west", "wet", "whale", "what", "wheat", "wheel",
        "when", "where", "whip", "whisper", "wide", "width", "wife", "wild",
        "will", "win", "window", "wine", "wing", "wink", "winner", "winter",
        "wire", "wisdom", "wise", "wish", "witness", "wolf", "woman",
        "wonder", "wood", "wool", "word", "work", "world", "worry", "worth",
        "wrap", "wreck", "wrestle", "wrist", "write", "wrong", "yard", "year",
        "yellow", "you", "young", "youth", "zebra", "zero", "zone", "zoo",
    ]
    wordlist = phrase.rstrip().split()
    if len(wordlist) not in [12, 15, 18, 21, 24]:
        raise ValueError("BIP-39 phrase has incorrect length")
    indices = []
    for word in wordlist:
        try:
            index = BIP39_TABLE_EN.index(word)
        except ValueError:
            raise ValueError("{} is not a BIP-39 word".format(word))
        indices.append(index)

    data = bytearray()
    bucket = 0
    bits_in_bucket = 0
    for index in indices:
        bucket = (bucket << 11) | index
        bits_in_bucket += 11
        while bits_in_bucket >= 8:
            data.append((bucket >> (bits_in_bucket - 8)) & 0xFF)
            mask = 0xFFFF_FFFF ^ (0xFF << (bits_in_bucket - 8))
            bucket &= mask
            bits_in_bucket -= 8

    if bits_in_bucket == 0:
        entered_checksum = data[-1]
        data = data[:-1]
    else:
        entered_checksum = bucket

    hasher = SHA256.new()
    hasher.update(data)
    digest = hasher.digest()
    checksum_bits = len(data) // 4
    checksum = digest[0] >> (8 - checksum_bits)

    if checksum == entered_checksum:
        return data
    else:
        raise ValueError("checksum did not match on BIP-39 phrase")
