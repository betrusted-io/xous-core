#!/bin/sh

# argument 1 is the target for copy

if [ -z "$1" ]
then
    echo "Usage: $0 ssh-target [privatekey]"
    echo "Missing ssh-target argument."
    echo "Assumes betrusted-scripts repo is cloned on ssh-target at ~/code/betrused-scripts/"
    exit 0
fi

DESTDIR=code/precursors

# notes:
# xous-stage1.bin written to 0x2050_0000 (64k erase block size)
# xous-kernel.bin written to 0x2051_0000 => passed as a0 arg to xous-stage1.bin
# This is handled in part by betrusted-scripts, with provision-xous.sh
# stage1 and kernel are merged into xous.img by this script.

cargo xtask hw-image ../betrusted-soc/build/software/soc.svd

if [ $? -ne 0 ]
then
    echo "build failed, aborting!"
    exit 1
fi

echo "Copying to target..."
if [ $# -gt 0 ]
then
    if [ -z "$2" ]
    then
	scp target/riscv32imac-unknown-none-elf/release/xous.img $1:$DESTDIR/
	scp ../betrusted-soc/build/gateware/encrypted.bin $1:$DESTDIR/
	scp ../betrusted-soc/build/csr.csv $1:$DESTDIR/soc-csr.csv
    else
	scp -i $2 target/riscv32imac-unknown-none-elf/release/xous.img $1:$DESTDIR/
	scp -i $2 ../betrusted-soc/build/gateware/encrypted.bin $1:$DESTDIR/
	scp -i $2 ../betrusted-soc/build/csr.csv $1:$DESTDIR/soc-csr.csv
    fi
else
    echo "Copy to target with $0 <user@host> <ssh-id>"
fi
