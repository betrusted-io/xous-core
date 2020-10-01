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

if [ ! -f ../../betrusted-soc/build/software/soc.svd ]
then
  echo "Rebuilding UTRA crate..."
  cd utra && ./svd2utra.py -f ../../betrusted-soc/build/software/soc.svd
else
  echo "WARNING: soc.svd not found, using stock example file (should only happen for certain CI tests)"
  cd utra && ./svd2utra.py -f example/soc.svd
fi
cd ..

echo "Compiling top..."
cargo build --release --target riscv32imac-unknown-none-elf
echo "Compiling kernel..."
cd kernel && cargo build -p kernel --release --target riscv32imac-unknown-none-elf
cd ..
echo "Compiling loader..."
cd loader && cargo build -p loader --release --target riscv32imac-unknown-none-elf
cd ..

echo "Creating image..."
cargo run --release -p tools --bin create-image -- \
      --csv emulation/csr.csv \
      --kernel kernel/target/riscv32imac-unknown-none-elf/release/kernel \
      --init target/riscv32imac-unknown-none-elf/release/shell \
              target/args.bin

echo "Building binary..."
riscv64-unknown-elf-objcopy loader/target/riscv32imac-unknown-none-elf/release/loader -O binary loader.bin
dd if=/dev/null of=loader.bin bs=1 count=1 seek=65536
cat loader.bin target/args.bin > xous.img

echo "Copying to target..."
if [ $# -gt 0 ]
then
    if [ -z "$2" ]
    then
	scp xous.img $1:code/betrusted-scripts/
	scp ../betrusted-soc/build/gateware/encrypted.bin $1:code/betrusted-scripts/
	scp ../emulation/csr.csv $1:code/betrusted-scripts/soc-csr.csv
    else
	scp -i $2 xous.img $1:code/betrusted-scripts/
	scp -i $2 ../betrusted-soc/build/gateware/encrypted.bin $1:code/betrusted-scripts/
	scp -i $2 emulation/csr.csv $1:code/betrusted-scripts/soc-csr.csv
    fi
else
    echo "Copy to target with $0 <user@host> <ssh-id>"
fi
