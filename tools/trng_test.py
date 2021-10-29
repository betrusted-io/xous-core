#! /usr/bin/env python3

import argparse

import usb.core
import usb.util
import array
import sys
import hashlib
import csv
import time

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
        self.vexdbg_addr = None

    def register(self, name):
        return int(self.registers[name], 0)

    def peek(self, addr, display=False):
        _dummy_s = '\x00'.encode('utf-8')
        data = array.array('B', _dummy_s * 4)

        for attempt in range(10):
            try:
                numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
                wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
                data_or_wLength=data, timeout=500)
            except Exception as e:
                self.dev.reset()
                time.sleep(2)
            else:
                break

        read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
        if display == True:
            sys.stderr.write("0x{:08x}\n".format(read_data))
        return read_data

    def poke(self, addr, wdata, check=False, display=False):
        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
                wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
                data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            sys.stderr.write("before poke: 0x{:08x}\n".format(read_data))

        data = array.array('B', wdata.to_bytes(4, 'little'))
        for attempt in range(10):
            try:
                numwritten = self.dev.ctrl_transfer(bmRequestType=(0x00 | 0x43), bRequest=0,
                    wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
                    data_or_wLength=data, timeout=500)
            except Exception as e:
                sys.stderr.write("error; resetting device\n")
                self.dev.reset()
                time.sleep(2)
            else:
                break

        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            sys.stderr.write("after poke: 0x{:08x}\n".format(read_data))
        if display == True:
            sys.stderr.write("wrote 0x{:08x} to 0x{:08x}\n".format(wdata, addr))

    def burst_read(self, addr, len):
        _dummy_s = '\x00'.encode('utf-8')
        maxlen = 4096

        ret = bytearray()
        packet_count = len // maxlen
        if (len % maxlen) != 0:
            packet_count += 1

        time.sleep(0.2) # this improves system stability, somehow
        for pkt_num in range(packet_count):
            # sys.stderr.write('.', end='')
            cur_addr = addr + pkt_num * maxlen
            if pkt_num == packet_count - 1:
                if len % maxlen != 0:
                    bufsize = len % maxlen
                else:
                    bufsize = maxlen
            else:
                bufsize = maxlen

            data = array.array('B', _dummy_s * bufsize)
            for attempt in range(10):
                try:
                    if self.vexdbg_addr != None:
                        self.poke(self.vexdbg_addr, 0x00020000)
                    numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
                        wValue=(cur_addr & 0xffff), wIndex=((cur_addr >> 16) & 0xffff),
                        data_or_wLength=data, timeout=50000)
                    if self.vexdbg_addr != None:
                        self.poke(self.vexdbg_addr, 0x02000000)
                except Exception as e:
                    sys.stderr.write("error; resetting device\n")
                    self.dev.reset()
                    time.sleep(2)
                else:
                    break
            else:
                sys.stderr.write("Burst read failed\n")
                exit(1)

            if numread != bufsize:
                sys.stderr.write("Burst read error: {} bytes requested, {} bytes read at 0x{:08x}\n".format(bufsize, numread, cur_addr))
            else:
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
                sys.stderr.write("Burst write error: {} bytes requested, {} bytes written at 0x{:08x}".format(bufsize, numwritten, cur_addr))
                exit(1)

    def ping_wdt(self):
        self.poke(self.register('wdt_watchdog'), 1, display=False)
        self.poke(self.register('wdt_watchdog'), 1, display=False)

    def load_csrs(self):
        LOC_CSRCSV = 0x20277000 # this address shouldn't change because it's how we figure out our version number

        csr_data = self.burst_read(LOC_CSRCSV, 0x8000)
        hasher = hashlib.sha512()
        hasher.update(csr_data[:0x7FC0])
        digest = hasher.digest()
        if digest != csr_data[0x7fc0:]:
            sys.stderr.write("Could not find a valid csr.csv descriptor on the device, aborting!\n")
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
        sys.stderr.write("Using SoC {} registers\n".format(self.gitrev))

def auto_int(x):
    return int(x, 0)

def main():
    parser = argparse.ArgumentParser(description="Pipe TRNG data out of a Xous 0.8/0.9 Precusor that is configured to run the test server")
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
    args = parser.parse_args()

    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found')

    dev.set_configuration()
    if args.config:
        cfg = dev.get_active_configuration()
        sys.stderr.write(str(cfg))
        sys.stderr.write("\n")

    pc_usb = PrecursorUsb(dev)

    if args.peek:
        pc_usb.peek(args.peek, display=True)
        # sys.stderr.write(burst_read(dev, args.peek, 256).hex())
        exit(0)

    if args.poke:
        addr, data = args.poke
        pc_usb.poke(addr, data, check=args.check_poke, display=True)
        # import os
        # d = bytearray(os.urandom(8000))
        # burst_write(dev, addr, d)
        # r = burst_read(dev, addr, 8000)
        # sys.stderr.write(r.hex())
        # if d != r:
        #     sys.stderr.write("mismatch")
        # else:
        #     sys.stderr.write("match")
        exit(0)

    pc_usb.load_csrs() # prime the CSR values
    if "v0.8" in pc_usb.gitrev:
        LOC_SOC    = 0x00000000
        LOC_STAGING= 0x00280000
        LOC_LOADER = 0x00500000
        LOC_KERNEL = 0x00980000
        LOC_WF200  = 0x07F80000
        LOC_EC     = 0x07FCE000
        LOC_AUDIO  = 0x06340000
        LEN_AUDIO  = 0x01C40000
    elif "v0.9" in pc_usb.gitrev:
        LOC_SOC    = 0x00000000
        LOC_STAGING= 0x00280000
        LOC_LOADER = 0x00500000
        LOC_KERNEL = 0x00980000
        LOC_WF200  = 0x07F80000
        LOC_EC     = 0x07FCE000
        LOC_AUDIO  = 0x06340000
        LEN_AUDIO  = 0x01C40000
    elif args.force == True:
        # try the v0.8 offsets
        LOC_SOC    = 0x00000000
        LOC_STAGING= 0x00280000
        LOC_LOADER = 0x00500000
        LOC_KERNEL = 0x00980000
        LOC_WF200  = 0x07F80000
        LOC_EC     = 0x07FCE000
        LOC_AUDIO  = 0x06340000
        LEN_AUDIO  = 0x01C40000
    else:
        sys.stderr.write("SoC is from an unknow rev '{}', use --force to continue anyways with v0.8 firmware offsets".format(pc_usb.load_csrs()))
        exit(1)

    vexdbg_addr = int(pc_usb.regions['vexriscv_debug'][0], 0)
    pc_usb.vexdbg_addr = vexdbg_addr
    #pc_usb.ping_wdt()
    #sys.stderr.write("Halting CPU.")
    #pc_usb.poke(vexdbg_addr, 0x00020000)

    messible2_in = pc_usb.register('messible2_in')
    messible_out = pc_usb.register('messible_out')
    RAM_A = 0x40B0_0000
    RAM_B = 0x40C0_0000
    BURST_LEN = 512 * 1024
    TIMEOUT = 30.0

    phase = 0
    last_phase = 0
    blocks = 0
    while True:
        start_time = time.time()
        sys.stderr.write("at phase {}, waiting for next buffer\n".format(phase))
        while True:
            remote_phase = pc_usb.peek(messible_out)
            if remote_phase > phase:
                break
            time.sleep(0.5)
            if time.time() > (start_time + TIMEOUT):
                try:
                    pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
                except usb.core.USBError:
                    pass # we expect an error because we reset the SOC and that includes the USB core
                time.sleep(2.0)
                dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)
                dev.set_configuration()
                pc_usb = PrecursorUsb(dev)
                pc_usb.load_csrs() # prime the CSR values
                #pc_usb.poke(vexdbg_addr, 0x02000000) # maybe the CPU is still halted, try resuming it
                sys.stderr.write("timeout & reset\n")
                phase = 0
                last_phase = 0
                remote_phase = pc_usb.peek(messible_out)
                break

        phase = remote_phase
        pc_usb.poke(messible2_in, phase)

        if last_phase != phase:
            if (phase % 2) == 1:
                sys.stderr.write("phase {} fetching RAM_A\n".format(phase))
                page = pc_usb.burst_read(RAM_A, BURST_LEN)
                sys.stdout.buffer.write(page)
                sys.stderr.write("got page A {}\n".format(len(page)))
            else:
                sys.stderr.write("phase {} fetching RAM_B\n".format(phase))
                page = pc_usb.burst_read(RAM_B, BURST_LEN)
                sys.stdout.buffer.write(page)
                sys.stderr.write("got page B {}\n".format(len(page)))

            blocks += 1
        else:
            sys.stderr.write("phase didn't increment, not transferring identical block")

        sys.stderr.write("at block {}".format(blocks))

    #sys.stderr.write("Resuming CPU.")
    #pc_usb.poke(vexdbg_addr, 0x02000000)

    #sys.stderr.write("Resetting SOC...")
    #try:
    #    pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    #except usb.core.USBError:
    #    pass # we expect an error because we reset the SOC and that includes the USB core

    # sys.stderr.write("If you need to run more commands, please unplug and re-plug your device in, as the Precursor USB core was just reset")

if __name__ == "__main__":
    main()
    exit(0)
