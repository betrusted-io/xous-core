#! /usr/bin/env python3
import argparse
import numpy as np
from PIL import Image as im
import random
import os

def randomize_palette(img):
    palette = img.getpalette()
    rand_palette = []
    for i in palette:
        rand_palette.append(random.randint(0, 255))
    img.putpalette(rand_palette)
    return img

def convert_to_image(times):
    thresh = times.mean() * 2
    times.clip(min=0, max=thresh)
    times = times * (255 / thresh)

    img = im.new("P", (256, 128))
    x = 0
    y = 0
    for k in times.astype(int):
        for d in k:
            img.putpixel((x, y), int(d))
            y += 1
            y %= 128
        x += 1
        x %= 256

    return img

def main():
    parser = argparse.ArgumentParser(description="Analyze performance logs")
    parser.add_argument(
        "--file", default=False, help="file to analyze", type=str
    )
    parser.add_argument(
        "--average", default=False, help="average over a series. `file` is a root name; data series is hard-coded for now", action="store_true"
    )
    args = parser.parse_args()

    if args.file is None:
        print("Must specify a file to analyze with --file")
        exit(0)

    rootname = os.path.splitext(args.file)[0]

    rekey_start = 0
    enc_start = 0
    enc_times = np.zeros((256, 128))
    rekey_times = np.zeros((256, 128))

    if args.average:
        flist = []
        for i in range(1,9):
            flist += [args.file + str(i) + ".bin"]
    else:
        flist = [args.file]

    for fname in flist:
        with open(fname, "rb") as f:
            data = f.read()
            entries = [data[i:i+8] for i in range(0, len(data), 8)]
            for entry in entries:
                code = int.from_bytes(entry[:4], 'little')
                timestamp = int.from_bytes(entry[4:], 'little')
                keybit = code & 0xFF
                databit = (code >> 8) & 0xFF
                start = (code & 0x100_0000) == 0
                if start:
                    rekey_times[keybit][databit] += timestamp - rekey_start
                    enc_start = timestamp
                else:
                    enc_times[keybit][databit] += timestamp - enc_start
                    rekey_start = timestamp

    rekey_times = rekey_times / len(flist)
    enc_times = enc_times / len(flist)

    print('enc time sample')
    for i in range(32):
        print('{}'.format(enc_times[i]))
    print('rekey time sample')
    print('{}'.format(rekey_times[0]))
    print("enc: mean {} / max {}".format(enc_times.mean(), enc_times.max()))
    hw_range = (3800,4400)
    ring_range = (12000,13000)
    hist, bins = enc_hist = np.histogram(enc_times, bins=64, range=hw_range)
    from matplotlib import pyplot as plt
    plt.hist(enc_times, bins=bins)
    plt.savefig('hist.png')
    print(enc_hist)
    print("enc typical cycles: {}".format(enc_hist[1][0]))
    print("rekey: mean {} / max {}".format(rekey_times.mean(), rekey_times.max()))
    rk_hist = np.histogram(rekey_times, bins=32)
    print(rk_hist)
    print("rekey typical cycles: {}".format(rk_hist[1][0]))

    enc = convert_to_image(enc_times)
    enc.convert("RGB").save(rootname + '_enc.png')
    enc_r = randomize_palette(enc)
    enc_r.convert("RGB").save(rootname + '_enc_r.png')

    rk = convert_to_image(rekey_times)
    rk.convert("RGB").save(rootname + '_rekey.png')
    rk_r = randomize_palette(rk)
    rk_r.convert("RGB").save(rootname + '_rekey_r.png')

if __name__ == "__main__":
    main()
    exit(0)
