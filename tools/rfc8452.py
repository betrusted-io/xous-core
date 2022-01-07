# -*- coding: utf-8 -*-
# Copyright (c) Bjorn Edstrom <be@bjrn.se> 2019

# Reference implementation of AES-GCM-SIV based on the IRTF draft.
# Do not use.

# Vendored from https://github.com/bjornedstrom/aes-gcm-siv-py on Nov 14 2021.
# Commit bc9c8f484405437720f7372850e8e8cadad08190
# And slightly modified to be compatible with the Pycryptodome crate as used elsewhere by
# the project.
#
# As of Nov 2021 Pycryptodome doesn't have an RFC8452 (aes-gcm-siv) implementation (the
# thing called Galois mode (GCM) is a different RFC). This code is labelled "do not use"
# by the author, but we use it only to diagnose/test a production
# Rust crate, so, in the worst case, we just get some invalid diagnostic results.

import Crypto.Cipher.AES as AES
import pyaesni
import binascii
import six
import struct


class Field(object):
    # The field is defined by the irreducible polynomial
    # x^128 + x^127 + x^126 + x^121 + 1
    _MOD = sum((1 << a) for a in [0, 121, 126, 127, 128])

    # x^-128 is equal to x^127 + x^124 + x^121 + x^114 + 1
    _INV = sum((1 << a) for a in [0, 114, 121, 124, 127])

    @staticmethod
    def add(x, y):
        #assert x < (1 << 128) # in pursuit of great performance we take great risks :-P
        #assert y < (1 << 128)
        return x ^ y

    @staticmethod
    def mul(x, y):
        #assert x < (1 << 128), x
        #assert y < (1 << 128), y

        res = 0
        for bit in range(128):
            if (y >> bit) & 1:
                #res ^= (2 ** bit) * x
                res ^= (1 << bit) * x

        return Field.mod(res, Field._MOD)

    @staticmethod
    def dot(a, b):
        return Field.mul(Field.mul(a, b), Field._INV)

    @staticmethod
    def mod(a, m):
        m2 = m
        i = 0
        while m2 < a:
            m2 <<= 1
            i += 1
        while i >= 0:
            a2 = a ^ m2
            if a2 < a:
                a = a2
            m2 >>= 1
            i -= 1
        return a


def polyval(h, xs):
    """POLYVAL takes a field element, H, and a series of field elements X_1,
   ..., X_s.  Its result is S_s, where S is defined by the iteration S_0
   = 0; S_j = dot(S_{j-1} + X_j, H), for j = 1..s"""
    s = 0
    for x in xs:
        s = Field.dot(Field.add(s, x), h)
    return s


class PolyvalIUF(object):
    """Polyval implemented as an IUF construction, specifically in the context
    of AES-GCM_SIV."""

    def __init__(self, h, nonce):
        self._s = 0
        self._h = b2i(h)
        self._nonce = bytearray(nonce)

    # TODO: update() is a bit sensitive w.r.t zero-padding, make sure
    # it's called so there is no superfluous zero-padding added in the middle
    # of the input due to splitting etc.
    def update(self, inp):
        def update16(inp):
            assert len(inp) == 16
            self._s = Field.dot(Field.add(self._s, b2i(inp)), self._h)

        def split16(s):
            return [s[i:i+16] for i in range(0, len(s), 16)]

        def _right_pad_to_16(b):
            while len(b) % 16 != 0:
                b += b'\x00'
            return b

        for block in split16(inp):
            update16(_right_pad_to_16(block))

    def digest(self):
        S_s = bytearray(i2b(self._s))
        for i in range(12):
            S_s[i] ^= self._nonce[i]
        S_s[15] &= 0x7f
        return S_s


def b2i(s):
    res = 0
    for c in reversed(s):
        res <<= 8
        res |= (ord(c) if six.PY2 else c)
    return res


def i2b(i):
    return i.to_bytes(16, 'little')
    # original implementation below -- if the leading bytes are '00', it will truncate and return a 15-long array, instead of 16-long, which is incorrect.
    #if i == 0:
    #    return b'\x00'*16
    #s = b''
    #while i:
    #    s += chr(i & 0xff) if six.PY2 else bytes([i & 0xff])
    #    i >>= 8
    #return bytes(s)


def s2i(s):
    return b2i(binascii.unhexlify(s))


def i2s(i):
    return binascii.hexlify(i2b(i))


def le_uint32(i):
    return struct.pack(b'<L', i)


def read_le_uint32(b):
    return struct.unpack(b'<L', b[0:4])[0]


def le_uint64(i):
    return struct.pack(b'<Q', i)


class AES_GCM_SIV(object):
    def __init__(self, key_gen_key, nonce):
        #aes_obj = AES.new(key_gen_key, AES.MODE_ECB)
        #msg_auth_key = aes_obj.encrypt(le_uint32(0) + nonce)[0:8] + \
        #               aes_obj.encrypt(le_uint32(1) + nonce)[0:8]
        msg_auth_key = pyaesni.cbc256_encrypt(le_uint32(0) + nonce, key_gen_key, bytearray(16))[0:8] + \
                       pyaesni.cbc256_encrypt((le_uint32(1) + nonce), key_gen_key, bytearray(16))[0:8]
        #msg_enc_key = aes_obj.encrypt(le_uint32(2) + nonce)[0:8] + \
        #              aes_obj.encrypt(le_uint32(3) + nonce)[0:8]
        msg_enc_key = pyaesni.cbc256_encrypt(le_uint32(2) + nonce, key_gen_key, bytearray(16))[0:8] + \
                      pyaesni.cbc256_encrypt(le_uint32(3) + nonce, key_gen_key, bytearray(16))[0:8]
        if len(key_gen_key) == 32:
            #msg_enc_key += aes_obj.encrypt(le_uint32(4) + nonce)[0:8] + \
            #               aes_obj.encrypt(le_uint32(5) + nonce)[0:8]
            msg_enc_key += pyaesni.cbc256_encrypt(le_uint32(4) + nonce, key_gen_key, bytearray(16))[0:8] + \
                           pyaesni.cbc256_encrypt(le_uint32(5) + nonce, key_gen_key, bytearray(16))[0:8]
        self.msg_auth_key = msg_auth_key
        self.msg_enc_key = msg_enc_key
        self.nonce = nonce

    def _right_pad_to_16(self, inp):
        while len(inp) % 16 != 0:
            inp += b'\x00'
        return inp

    def _aes_ctr(self, key, initial_block, inp):
        block = initial_block
        output = b''
        while len(inp) > 0:
            #keystream_block = AES.new(key, AES.MODE_ECB).encrypt(block)
            keystream_block = pyaesni.cbc256_encrypt(block, key, bytearray(16))
            #print("aes")
            #print(binascii.hexlify(keystream_block))
            #print(binascii.hexlify(keystream_block_a))
            block = le_uint32((read_le_uint32(block[0:4]) + 1) & 0xffffffff) + block[4:]
            todo = min(len(inp), len(keystream_block))
            for j in range(todo):
                if six.PY2:
                    output += chr(ord(keystream_block[j]) ^ ord(inp[j]))
                else:
                    output += bytes([keystream_block[j] ^ inp[j]])
            inp = inp[todo:]
        return output

    def _polyval_calc(self, plaintext, additional_data):
        # Instead of calculating S_s inline using the RFC polyval() function,
        # we redesign polyval as an IUF "hash". The old/RFC way would be as below:
        #
        # """
        # padded_plaintext = self._right_pad_to_16(plaintext)
        # padded_ad = self._right_pad_to_16(additional_data)
        # S_s = bytearray(
        #     i2b(polyval(b2i(self.msg_auth_key),
        #                 map(b2i, split16(padded_ad) + split16(padded_plaintext) + [length_block]))))
        # nonce = bytearray(self.nonce)
        # for i in range(12):
        #     S_s[i] ^= nonce[i]
        # S_s[15] &= 0x7f
        # assert S_s == S_s_new
        # """

        pvh = PolyvalIUF(self.msg_auth_key, self.nonce)
        pvh.update(additional_data)
        pvh.update(plaintext)

        length_block = le_uint64(len(additional_data) * 8) + \
                       le_uint64(len(plaintext) * 8)
        pvh.update(length_block)

        return pvh.digest()

    def encrypt(self, plaintext, additional_data):
        """Encrypt"""

        if len(plaintext) > 2**36:
            raise ValueError('plaintext too large')

        if len(additional_data) > 2**36:
            raise ValueError('additional_data too large')

        # Polyval/tag calculation
        S_s = self._polyval_calc(plaintext, additional_data)
        #tag = AES.new(self.msg_enc_key).encrypt(bytes(S_s))
        tag = pyaesni.cbc256_encrypt(bytes(S_s), self.msg_enc_key, bytearray(16))

        # Encrypt
        counter_block = bytearray(tag)
        counter_block[15] |= 0x80
        return self._aes_ctr(self.msg_enc_key, bytes(counter_block), plaintext) + bytes(tag)

    def decrypt(self, ciphertext, additional_data):
        """Decrypt"""

        if len(ciphertext) < 16 or len(ciphertext) > 2**36 + 16:
            raise ValueError('ciphertext too small or too large')

        if len(additional_data) > 2**36:
            raise ValueError('additional_data too large')

        ciphertext, tag = ciphertext[0:-16], ciphertext[-16:]

        counter_block = bytearray(tag)
        counter_block[15] |= 0x80
        plaintext = self._aes_ctr(self.msg_enc_key, bytes(counter_block), ciphertext)

        # Polyval/tag calculation
        S_s = self._polyval_calc(plaintext, additional_data)
        expected_tag = bytearray(AES.new(self.msg_enc_key, AES.MODE_ECB).encrypt(bytes(S_s)))

        # Check tag
        actual_tag = bytearray(tag)

        xor_sum = 0
        for i in range(len(expected_tag)):
            xor_sum |= expected_tag[i] ^ actual_tag[i]

        if xor_sum != 0:
            raise ValueError('auth fail')

        return plaintext
