#!/bin/bash
set -e

# populate with an invalid default so the equals compares work downstream
ARG1=${1:-bogus}

if [ $ARG1 == "-s" ]; then
    echo "Updating to latest stabilized release"
    REVISION="releases/latest"
elif [ $ARG1 == "-b" ]; then
    echo "Updating to bleeding edge release"
    REVISION="latest-ci"
else
    echo "Usage: ${0} [-s] [-b] [[-l LOCALE]], where LOCALE is one of en, ja, zh, or en-tts"
    echo "One of -s or -b must be specified for either stabilized or bleeding edge branches"
    echo " "
    echo "This script does a factory reset: all keys and data will be lost. No backsies."
    exit 1
fi

ARG2=${2:-bogus}
if [ $ARG2 == "-l" ]; then
    if [ -z "$3" ]; then
        echo "Missing locale specifier"
        exit 0
    fi
    LOCALE="-"$3
else
    LOCALE=""
fi

wget https://ci.betrusted.io/$REVISION/soc_csr.bin -O /tmp/soc_csr.bin
./usb_update.py --soc /tmp/soc_csr.bin --force
rm /tmp/soc_csr.bin

echo "waiting for device to reboot"
sleep 5

wget https://ci.betrusted.io/$REVISION/loader.bin -O /tmp/loader.bin
./usb_update.py -l /tmp/loader.bin
rm /tmp/loader.bin

echo "waiting for device to reboot"
sleep 4

wget https://ci.betrusted.io/$REVISION/xous$LOCALE.img -O /tmp/xous.img
./usb_update.py -k /tmp/xous.img
rm /tmp/xous.img

echo "waiting for device to reboot"
sleep 5

wget https://ci.betrusted.io/$REVISION/ec_fw.bin -O /tmp/ec_fw.bin
./usb_update.py -e /tmp/ec_fw.bin
rm /tmp/ec_fw.bin

sleep 5

wget https://ci.betrusted.io/$REVISION/wf200_fw.bin -O /tmp/wf200_fw.bin
./usb_update.py -w /tmp/wf200_fw.bin
rm /tmp/wf200_fw.bin

echo " "
echo "IMPORTANT: you must run 'ecup auto' to update the EC with the staged firmware objects."
