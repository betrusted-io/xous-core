#! /usr/bin/env python3

import argparse

import usb.core
import usb.util
import array
import sys
import hashlib
import csv

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
                wValue=(cur_addr & 0xffff), wIndex=((cur_addr >> 16) & 0xffff),
                data_or_wLength=wdata, timeout=500)

            if numwritten != bufsize:
                print("Burst write error: {} bytes requested, {} bytes written at 0x{:08x}".format(bufsize, numwritten, cur_addr))
                exit(1)

    def ping_wdt(self):
        self.poke(self.register('wdt_watchdog'), 0x600d, display=False)
        self.poke(self.register('wdt_watchdog'), 0xc0de, display=False)

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

    def load_csrs(self):
        LOC_CSRCSV = 0x20278000 # this address shouldn't change because it's how we figure out our version number

        csr_data = self.burst_read(LOC_CSRCSV, 0x8000)
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

            self.burst_write(flash_region, data[written:(written+chunklen)])
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
                print("Errors were found in verification, programming failed")
                exit(1)
            else:
                print("Verification passed.")
        else:
            print("Skipped verification at user request")

        self.ping_wdt()

def auto_int(x):
    return int(x, 0)

def main():
    parser = argparse.ArgumentParser(description="Update/upload to a Precursor device running Xous 0.8/0.9")
    parser.add_argument(
        "-s", "--soc", required=False, help="SoC gateware", type=str, nargs='?', metavar=('SoC gateware file'), const='soc_csr.bin'
    )
    parser.add_argument(
        "-l", "--loader", required=False, help="Loader", type=str, nargs='?', metavar=('loader file'), const='loader.bin'
    )
    parser.add_argument(
        "-k", "--kernel", required=False, help="Kernel", type=str, nargs='?', metavar=('kernel file'), const='xous.img'
    )
    parser.add_argument(
        "-e", "--ec", required=False, help="EC gateware", type=str, nargs='?', metavar=('EC gateware package'), const='ec_fw.bin'
    )
    parser.add_argument(
        "-w", "--wf200", required=False, help="WF200 firmware", type=str, nargs='?', metavar=('WF200 firmware package'), const='wf200_fw.bin'
    )
    parser.add_argument(
        "--peek", required=False, help="Inspect an address", type=auto_int, metavar=('ADDR')
    )
    parser.add_argument(
        "--poke", required=False, help="Write to an address", type=auto_int, nargs=2, metavar=('ADDR', 'DATA')
    )
    parser.add_argument(
        "--check-poke", required=False, action='store_true', help="Read data before and after the poke"
    )
    parser.add_argument(
        "--config", required=False, help="Print the descriptor", action='store_true'
    )
    parser.add_argument(
        "-i", "--image", required=False, help="Manually specify an image and address. Offset is relative to bottom of flash.", type=str, nargs=2, metavar=('IMAGEFILE', 'ADDR')
    )
    parser.add_argument(
        "-n", "--no-verify", help="Skip readback verification (may be necessary for large files to avoid WDT timeout). Only honored with -i.", action='store_true'
    )
    parser.add_argument(
        "-f", "--force", help="Ignore gitrev version on SoC and try to burn an image anyways", action="store_true"
    )
    args = parser.parse_args()

    if not len(sys.argv) > 1:
        print("No arguments specified, doing nothing. Use --help for more information.")
        exit(1)

    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found')

    dev.set_configuration()
    if args.config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

    if args.no_verify:
        verify = False
    else:
        verify = True

    if args.peek:
        pc_usb.peek(args.peek, display=True)
        # print(burst_read(dev, args.peek, 256).hex())
        exit(0)

    if args.poke:
        addr, data = args.poke
        pc_usb.poke(addr, data, check=args.check_poke, display=True)
        # import os
        # d = bytearray(os.urandom(8000))
        # burst_write(dev, addr, d)
        # r = burst_read(dev, addr, 8000)
        # print(r.hex())
        # if d != r:
        #     print("mismatch")
        # else:
        #     print("match")
        exit(0)

    pc_usb.load_csrs() # prime the CSR values
    if "v0.8" in pc_usb.gitrev:
        LOC_SOC    = 0x00000000
        LOC_LOADER = 0x00500000
        LOC_KERNEL = 0x00980000
        LOC_WF200  = 0x07F80000
        LOC_EC     = 0x07FCE000
    elif args.force == True:
        # try the v0.8 offsets
        LOC_SOC    = 0x00000000
        LOC_LOADER = 0x00500000
        LOC_KERNEL = 0x00980000
        LOC_WF200  = 0x07F80000
        LOC_EC     = 0x07FCE000
    else:
        print("SoC is from an unknow rev, use --force to continue anyways with v0.8 firmware offsets")
        exit(1)

    vexdbg_addr = int(pc_usb.regions['vexriscv_debug'][0], 0)
    pc_usb.ping_wdt()
    print("Halting CPU.")
    pc_usb.poke(vexdbg_addr, 0x00020000)

    if args.image:
        image_file, addr_str = args.image
        addr = int(addr_str, 0)
        print("Burning manually specified image '{}' to address 0x{:08x} relative to bottom of FLASH".format(image_file, addr))
        with open(image_file, "rb") as f:
            image_data = f.read()
            pc_usb.flash_program(addr, image_data, verify=verify)

    if args.ec != None:
        print("Staging EC firmware package '{}' in SOC memory space...".format(args.ec))
        with open(args.ec, "rb") as f:
            image = f.read()
            pc_usb.flash_program(LOC_EC, image)

    if args.wf200 != None:
        print("Staging WF200 firmware package '{}' in SOC memory space...".format(args.wf200))
        with open(args.wf200, "rb") as f:
            image = f.read()
            pc_usb.flash_program(LOC_WF200, image)

    if args.kernel != None:
        print("Programming kernel image {}".format(args.kernel))
        with open(args.kernel, "rb") as f:
            image = f.read()
            pc_usb.flash_program(LOC_KERNEL, image)

    if args.loader != None:
        print("Programming loader image {}".format(args.loader))
        with open(args.loader, "rb") as f:
            image = f.read()
            pc_usb.flash_program(LOC_LOADER, image)

    if args.soc != None:
        print("Programming SoC gateware {}".format(args.soc))
        with open(args.soc, "rb") as f:
            image = f.read()
            pc_usb.flash_program(LOC_SOC, image)

    print("Resuming CPU.")
    pc_usb.poke(vexdbg_addr, 0x02000000)

    print("Resetting SOC...")
    try:
        pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    except usb.core.USBError:
        pass # we expect an error because we reset the SOC and that includes the USB core

    # print("If you need to run more commands, please unplug and re-plug your device in, as the Precursor USB core was just reset")

if __name__ == "__main__":
    main()
    exit(0)
