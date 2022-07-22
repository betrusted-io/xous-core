#! /usr/bin/env python3

import argparse
import usb.core
import usb.util
from datetime import datetime
from progressbar.bar import ProgressBar
import requests

try:
    from .precursorusb import PrecursorUsb
except ImportError:
    from precursorusb import PrecursorUsb

QR_CODE = """

  ██████████████  ██  ████  ██████    ██      ██████████████
  ██          ██      ██████    ██  ████  ██  ██          ██
  ██  ██████  ██  ████████████      ██  ████  ██  ██████  ██
  ██  ██████  ██      ████      ██  ██  ██    ██  ██████  ██
  ██  ██████  ██      ████      ████  ██      ██  ██████  ██
  ██          ██  ██████    ████  ██████      ██          ██
  ██████████████  ██  ██  ██  ██  ██  ██  ██  ██████████████
                      ██    ██  ████  ██████
  ██  ██      ████  ██████████  ████      ██    ██    ██  ██
  ██████    ██  ██    ████      ██████  ██    ████      ████
    ██  ██    ██████    ██  ████████████████  ████  ████  ██
  ████    ██    ██  ██        ██      ██  ████      ██
  ████  ████  ██████    ██████████    ██████  ████        ██
    ██  ██        ██  ██  ████████████  ██████████      ████
    ████████  ██  ████  ████    ████    ██    ████        ██
  ██████  ████    ████  ██████  ██    ██  ██    ████
    ████████████████████  ████    ██    ████  ████        ██
    ██              ████        ████  ██████████      ██████
  ████    ██  ██  ██    ██  ████████  ██████████  ████    ██
        ██            ██  ██  ██████      ██  ██  ██
  ████  ████  ████████  ████████  ██    ██████████████  ██
                  ██        ██████      ████      ██████  ██
  ██████████████  ████████      ██████    ██  ██  ██      ██
  ██          ██    ██  ██  ██    ██  ██████      ██    ████
  ██  ██████  ██      ████  ██  ██      ██████████████  ████
  ██  ██████  ██                ████  ████████    ██████  ██
  ██  ██████  ██  ████████  ██████    ██████      ██    ████
  ██          ██        ████  ██  ██  ████████  ██████
  ██████████████  ████  ██  ████████      ██  ████████    ██

"""

def get_with_progress(url, name='Downloading'):
    r = requests.get(url, stream=True)
    total_length = int(r.headers.get('content-length'))
    ret = bytearray()
    progress = ProgressBar(min_value=0, max_value=total_length, prefix=name + ' ').start()
    for chunk in r.iter_content(chunk_size=65536):
        if chunk:
            ret += bytearray(chunk)
            progress.update(len(ret))
    progress.finish()
    return ret

def get_usb_interface(config=False, peek=None, override_csr=None, force=False):
    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found')

    dev.set_configuration()
    if config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

    if peek:
        pc_usb.peek(peek, display=True)
        exit(0)

    pc_usb.load_csrs(override_csr) # prime the CSR values
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
    elif force == True:
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

    return (locs, pc_usb)

def main():
    parser = argparse.ArgumentParser(description="Precursor USB Updater v2")
    parser.add_argument(
        "-b", "--bleeding-edge", required=False, help="Update to bleeding-edge CI build", action='store_true'
    )
    parser.add_argument(
        "-l", "--language", help="Select Xous language [en|ja|zh|en-tts]", required=False, type=str, default="en"
    )
    parser.add_argument(
        "--factory-reset", required=False, help="Delete passwords and do a factory reset", action='store_true'
    )
    parser.add_argument(
        "--paranoid", required=False, help="Do a full-wipe of the PDDB after the factory reset", action='store_true'
    )
    parser.add_argument(
        "--config", required=False, help="Print the Precursor USB descriptor", action='store_true'
    )
    parser.add_argument(
        "--override-csr", required=False, help="CSR file to use instead of CSR values stored with the image. Used to recover in case of a partially corrupted gateware", type=str,
    )
    parser.add_argument(
        "--peek", required=False, help="Inspect an address, then quit. Useful for sanity testing your config.", type=auto_int, metavar=('ADDR')
    )
    parser.add_argument(
        "--force", required=False, help="Ignore gitrev version on SoC and try to burn an image anyways", action="store_true"
    )
    parser.add_argument(
        "--dry-run", required=False, help="Don't actually burn anything, but print what would happen if we did", action="store_true"
    )

    args = parser.parse_args()

    VALID_LANGUAGES = {
        "en",
        "en-tts",
        "ja",
        "zh",
    }
    language = args.language.lower()
    if language not in VALID_LANGUAGES:
        print("Language selection '{}' is not valid. Please select one of {}".format(language, VALID_LANGUAGES))
        exit(1)

    # initial check to see if the Precursor device is there
    try:
        (locs, pc_usb) = get_usb_interface(args.config, args.peek, args.override_csr, args.force)
    except ValueError:
        print("Precursor device not found. Please check the USB cable and ensure that `usb debug` was run in Shellchat")
        exit(1)

    # now try to download all the artifacts and check their versions
    # this list should visit kernels in order from newest to oldest.
    URL_BLEEDING = 'https://ci.betrusted.io/releases/latest/'
    URL_STABLE = 'https://ci.betrusted.io/latest-ci/'
    print("Phase 1: Download the update")
    if args.bleeding_edge:
        print("Bleeding edge CI build selected")
        url = URL_BLEEDING
    else:
        print("Latest stable build selected")
        url = URL_STABLE

    try:
        while True:
            # first try the stable branch and see if it meets the version requirement
            kernel = get_with_progress(url + 'xous-' + language + '.img', 'Kernel')
            if int.from_bytes(kernel[:4], 'little') != 1:
                print("Downloaded kernel image has unexpected signature version. Aborting.")
                exit(1)
            kern_len = int.from_bytes(kernel[4:8], 'little') + 0x1000
            if len(kernel) != kern_len:
                print("Downloaded kernel has the wrong length. Aborting.")
                attempt += 1
                exit(1)
            curver_loc = kern_len - 4 - 4 - 16
            curver = SemVer(kernel[curver_loc:curver_loc + 16])

            loader = get_with_progress(url + 'loader.bin', 'Loader')
            soc_csr = get_with_progress(url + 'soc_csr.bin', 'Gateware')
            ec_fw = get_with_progress(url + 'ec_fw.bin', 'Embedded Controller')
            wf200 = get_with_progress(url + 'wf200_fw.bin', 'WF200')

            print("Downloaded Xous version {}".format(curver.as_str()))
            break
    except Exception as e:
        if False == single_yes_or_no_question("Error: '{}' encountered downloading the update. Retry? ".format(e)):
            print("Abort by user request.")
            exit(0)

    if args.factory_reset:
        if False == single_yes_or_no_question("This will permanently erase user data on the device. Proceed? "):
            print("Abort by user request.")
            exit(0)
        worklist = [
            ['erase', "Disabling boot by erasing loader...", locs['LOC_LOADER'][0], 1024 * 256],
            ['prog', "Uploading kernel", locs['LOC_KERNEL'][0], kernel],
            ['prog', "Uploading EC", locs['LOC_EC'][0], ec_fw],
            ['prog', "Uploading wf200", locs['LOC_WF200'][0], wf200],
            ['prog', "Overwriting gateware", locs['LOC_SOC'][0], soc_csr],
        ]
        if args.paranoid:
            # erase the entire area -- about 10-15 minutes
            worklist += [['erase', "Full erase of PDDB", locs['LOC_PDDB'][0], 0x620_0000]]
            worklist += [['prog', "Restoring loader", locs['LOC_LOADER'][0], loader]]
        else:
            # just deletes the page table -- about 10 seconds
            worklist += [['erase', "Shallow-delete of PDDB", locs['LOC_PDDB'][0], 1024 * 512]]
            worklist += [['prog', "Restoring loader", locs['LOC_LOADER'][0], loader]]
    else:
        worklist = [
            ['erase', "Disabling boot by erasing loader...", locs['LOC_LOADER'][0], 1024 * 256],
            ['prog', "Uploading kernel", locs['LOC_KERNEL'][0], kernel],
            ['prog', "Uploading EC", locs['LOC_EC'][0], ec_fw],
            ['prog', "Uploading wf200", locs['LOC_WF200'][0], wf200],
            ['prog', "Staging gateware", locs['LOC_STAGING'][0], soc_csr],
            ['prog', "Restoring loader", locs['LOC_LOADER'][0], loader],
        ]

    print("\nPhase 2: Apply the update")
    print("Halting CPU for update.")
    vexdbg_addr = int(pc_usb.regions['vexriscv_debug'][0], 0)
    pc_usb.ping_wdt()
    pc_usb.poke(vexdbg_addr, 0x00020000)
    for work in worklist:

        retry_usb = False
        while True:
            if retry_usb:
                print("Trying to re-aquire Precursor device...")
                try:
                    (locs, pc_usb) = get_usb_interface(args.config, args.peek, args.override_csr, args.force)
                except Exception as e:
                    if False == single_yes_or_no_question("Failed to find Precursor device. Try again? "):
                        print("Abort by user request!\n\nSystem may not be bootable, but you can retry an update as long as you do not power-off or hard-reset the device.")
                        exit(0)

                vexdbg_addr = int(pc_usb.regions['vexriscv_debug'][0], 0)
                pc_usb.ping_wdt()
                pc_usb.poke(vexdbg_addr, 0x00020000)
                retry_usb = False

            try:
                print(work[1])
                if work[0] == 'erase':
                    if args.dry_run:
                        print("DRYRUN: would erase at 0x{:x}, len 0x{:x}".format(work[2], work[3]))
                    else:
                        pc_usb.erase_region(work[2], work[3])
                    break
                else:
                    if args.dry_run:
                        print("DRYRUN: Would write at 0x{:x}".format(work[2]))
                    else:
                        pc_usb.flash_program(work[2], work[3], verify=False)
                    break
            except Exception as e:
                print("Error encountered while {}: '{}'".format(work[1], e))
                print("Try reseating the USB connection.")
                if False == single_yes_or_no_question("Try again? "):
                    print("Abort by user request!\n\nSystem may not be bootable, but you can retry an update as long as you do not power-off or hard-reset the device.")
                    exit(0)
                retry_usb = True

    print("Resuming CPU.")
    pc_usb.poke(vexdbg_addr, 0x02000000)

    print("Resetting SOC...")
    try:
        pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    except usb.core.USBError:
        pass # we expect an error because we reset the SOC and that includes the USB core


    print("\nUpdate finished!\n")
    print("{}\nVisit the QR code above to help locate the hole, or go to https://ci.betrusted.io/i/reset.jpg.".format(QR_CODE))
    print("Reboot by inserting a paperclip in the hole in the lower right hand side, then follow the on-device instructions.")


def auto_int(x):
    return int(x, 0)

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

if __name__ == "__main__":
    main()
    exit(0)
