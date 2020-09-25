#!/bin/sh

# argument 1 is the target for copy

if [ -z "$1" ]
then
    echo "Usage: $0 ssh-target [privatekey]"
    echo "Missing ssh-target argument."
    echo "Assumes betrusted-scripts repo is cloned on ssh-target at ~/code/betrused-scripts/"
    exit 0
fi

# notes:
# xous-stage1.bin written to 0x2050_0000 (64k erase block size)
# xous-kernel.bin written to 0x2051_0000 => passed as a0 arg to xous-stage1.bin
# This is handled in part by betrusted-scripts, with provision-xous.sh
# stage1 and kernel are merged into xous.img by this script.

cargo build --target riscv32imac-unknown-none-elf
cd kernel && cargo build -p kernel --target riscv32imac-unknown-none-elf
cd ..
cd loader && cargo build -p loader --target riscv32imac-unknown-none-elf
cd ..

cargo run -p tools --bin create-image -- \
      --csv emulation/csr.csv \
      --kernel kernel/target/riscv32imac-unknown-none-elf/debug/kernel \
      --init target/riscv32imac-unknown-none-elf/debug/shell \
              target/args.bin

riscv64-unknown-elf-objcopy loader/target/riscv32imac-unknown-none-elf/release/loader -O binary loader_raw.bin
dd if=loader_raw.bin of=loader.bin bs=1024 count=64
cat loader.bin target/args.bin > xous.img

if [ $# -gt 0 ]
then
    if [ -z "$2" ]
    then
	scp xous.img $1:code/betrusted-scripts/
	scp ../betrusted-soc/build/gateware/encrypted.bin $1:code/betrusted-scripts/
    else
	scp -i $2 xous.img $1:code/betrusted-scripts/
	scp -i $2 ../betrusted-soc/build/gateware/encrypted.bin $1:code/betrusted-scripts/
    fi
else
    echo "Copy to target with $0 <user@host> <ssh-id>"
fi
