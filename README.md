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
```

This will compile all runtime packages for `riscv32imac-unknown-none-elf`.

These need to be packaged into a loadable binary.  You can generate such
a binary by running the following:

```sh
$ cargo run -p tools --bin create-image -- \
        --csv emulation/csr.csv
        --kernel target/riscv32imac-unknown-none-elf/debug/kernel \
        --init target/riscv32imac-unknown-none-elf/debug/shell \
        target/args.bin

$
```
