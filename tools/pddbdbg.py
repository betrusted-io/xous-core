#! /usr/bin/env python3
import argparse
import sys

from Crypto.Cipher import AES
from rfc8452 import AES_GCM_SIV

import binascii
import hashlib

SYSTEM_BASIS = '.System'

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


def main():
    parser = argparse.ArgumentParser(description="Debug PDDB Images")
    parser.add_argument(
        "--pddb-image", required=False, help="pddb disk image", type=str, nargs='?', metavar=('pddb image'), const='./pddb.bin'
    )
    parser.add_argument(
        "--pddb-keys", required=False, help="known pddb keys", type=str, nargs='?', metavar=('pddb keys'), const='./pddb.key'
    )
    args = parser.parse_args()

    if args.pddb_keys == None:
        keyfile = './tools/pddb.key'
    else:
        keyfile = args.pddb_keys

    if args.pddb_image == None:
        imagefile = './tools/pddb.bin'
    else:
        imagefile = args.pddb_image

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

    print("Found basis keys:")
    print(keys)

    MBBB_PAGES = 10
    FSCB_PAGES = 16
    KEY_PAGES = 1
    PAGE_SIZE = 4096

    with open(imagefile, 'rb') as img_f:
        raw_img = img_f.read()
        pddb_len = len(raw_img)
        pddb_size_pages = pddb_len // PAGE_SIZE

        img_index = 0
        tables = decode_pagetable(raw_img, pddb_size_pages, keys)
        img_index += pddb_size_pages * Pte.PTE_LEN
        rawkeys = raw_img[img_index : img_index + PAGE_SIZE * KEY_PAGES]
        img_index += PAGE_SIZE * KEY_PAGES
        mbbb = raw_img[img_index : img_index + PAGE_SIZE * MBBB_PAGES]
        img_index += PAGE_SIZE * MBBB_PAGES
        fscb = decode_fscb(raw_img[img_index: img_index + PAGE_SIZE * FSCB_PAGES], keys)
        img_index += PAGE_SIZE * FSCB_PAGES
        data = raw_img[img_index:]

        for name, key in keys.items():
            if name in tables:
                print("Basis {}".format(name))
                v2p_table = tables[name][0]
                p2v_table = tables[name][1]

                basis_data = bytearray()
                errors = 0
                for vp in sorted(v2p_table):
                    pp_start = v2p_table[vp]
                    # print("pp_start: {}, vp: {}".format(pp_start, vp))
                    pp_data = data[pp_start:pp_start + PAGE_SIZE]
                    try:
                        cipher = AES_GCM_SIV(key, pp_data[:12])
                        pt_data = cipher.decrypt(pp_data[12:], basis_aad(name))
                        basis_data.extend(bytearray(pt_data))
                    except KeyError:
                        errors += 1
                        print("couldn't decrypt vpage @ {} ppage @ {:x}".format(vp, v2p_table[vp]))

                if errors == 0:
                    print([hex(x) for x in basis_data[:256]])
                    basis = Basis(basis_data)
                    print(basis.as_str())

class Basis:
    MAX_NAME_LEN = 64
    def __init__(self, i_bytes):
        self.bytes = i_bytes
        i = 0
        self.journal = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.magic = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.version = int.from_bytes(i_bytes[i:i+2], 'little')
        i += 2
        self.name = i_bytes[i:i+Basis.MAX_NAME_LEN].rstrip(b'\x00').decode('utf8', errors='ignore')
        i += Basis.MAX_NAME_LEN
        print("alloc on disk: {}".format(i_bytes[i:i+8].hex()))
        self.prealloc_open_end = int.from_bytes(i_bytes[i:i+8], 'little')
        i += 8
        self.num_dicts = int.from_bytes(i_bytes[i:i+4], 'little')
        i += 4
        self.dict_ptr = int.from_bytes(i_bytes[i:i+8], 'little')
        i += 8

    def as_str(self):
        desc = ''
        desc += ' Journal rev: {}\n'.format(self.journal)
        desc += ' Magic: {:x}\n'.format(self.magic)
        desc += ' Version: {}\n'.format(self.version)
        desc += ' Name: {}\n'.format(self.name)
        desc += ' Alloc: {:x}\n'.format(self.prealloc_open_end)
        desc += ' NumDicts: {:x}\n'.format(self.num_dicts)
        desc += ' DictPtr: {:x}\n'.format(self.dict_ptr)
        return desc

def basis_aad(name, version=0, dna=0):
    name_bytes = bytearray(name, 'utf-8')
    name_bytes += bytearray(([0] * (Basis.MAX_NAME_LEN - len(name))))
    name_bytes += version.to_bytes(2, 'little')
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
        return int.from_bytes(self.bytes[:6], 'little')
    def flags(self):
        if self.bytes[6] == 1:
            return 'CLN'
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

def decode_pagetable(img, entries, keys):
    key_table = {}
    for name, key in keys.items():
        print("Pages for basis {}".format(name))
        print("key: {}".format(key.hex()))
        cipher = AES.new(key, AES.MODE_ECB)
        encrypted_ptes = [img[i:i+Pte.PTE_LEN] for i in range(0, entries * Pte.PTE_LEN, Pte.PTE_LEN)]
        page = 0
        v2p_table = {}
        p2v_table = {}
        for pte in encrypted_ptes:
            maybe_pte = Pte(cipher.decrypt(pte))
            if maybe_pte.is_valid():
                print(maybe_pte.as_str(page))
                p_addr = page * (4096 // Pte.PTE_LEN)
                v2p_table[maybe_pte.addr()] = p_addr
                p2v_table[p_addr] = maybe_pte.addr()

            page += Pte.PTE_LEN
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
        return 'a_{:05x} | c_{} | v_{} | ss_{} | j_{:02}'.format(self.page_number(), self.clean(), self.valid(), self.space_state(), self.journal())

class Fscb:
    def __init__(self, i_bytes):
        FASTSPACE_PAGES = 2
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
            print(pp.as_str())

    def try_replace(self, candidate):
        if candidate.is_valid():
            pp = candidate.get_pp()
            cur_pp = self.free_space[pp.page_number() * 4096]
            if pp.valid() and (cur_pp.journal() < pp.journal()):
                print("FSCB replace {} /with/ {}".format(cur_pp.as_str(), pp.as_str()))
                self.free_space[pp.page_number() * 4096] = pp

    # this is the "AAD" used to encrypt the FastSpace
    def aad(version=0, dna=0):
        return bytearray([46, 70, 97, 115, 116, 83, 112, 97, 99, 101]) + version.to_bytes(2, 'little') + dna.to_bytes(8, 'little')

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
            elif bytearray(page[:16] == bytearray([0xff] * 16)):
                space_update.append(page[16:])
            else:
                if fscb_start == None:
                    fscb_start = pg
            pg += 1

        if fscb_start:
            print("Found FSCB at {:x}".format(fscb_start))
            fscb_enc = img[fscb_start * 4096 : (fscb_start + FSCB_LEN_PAGES) * 4096]
            try:
                nonce = fscb_enc[:12]
                print("key: {}, nonce: {}".format(key.hex(), nonce.hex()))
                cipher = AES_GCM_SIV(key, nonce)
                #print("aad: {}".format(Fscb.aad().hex()))
                #print("mac: {}".format(fscb_enc[-16:].hex()))
                #print("data: {}".format(fscb_enc[12:-16].hex()))
                fscb_dec = cipher.decrypt(fscb_enc[12:], Fscb.aad())
                fscb = Fscb(fscb_dec)
            except KeyError:
                print("couldn't decrypt")
                fscb = None

        if fscb != None:
            print("key: {}".format(key.hex()))
            cipher = AES.new(key, AES.MODE_ECB)
            for update in space_update:
                entries = [update[i:i+SpaceUpdate.SU_LEN] for i in range(0, len(update), SpaceUpdate.SU_LEN)]
                for entry in entries:
                    if entry == bytearray([0xff] * 16):
                        break
                    else:
                        fscb.try_replace(SpaceUpdate(cipher.decrypt(entry)))

    return fscb

if __name__ == "__main__":
    main()
    exit(0)
