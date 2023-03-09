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

LANGUAGE = {
    0: "en",
    1: "en-tts",
    2: "ja",
    3: "zh",
}

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

def check_header(backup):
    checksums = []
    i = 0
    backup_version = int.from_bytes(backup[i:i+4], 'little')
    print("Backup protocol version: 0x{:08x}".format(backup_version))
    if backup_version != 0x00010000 and backup_version != 0x00010001:
        print("Backup protocol version is not correct.")
        return (False, checksums)
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
    if (datetime.utcfromtimestamp(ts) - datetime.now()).total_seconds() > 600:
        print("WARNING: Backup timestamp is in the future. Is the RTC and timezone set correctly on the device?")
        # return False # non-fatal, because we have hash checks now
    if datetime.utcfromtimestamp(ts).year < 2021:
        print("WARNING: Backup timestamp is from before 2021. Is the RTC and timezone set correctly on the device?")
        # return False
    i += 8
    lcode = int.from_bytes(backup[i:i+4], 'little')
    print("Language code: {}".format(lcode))
    if lcode < len(LANGUAGE):
        language = LANGUAGE[lcode]
    else:
        print("Language code is incorrect.")
        return (False, checksums)
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
        print("Checksums found in backup image, checking...")
        PT_HEADER_LEN = 4 + 16 * 4 + 4 + 8 + 64 + 4 + 4
        NONCE_LEN = 12
        CT_LEN = PT_HEADER_LEN + 1024 + 64
        TAG_LEN = 16
        COMMIT_NONCE_LEN = 32
        COMMIT_LEN = 32
        CHECKSUM_LEN = 32
        PADDING = 4
        checksum_loc = PT_HEADER_LEN + NONCE_LEN + CT_LEN + TAG_LEN + COMMIT_NONCE_LEN + COMMIT_LEN + PADDING
        check_region = backup[:checksum_loc]
        checksum = backup[checksum_loc:checksum_loc + CHECKSUM_LEN]
        hasher = SHA512.new(truncate="256")
        hasher.update(check_region)
        computed_checksum = hasher.digest()
        if computed_checksum != checksum:
            print("Header failed hash integrity check!")
            return (False, checksums)

        # Checksums are aligned to the end of the first 4096-byte block
        if header_total_size != len(backup):
            print("Header length does not match expected length")
            return (False, checksums)
        raw_checksums = backup[-(total_checksums * 16):]
        checksums = [raw_checksums[i:i+16] for i in range(0, len(raw_checksums), 16)]

    op = int.from_bytes(backup[i:i+4], 'little')
    print("Opcode (should be 1): {}".format(op))
    if op != 1:
        print("Opcode is incorrect.")
        return (False, checksums)

    print("Backup header passes sanity check!")
    return (True, checksums)

def auto_int(x):
    return int(x, 0)

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
        "--output", help="Output file name. Defaults to ./backup_<current_time>.pddb", type=str, default=None
    )
    args = parser.parse_args()

    if args.output is None:
        ofile = "backup_{}.pddb".format(datetime.now().strftime('%Y%m%d_%H%M%S'))
    else:
        ofile = args.output

    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found; be sure that the debug core is enabled')

    dev.set_configuration()
    if args.config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

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
        print("SoC is from an unknown rev '{}', use --force to continue anyways with v0.9 firmware offsets".format(pc_usb.load_csrs()))
        exit(1)

    pc_usb.ping_wdt()
    print("Halting CPU.")
    pc_usb.halt()

    header_checked = False
    checksum_error = False
    flash_region = int(pc_usb.regions['spiflash'][0], 0)
    if args.peek:
        pc_usb.peek(args.peek + flash_region, display=True)
    else:
        with open(ofile, "wb") as file:
            checksum = []
            start_addr = locs['LOC_PDDB'][0] - 0x1000
            total_length = locs['LOC_WF200'][0] - locs['LOC_PDDB'][0] + 0x1000
            progress = ProgressBar(min_value=0, max_value=total_length, prefix='Backing up ').start()
            block_size = 4096
            amount_read = 0
            check_block = bytearray(1024*1024) # the size of this is encoded in the header, but, it doesn't change so we hard code it here
            check_block_num = 0
            check_block_offset = 0
            while amount_read < total_length:
                if amount_read % (block_size * 16) == 0:
                    pc_usb.ping_wdt()
                backup = pc_usb.burst_read(start_addr + amount_read + flash_region, block_size)

                # block check logic
                if len(checksum) > 0: # this check inherently skips the header block, while also suppressing the check if no checksums are provided
                    check_block[check_block_offset:check_block_offset + block_size] = backup
                    check_block_offset += block_size
                    if check_block_offset >= len(check_block):
                        hasher = SHA512.new(truncate="256")
                        #print("hashing block of {} len".format(len(check_block)))
                        #print("start: {}".format(check_block[:64].hex()))
                        #print("end: {}".format(check_block[-64:].hex()))
                        hasher.update(check_block)
                        sum = hasher.digest()
                        if sum[:16] != checksum[check_block_num]:
                            print("Calculated: {}\nExpected: {}".format(sum[:16].hex(), bytes(checksum[check_block_num]).hex()))
                            print("Bad checksum on block {} at offset 0x{:x}. Re-run backup!".format(check_block_num, amount_read - block_size))
                            # TODO: automatic retry of the download. However, for now, this condition *should* be very rare
                            # (not even sure how to trigger it for testing) that we just print the error message for now.
                            checksum_error = True
                        else:
                            # print("Block {}/{} downloaded OK".format(check_block_num + 1, len(checksum)))
                            pass
                        check_block_offset = 0
                        check_block_num += 1

                # header check - putting it after the above skips incorporating the header data into check_block
                if header_checked is False:
                    (header_ok, checksum) = check_header(backup)
                    if header_ok:
                        header_checked = True
                    else:
                        break
                amount_read += block_size
                if amount_read < total_length:
                    progress.update(amount_read)

                file.write(backup)
            progress.finish()
            file.close()

        print("Resuming CPU.")
        pc_usb.unhalt()

    if checksum_error:
        print("Link errors detected while downloading the backup. Data is likely corrupted, please try again!")
        # Either that, or possibly the PDDB got into an inconsistent state after preparing the backup!

    print("Resetting SOC...")
    try:
        pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    except usb.core.USBError:
        pass # we expect an error because we reset the SOC and that includes the USB core

    if header_checked is False:
        print("--- BACKUP FAILED ---")
        print("Backup header did not pass basic integrity tests. Did you run 'Prepare Backups' on the device? Is the device time and timezone set correctly?")
    # print("If you need to run more commands, please unplug and re-plug your device in, as the Precursor USB core was just reset")

if __name__ == "__main__":
    main()
    exit(0)
