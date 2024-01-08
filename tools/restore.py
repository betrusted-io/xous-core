#! /usr/bin/env python3

import argparse

import usb.core
import usb.util
import array
import sys
import hashlib
import csv
import urllib.request
from datetime import datetime
import requests
from Crypto.Hash import SHA512

from progressbar.bar import ProgressBar

class PrecursorUsb:
    def __init__(self, dev):
        self.dev = dev
        self.RDSR = 0x05
        self.RDSCUR = 0x2B
        self.RDID = 0x9F
        self.WREN = 0x06
        self.WRDI = 0x04
        self.SE4B = 0x21
        self.BE4B = 0xDC
        self.PP4B = 0x12
        self.registers = {}
        self.regions = {}
        self.gitrev = ''

    def register(self, name):
        return int(self.registers[name], 0)

    def halt(self):
        if 'vexriscv_debug' in self.regions:
            self.poke(int(self.regions['vexriscv_debug'][0], 0), 0x00020000)
        elif 'reboot_cpu_hold_reset' in self.registers:
            self.poke(self.register('reboot_cpu_hold_reset'), 1)
        else:
            print("Can't find reset CSR. Try updating to the latest version of this program")

    def unhalt(self):
        if 'vexriscv_debug' in self.regions:
            self.poke(int(self.regions['vexriscv_debug'][0], 0), 0x02000000)
        elif 'reboot_cpu_hold_reset' in self.registers:
            self.poke(self.register('reboot_cpu_hold_reset'), 0)
        else:
            print("Can't find reset CSR. Try updating to the latest version of this program")

    def peek(self, addr, display=False):
        _dummy_s = '\x00'.encode('utf-8')
        data = array.array('B', _dummy_s * 4)

        numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
        wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
        data_or_wLength=data, timeout=500)

        read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
        if display == True:
            print("0x{:08x}".format(read_data))
        return read_data

    def poke(self, addr, wdata, check=False, display=False):
        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            print("before poke: 0x{:08x}".format(read_data))

        data = array.array('B', wdata.to_bytes(4, 'little'))
        numwritten = self.dev.ctrl_transfer(bmRequestType=(0x00 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            print("after poke: 0x{:08x}".format(read_data))
        if display == True:
            print("wrote 0x{:08x} to 0x{:08x}".format(wdata, addr))

    def burst_read(self, addr, len):
        _dummy_s = '\x00'.encode('utf-8')
        maxlen = 4096

        ret = bytearray()
        packet_count = len // maxlen
        if (len % maxlen) != 0:
            packet_count += 1

        for pkt_num in range(packet_count):
            cur_addr = addr + pkt_num * maxlen
            if pkt_num == packet_count - 1:
                if len % maxlen != 0:
                    bufsize = len % maxlen
                else:
                    bufsize = maxlen
            else:
                bufsize = maxlen

            data = array.array('B', _dummy_s * bufsize)
            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
                wValue=(cur_addr & 0xffff), wIndex=((cur_addr >> 16) & 0xffff),
                data_or_wLength=data, timeout=500)

            if numread != bufsize:
                print("Burst read error: {} bytes requested, {} bytes read at 0x{:08x}".format(bufsize, numread, cur_addr))
                exit(1)

            ret = ret + data

        return ret

    def burst_write(self, addr, data):
        if len(data) == 0:
            return

        # the actual "addr" doesn't matter for a burst_write, because it's specified
        # as an argument to the flash_pp4b command. We lock out access to the base of
        # SPINOR because it's part of the gateware, so, we pick a "safe" address to
        # write to instead. The page write responder will aggregate any write data
        # to anywhere in the SPINOR address range.
        writebuf_addr = 0x2098_0000 # the current start address of the kernel, for example

        maxlen = 4096
        packet_count = len(data) // maxlen
        if (len(data) % maxlen) != 0:
            packet_count += 1

        for pkt_num in range(packet_count):
            cur_addr = addr + pkt_num * maxlen
            if pkt_num == packet_count - 1:
                if len(data) % maxlen != 0:
                    bufsize = len(data) % maxlen
                else:
                    bufsize = maxlen
            else:
                bufsize = maxlen

            wdata = array.array('B', data[(pkt_num * maxlen):(pkt_num * maxlen) + bufsize])
            numwritten = self.dev.ctrl_transfer(bmRequestType=(0x00 | 0x43), bRequest=0,
                # note use of writebuf_addr instead of cur_addr -> see comment above about the quirk of write addressing
                wValue=(writebuf_addr & 0xffff), wIndex=((writebuf_addr >> 16) & 0xffff),
                data_or_wLength=wdata, timeout=500)

            if numwritten != bufsize:
                print("Burst write error: {} bytes requested, {} bytes written at 0x{:08x}".format(bufsize, numwritten, cur_addr))
                exit(1)

    def ping_wdt(self):
        self.poke(self.register('wdt_watchdog'), 1, display=False)
        self.poke(self.register('wdt_watchdog'), 1, display=False)

    def spinor_command_value(self, exec=0, lock_reads=0, cmd_code=0, dummy_cycles=0, data_words=0, has_arg=0):
        return ((exec & 1) << 1 |
                (lock_reads & 1) << 24 |
                (cmd_code & 0xff) << 2 |
                (dummy_cycles & 0x1f) << 11 |
                (data_words & 0xff) << 16 |
                (has_arg & 1) << 10
               )

    def flash_rdsr(self, lock_reads):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=lock_reads, cmd_code=self.RDSR, dummy_cycles=4, data_words=1, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_rdscur(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.RDSCUR, dummy_cycles=4, data_words=1, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_rdid(self, offset):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, cmd_code=self.RDID, dummy_cycles=4, data_words=offset, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_wren(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.WREN)
        )

    def flash_wrdi(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.WRDI)
        )

    def flash_se4b(self, sector_address):
        self.poke(self.register('spinor_cmd_arg'), sector_address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.SE4B, has_arg=1)
        )

    def flash_be4b(self, block_address):
        self.poke(self.register('spinor_cmd_arg'), block_address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.BE4B, has_arg=1)
        )

    def flash_pp4b(self, address, data_bytes):
        self.poke(self.register('spinor_cmd_arg'), address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.PP4B, has_arg=1, data_words=(data_bytes//2))
        )

    def load_csrs(self, fname=None):
        LOC_CSRCSV = 0x20277000 # this address shouldn't change because it's how we figure out our version number
        # CSR extraction:
        # dd if=soc_csr.bin of=csr_data_0.9.6.bin skip=2524 count=32 bs=1024
        if fname == None:
            csr_data = self.burst_read(LOC_CSRCSV, 0x8000)
        else:
            with open(fname, "rb") as f:
                csr_data = f.read(0x8000)

        hasher = hashlib.sha512()
        hasher.update(csr_data[:0x7FC0])
        digest = hasher.digest()
        if digest != csr_data[0x7fc0:]:
            print("Could not find a valid csr.csv descriptor on the device, aborting!")
            exit(1)

        csr_len = int.from_bytes(csr_data[:4], 'little')
        csr_extracted = csr_data[4:4+csr_len]
        decoded = csr_extracted.decode('utf-8')
        # strip comments
        stripped = []
        for line in decoded.split('\n'):
            if line.startswith('#') == False:
                stripped.append(line)
        # create database
        csr_db = csv.reader(stripped)
        for row in csr_db:
            if len(row) > 1:
                if 'csr_register' in row[0]:
                    self.registers[row[1]] = row[2]
                if 'memory_region' in row[0]:
                    self.regions[row[1]] = [row[2], row[3]]
                if 'git_rev' in row[0]:
                    self.gitrev = row[1]
        print("Using SoC {} registers".format(self.gitrev))

    def erase_region(self, addr, length):
        # ID code check
        code = self.flash_rdid(1)
        print("ID code bytes 1-2: 0x{:08x}".format(code))
        if code != 0x8080c2c2:
            print("ID code mismatch")
            exit(1)
        code = self.flash_rdid(2)
        print("ID code bytes 2-3: 0x{:08x}".format(code))
        if code != 0x3b3b8080:
            print("ID code mismatch")
            exit(1)

        # block erase
        progress = ProgressBar(min_value=0, max_value=length, prefix='Erasing ').start()
        erased = 0
        while erased < length:
            self.ping_wdt()
            if (length - erased >= 65536) and ((addr & 0xFFFF) == 0):
                blocksize = 65536
            else:
                blocksize = 4096

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            if blocksize == 4096:
                self.flash_se4b(addr + erased)
            else:
                self.flash_be4b(addr + erased)
            erased += blocksize

            while (self.flash_rdsr(1) & 0x01) != 0:
                pass

            result = self.flash_rdscur()
            if result & 0x60 != 0:
                print("E_FAIL/P_FAIL set on erase, programming may fail, but trying anyways...")

            if self.flash_rdsr(1) & 0x02 != 0:
                self.flash_wrdi()
                while (self.flash_rdsr(1) & 0x02) != 0:
                    pass
            if erased < length:
                progress.update(erased)
        progress.finish()
        print("Erase finished")

    # addr is relative to the base of FLASH (not absolute)
    def flash_program(self, addr, data, verify=True):
        flash_region = int(self.regions['spiflash'][0], 0)
        flash_len = int(self.regions['spiflash'][1], 0)

        if (addr + len(data) > flash_len):
            print("Write data out of bounds! Aborting.")
            exit(1)

        # ID code check
        code = self.flash_rdid(1)
        print("ID code bytes 1-2: 0x{:08x}".format(code))
        if code != 0x8080c2c2:
            print("ID code mismatch")
            exit(1)
        code = self.flash_rdid(2)
        print("ID code bytes 2-3: 0x{:08x}".format(code))
        if code != 0x3b3b8080:
            print("ID code mismatch")
            exit(1)

        # block erase
        progress = ProgressBar(min_value=0, max_value=len(data), prefix='Erasing ').start()
        erased = 0
        while erased < len(data):
            self.ping_wdt()
            if (len(data) - erased >= 65536) and ((addr & 0xFFFF) == 0):
                blocksize = 65536
            else:
                blocksize = 4096

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            if blocksize == 4096:
                self.flash_se4b(addr + erased)
            else:
                self.flash_be4b(addr + erased)
            erased += blocksize

            while (self.flash_rdsr(1) & 0x01) != 0:
                pass

            result = self.flash_rdscur()
            if result & 0x60 != 0:
                print("E_FAIL/P_FAIL set on erase, programming may fail, but trying anyways...")

            if self.flash_rdsr(1) & 0x02 != 0:
                self.flash_wrdi()
                while (self.flash_rdsr(1) & 0x02) != 0:
                    pass
            if erased < len(data):
                progress.update(erased)
        progress.finish()
        print("Erase finished")

        # program
        # pad out to the nearest word length
        if len(data) % 4 != 0:
            data += bytearray([0xff] * (4 - (len(data) % 4)))
        written = 0
        progress = ProgressBar(min_value=0, max_value=len(data), prefix='Writing ').start()
        while written < len(data):
            self.ping_wdt()
            if len(data) - written > 256:
                chunklen = 256
            else:
                chunklen = len(data) - written

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            self.burst_write(self.register('spinor_wdata'), data[written:(written+chunklen)])
            self.flash_pp4b(addr + written, chunklen)

            written += chunklen
            if written < len(data):
                progress.update(written)
        progress.finish()
        print("Write finished")

        if self.flash_rdsr(1) & 0x02 != 0:
            self.flash_wrdi()
            while (self.flash_rdsr(1) & 0x02) != 0:
                pass

        # dummy reads to clear the "read lock" bit
        self.flash_rdsr(0)

        # verify
        self.ping_wdt()
        if verify:
            print("Performing readback for verification...")
            self.ping_wdt()
            rbk_data = self.burst_read(addr + flash_region, len(data))
            if rbk_data != data:
                errs = 0
                err_thresh = 64
                for i in range(0, len(rbk_data)):
                    if rbk_data[i] != data[i]:
                        if errs < err_thresh:
                            print("Error at 0x{:x}: {:x}->{:x}".format(i, data[i], rbk_data[i]))
                        errs += 1
                    if errs == err_thresh:
                        print("Too many errors, stopping print...")
                print("Errors were found in verification, programming failed")
                print("Total byte errors: {}".format(errs))
                exit(1)
            else:
                print("Verification passed.")
        else:
            print("Skipped verification at user request")

        self.ping_wdt()

def auto_int(x):
    return int(x, 0)

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

class SemVer:
    def __init__(self, b):
        self.maj = int.from_bytes(b[0:2], 'little')
        self.min = int.from_bytes(b[2:4], 'little')
        self.rev = int.from_bytes(b[4:6], 'little')
        self.extra = int.from_bytes(b[6:8], 'little')
        self.has_commit = int.from_bytes(b[12:16], 'little')
        # note: very old kernel will return a version of 0.0.0
        if self.has_commit != 0:
            self.commit = int.from_bytes(b[8:12], 'little')

    def ord(self): # returns a number that you can use to compare if versions are bigger or smaller than each other
        return self.maj << 48 | self.min << 32 | self.rev << 16 | self.extra

    def as_str(self):
        if self.has_commit == 0:
            return "v{}.{}.{}-{}".format(self.maj, self.min, self.rev, self.extra)
        else:
            return "v{}.{}.{}-{}-g{:x}".format(self.maj, self.min, self.rev, self.extra, self.commit)

def single_yes_or_no_question(question, default_no=False):
    choices = ' [y/N]: ' if default_no else ' [Y/n]: '
    default_answer = 'n' if default_no else 'y'
    reply = str(input(question + choices)).lower().strip() or default_answer
    if reply[0] == 'y':
        return True
    if reply[0] == 'n':
        return False
    else:
        return False if default_no else True

def main():
    parser = argparse.ArgumentParser(description="Update/upload to a Precursor device running Xous 0.8/0.9")
    parser.add_argument(
        "--config", required=False, help="Print the descriptor", action='store_true'
    )
    parser.add_argument(
        "--override-csr", required=False, help="CSR file to use instead of CSR values stored with the image. Used to recover in case of partial update of soc_csr.bin", type=str,
    )
    parser.add_argument(
        "--peek", required=False, help="Inspect an address", type=auto_int, metavar=('ADDR')
    )
    parser.add_argument(
        "--file", required=False, help="File to restore from. Defaults to backup.pddb", default="backup.pddb", type=str
    )
    args = parser.parse_args()

    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found')

    dev.set_configuration()
    if args.config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

    if args.peek:
        pc_usb.peek(args.peek, display=True)
        # print(burst_read(dev, args.peek, 256).hex())
        exit(0)

    pc_usb.load_csrs(args.override_csr) # prime the CSR values
    if "v0.8" in pc_usb.gitrev:
        locs = {
           "LOC_SOC"    : [0x0000_0000, "soc_csr.bin"],
           "LOC_STAGING": [0x0028_0000, "pass"],
           "LOC_LOADER" : [0x0050_0000, "loader.bin"],
           "LOC_KERNEL" : [0x0098_0000, "xous.img"],
           "LOC_WF200"  : [0x07F8_0000, "pass"],
           "LOC_EC"     : [0x07FC_E000, "pass"],
           "LOC_AUDIO"  : [0x0634_0000, "short_8khz.wav"],
           "LEN_AUDIO"  : [0x01C4_0000, "pass"],
           "LOC_PDDB"   : [0x0100_0000, "pass"],
        }
    elif "v0.9" in pc_usb.gitrev:
        locs = {
            "LOC_SOC"    : [0x0000_0000, "soc_csr.bin"],
            "LOC_STAGING": [0x0028_0000, "pass"],
            "LOC_LOADER" : [0x0050_0000, "loader.bin"],
            "LOC_KERNEL" : [0x0098_0000, "xous.img"],
            "LOC_WF200"  : [0x07F8_0000, "pass"],
            "LOC_EC"     : [0x07FC_E000, "pass"],
            "LOC_AUDIO"  : [0x0634_0000, "short_8khz.wav"],
            "LEN_AUDIO"  : [0x01C4_0000, "pass"],
            "LOC_PDDB"   : [0x01D8_0000, "pass"],
        }
    elif args.force == True:
        # try the v0.9 offsets
        locs = {
           "LOC_SOC"    : [0x00000000, "soc_csr.bin"],
           "LOC_STAGING": [0x00280000, "pass"],
           "LOC_LOADER" : [0x00500000, "loader.bin"],
           "LOC_KERNEL" : [0x00980000, "xous.img"],
           "LOC_WF200"  : [0x07F80000, "pass"],
           "LOC_EC"     : [0x07FCE000, "pass"],
           "LOC_AUDIO"  : [0x06340000, "short_8khz.wav"],
           "LEN_AUDIO"  : [0x01C40000, "pass"],
           "LOC_PDDB"   : [0x01D80000, "pass"],
        }
    else:
        print("SoC is from an unknow rev '{}', use --force to continue anyways with v0.9 firmware offsets".format(pc_usb.load_csrs()))
        exit(1)

    LANGUAGE = {
        0: "en",
        1: "en-tts",
        2: "ja",
        3: "zh",
    }
    try:
        with open(args.file, "rb") as backup_file:
            backup = bytearray(backup_file.read())

            i = 0
            backup_version = int.from_bytes(backup[i:i+4], 'little')
            print("Backup protocol version: 0x{:08x}".format(backup_version))
            i += 4
            print("Xous version: {}".format(bytes_to_semverstr(backup[i:i+16])))
            backup_xous_ver = SemVer(backup[i:i+16])
            i += 16
            print("SOC version: {}".format(bytes_to_semverstr(backup[i:i+16])))
            i += 16
            print("EC version: {}".format(bytes_to_semverstr(backup[i:i+16])))
            i += 16
            print("WF200 version: {}".format(bytes_to_semverstr(backup[i:i+16])))
            i += 16
            i += 4 # padding because align=8
            ts = int.from_bytes(backup[i:i+8], 'little') / 1000
            print("Timestamp: {} / {}".format(ts, datetime.utcfromtimestamp(ts).strftime('%Y-%m-%d %H:%M:%S')))
            i += 8
            lcode = int.from_bytes(backup[i:i+4], 'little')
            print("Language code: {}".format(lcode))
            if lcode < len(LANGUAGE):
                language = LANGUAGE[lcode]
            else:
                language = "en"
            i += 4
            print("Keyboard layout code: {}".format(int.from_bytes(backup[i:i+4], 'little')))
            i += 4
            print("DNA: 0x{:x}".format(int.from_bytes(backup[i:i+8], 'little')))
            i += 8
            checksum_region_len = int.from_bytes(backup[i:i+4], 'little') * 4096
            print("Checksum region length: 0x{:x}".format(checksum_region_len))
            i += 4
            total_checksums = int.from_bytes(backup[i:i+4], 'little')
            print("Number of checksum blocks: {}".format(total_checksums)) # if this is 0, checksumming was skipped
            i += 4
            header_total_size = int.from_bytes(backup[i:i+4], 'little')
            print("Header total length in bytes: {}".format(header_total_size))
            i += 4
            i += 36 # reserved

            if backup_version == 0x10001:
                checksum_errors = False
                check_region = backup[:0x5d0]
                checksum = backup[0x5d0:0x5f0]
                print("Doing hash verification of pt+ct metadata...")
                hasher = SHA512.new(truncate="256")
                hasher.update(check_region)
                computed_checksum = hasher.digest()
                if computed_checksum != checksum:
                    print("Header failed hash integrity check!")
                    print("Calculated: {}".format(computed_checksum.hex()))
                    print("Expected:   {}".format(checksum.hex()))
                    exit(1)
                else:
                    print("Header passed integrity check.")

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
                            print("Bad checksum on block {} at offset 0x{:x}".format(check_block_num, check_block_num * checksum_region_len))
                            print("  Calculated: {}".format(sum[:16].hex()))
                            print("  Expected:   {}".format(checksums[check_block_num].hex()))
                            checksum_errors = True
                        check_block_num += 1

                    if checksum_errors:
                        print("Media errors were detected! Backup may be unusable, aborting restore.")
                        exit(1)
                    else:
                        print("No media errors detected, {} blocks passed checksum tests".format(total_checksums))

            if i != 0x90:
                # this is a sanity check for myself, and python doesn't like it when i make this an assert
                # because it does the math and realizes it's always true but the point is I don't want to do the math.
                print("Plaintext operand offset calculated incorrectly! Check data structure sizes.")
                exit(1)
            backup[i:i+4] = (2).to_bytes(4, 'little')
            op = int.from_bytes(backup[i:i+4], 'little')
            print("Opcode (should be 2, to trigger the next phase of restore): {}".format(op))
            assert(op==2)

            # now try to download all the artifacts and check their versions
            # this list should visit kernels in order from newest to oldest.
            URL_LIST = [
                'https://ci.betrusted.io/releases/v0.9.15/',
                'https://ci.betrusted.io/releases/v0.9.14/',
                'https://ci.betrusted.io/releases/v0.9.13/',
                'https://ci.betrusted.io/releases/v0.9.12/',
                'https://ci.betrusted.io/releases/v0.9.11/',
                'https://ci.betrusted.io/releases/v0.9.10/',
                'https://ci.betrusted.io/releases/v0.9.9/',
                'https://ci.betrusted.io/releases/v0.9.8/',
                'https://ci.betrusted.io/releases/v0.9.7/'
            ]
            if False: # insert bleeding-edge build for pre-release testing
                URL_LIST.insert(0, 'https://ci.betrusted.io/latest-ci/')

            attempt = 0
            while attempt < len(URL_LIST):
                # first try the stable branch and see if it meets the version requirement
                print("Downloading candidate restore kernel...")
                candidate_kernel = requests.get(URL_LIST[attempt] + 'xous-' + language + '.img').content
                if int.from_bytes(candidate_kernel[:4], 'little') != 1 and int.from_bytes(candidate_kernel[:4], 'little') != 2:
                    print("Downloaded kernel image has unexpected signature version. Trying the next image.")
                    attempt += 1
                    continue
                kern_len = int.from_bytes(candidate_kernel[4:8], 'little') + 0x1000
                if len(candidate_kernel) != kern_len:
                    print("Downloaded kernel has the wrong length. Trying the next image.")
                    attempt += 1
                    continue
                minver_loc = kern_len - 4 - 4 - 16 - 16 # length, sigver, current semver, then minver
                curver_loc = kern_len - 4 - 4 - 16
                min_compat_kernel = SemVer(candidate_kernel[minver_loc:minver_loc + 16])
                if min_compat_kernel.ord() > backup_xous_ver.ord():
                    print("Downloaded kernel is too new for the backup, trying an older version...")
                    attempt += 1
                    continue
                # alright, we should now be at a base URL where everything is a match. We skip checking
                # the rest of the versions because it "should" be an ensemble.
                curver = SemVer(candidate_kernel[curver_loc:curver_loc + 16])
                # print("Min ver: {}".format(min_compat_kernel.as_str()))
                # print("Cur ver: {}".format(curver.as_str()))
                print("Found viable kernel!")
                kernel = candidate_kernel
                print("Downloading loader...")
                loader = requests.get(URL_LIST[attempt] + 'loader.bin').content
                print("Downloading gateware...")
                soc_csr = requests.get(URL_LIST[attempt] + 'soc_csr.bin').content
                print("Downloading embedded controller...")
                ec_fw = requests.get(URL_LIST[attempt] + 'ec_fw.bin').content
                print("Downloading wf200...")
                wf200 = requests.get(URL_LIST[attempt] + 'wf200_fw.bin').content
                break
            print("The restore process takes about 30 minutes on a computer with an optimal USB stack (most Intel PC & Rpi), and about 2.5 hours on one with a buggy USB stack (most Macs & AMD PC).")
            if False == single_yes_or_no_question("Proceed with copying restore data from Xous release {}? ".format(curver.as_str())):
                print("Abort by user request.")
                exit(0)

            pc_usb.ping_wdt()
            print("Halting CPU.")
            pc_usb.halt()

            worklist = [
                ['erase', "Disabling boot by erasing loader...", locs['LOC_LOADER'][0], 1024 * 256],
                ['prog', "Uploading kernel", locs['LOC_KERNEL'][0], kernel],
                ['prog', "Uploading EC", locs['LOC_EC'][0], ec_fw],
                ['prog', "Uploading wf200", locs['LOC_WF200'][0], wf200],
                ['prog', "Uploading gateware", locs['LOC_STAGING'][0], soc_csr],
                ['prog', "Uploading PDDB", locs['LOC_PDDB'][0] - 0x1000, backup], # the PDDB file has a 4096-byte prefix
                ['prog', "Restoring loader", locs['LOC_LOADER'][0], loader],
            ]
            for work in worklist:
                success = False
                while success == False:
                    try:
                        print(work[1])
                        if work[0] == 'erase':
                            #print("pretend erase {}".format(work[2]))
                            pc_usb.erase_region(work[2], work[3])
                            success = True
                        else:
                            #print("prentend upload: {}".format(work[2]))
                            pc_usb.flash_program(work[2], work[3], verify=False)
                            success = True
                    except:
                        print("Error encountered while {}".format(work[1]))
                        print("Try reseating the USB connection.")
                        if False == single_yes_or_no_question("Try again? "):
                            print("Abort by user request. System may not be bootable, but you can try again later.")
                            exit(0)

            print("Restore finished copying objects.\nYou will need to reboot by inserting a paperclip in the hole in the lower right hand side,\nand follow on-screen instructions")

            print("Resuming CPU.")
            pc_usb.unhalt()

            print("Resetting SOC...")
            try:
                pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
            except usb.core.USBError:
                pass # we expect an error because we reset the SOC and that includes the USB core
    except:
        print("`backup.pddb` could not be opened. Aborting.")
        exit(1)


if __name__ == "__main__":
    main()
    exit(0)
