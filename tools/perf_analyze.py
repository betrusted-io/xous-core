#! /usr/bin/env python3
import argparse
import numpy as np
import os

# manually extracted from source files for now. A dictionary of lists
# whose entry indices correspond to the path to the source file.
FILEMAP = {
    'pddb' : [
        "services/pddb/src/main.rs",
        "services/pddb/src/dictionary.rs",
    ],
    'shellchat' : [
        "reserved",
        "reserved",
        "services/shellchat/src/cmds/pddb_cmd.rs",
    ],
    'vault' : [
        "reserved",
        "app/vault/src/actions.rs",
    ]
}
# manually extracted from build log message
PID_DICT = {
        1:  'kernel',
        2:  'xous-ticktimer',
        3:  'xous-log',
        4:  'xous-names',
        5:  'xous-susres',
        6:  'graphics-server',
        7:  'keyboard',
        8:  'spinor',
        9:  'llio',
        10: 'com',
        11: 'net',
        12: 'dns',
        13: 'gam',
        14: 'ime-frontend',
        15: 'ime-plugin-shell',
        16: 'codec',
        17: 'modals',
        18: 'root-keys',
        19: 'trng',
        20: 'sha2',
        21: 'engine-25519',
        22: 'jtag',
        23: 'status',
        24: 'shellchat',
        25: 'pddb',
        26: 'usb-device-xous',
        27: 'vault',
}

# display time in milliseconds
TIMEBASE = 1000.0 * (10e-9)

def main():
    parser = argparse.ArgumentParser(description="Analyze performance logs")
    parser.add_argument(
        "--file", help="file to analyze", type=str
    )
    parser.add_argument(
        "--average", help="average over a series. `file` is a root name; argument is number of data series, 1-offset indexed", type=int
    )
    args = parser.parse_args()

    if args.file is None:
        print("Must specify a file to analyze with --file")
        exit(0)

    if args.average:
        flist = []
        for i in range(1,args.average + 1):
            flist += [args.file + str(i) + ".bin"]
    else:
        flist = [args.file]

    for fname in flist:
        with open(fname, "rb") as f:
            data = f.read()
            entries = [data[i:i+8] for i in range(0, len(data), 8)]

            timers = []
            last_start = 0

            for entry in entries:
                code = int.from_bytes(entry[:4], 'little')
                timestamp = int.from_bytes(entry[4:], 'little')

                if code == 0 and timestamp == 0:
                    continue

                file_id = (code >> 29) & 0x7
                process_id = (code >> 24) & 0x1F
                meta = (code >> 21) & 0x7
                index = (code >> 13) & 0xFF
                line = code & 0x1FFF

                if process_id in PID_DICT:
                    pname = PID_DICT[process_id]
                    if pname in FILEMAP:
                        map = FILEMAP[pname]
                        if file_id < len(map):
                            fname = map[file_id]
                        else:
                            fname = 'fid{}'.format(file_id)
                    else:
                        fname = 'fid{}'.format(file_id)
                else:
                    pname = 'pid{}'.format(process_id)
                    fname = 'fid{}'.format(file_id)

                stampname = pname + fname + str(index)

                if meta == 0:
                    mname = ''
                elif meta == 1:
                    mname = 'START'
                    last_start = timestamp
                    timers += [[stampname, timestamp]]
                elif meta == 2:
                    mname = 'STOP'
                else:
                    mname = 'INVALID'

                stampstr = ''
                for t in timers:
                    stampstr += ' {:7.3f}'.format( (timestamp - t[1]) * TIMEBASE )

                print('{:8.3f}{}: {}[{}] {} {}:{}'.format(
                    timestamp * TIMEBASE,
                    stampstr,
                    mname,
                    index,
                    pname,
                    fname,
                    line,
                ))

                if mname == 'STOP':
                    remove = None
                    for idx, t in enumerate(timers):
                        if stampname in t:
                            remove = idx
                    if remove is not None:
                        timers.pop(remove)

if __name__ == "__main__":
    main()
    exit(0)
