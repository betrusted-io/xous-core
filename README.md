# Xous Core

Core files for the Xous microkernel operating system.

This repository contains everything necessary to build the Xous kernel
from source.  It consists of the following projects:

* **kernel**: core memory manager, irq manager, and syscallhandler
* **loader**: initial loader used to start the kernel
* **tools**: programs used to construct a final boot image
* **docs**: documentation on various aspects of Xous
* **emulation**: Renode scripts used to emulate Xous
* **xous-rs**: userspace library

## Generating an image

You can build all Xous packages by running:

```sh
$ rustup target add riscv32imac-unknown-none-elf
$ cargo build --target riscv32imac-unknown-none-elf
$ cd kernel
$ cargo build -p kernel --target riscv32imac-unknown-none-elf
$ cd ..
$ cd loader
$ cargo build -p loader --target riscv32imac-unknown-none-elf
$ cd ..
```

This will compile all runtime packages for `riscv32imac-unknown-none-elf`.
Note that the kernel and loader must currently be built separately, due
to the fact that they have custom linker scripts.

These need to be packaged into a loadable binary.  You can generate such
a binary by running the following:

```sh
$ cargo run -p tools --bin create-image -- \
        --csv emulation/csr.csv \
        --kernel kernel/target/riscv32imac-unknown-none-elf/debug/kernel \
        --init target/riscv32imac-unknown-none-elf/debug/shell \
        target/args.bin
```

## Targeting Real Hardware

The following files need manual adjustment:

* emulation/csr.csv should map to the csr.csv of the final SoC
* examples/graphics-server/src/backend/betrusted.rs needs a correct address for the "control" data structure (based on csr.csv)
* kernel/src/debug.rs:7 needs a correct UART base, based on csr.csv
* loader/src/debug.rs:31 needs a correct UART base, basedon csr.csv

Create the loader.bin from the .elf. `objcopy` adds several hundred megabytes of zero at the end of `loader.bin` so just
take the first 64kiB, which is the exact size allocated for it anyways in hardware. The final loadable image is created by 
`cat`ing the loader and the kernel together.

```
riscv64-unknown-elf-objcopy loader/target/riscv32imac-unknown-none-elf/release/loader -O binary loader_raw.bin
dd if=loader_raw.bin of=loader.bin bs=1024 count=64
cat loader.bin target/args.bin > xous.img
```

The image should be written to location 0x2050_0000 (SPI ROM offset 0x50_0000), using
the `provision-xous.sh` script inside [betrusted-scipts](https://github.com/betrusted-io/betrusted-scripts/blob/master/provision-xous.sh)
running on a Betrusted provisioning harness, that is a Raspberry Pi 4 with the appropriate debug hat attached, and the Precursor hardware plugged into the debug hat.

## Try It Out on a Desktop

Clone this repository, and run

`cargo xtask run`
