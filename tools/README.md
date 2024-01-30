# Xous Tools

The `updater` directory contains the `precursorupdater` script for device updates.
There are also various other scripts for backup, restore, and PDDB analysis located
here.

The `src` directory contains build tools for Xous, used to package up the
kernel and initial program images and create something that the runtime
can use.

It contains a number of programs:

* **copy-object**: A re-implementation of `objcopy`
* **create-image**: Tool used to create a boot args struct for Xous
* **make-tags**: Test program used to create raw boot arg tags
* **read-tags**: Test program to verify the tags were created

## Building

To build this repository, you will need Rust.

1. Build the tools: `cargo build --release`

## Using

The two most useful tools are `copy-object` and `create-image`.

To use `copy-object`, simply run `target/release/copy-object` and
specify the elf file you would like to copy.

To create a tags file with `create-image`, you will need to specify the
path to the kernel, as well as any initial programs you would like to
run.  You will also need to specify the memory range, or pass a
`csr.csv` file as an argument.

For example:

```sh
$ target/release/create-image \
      --kernel ../kernel/target/riscv32i-unknown-none-elf/debug/xous-kernel \
      --csv ../betrusted-soc/test/csr.csv \
      --init ../shell/target/riscv32i-unknown-none-elf/debug/xpr \
      args.bin
Arguments: Xous Arguments with 4 parameters
   Main RAM "SrEx" (78457253): 40000000 - 41000000
   Additional regions:
        Audi (69647541): e0000000 - e0001000
        CSRs (73525343): f0000000 - f000c000
        Disp (70736944): b0000000 - b0006000
        SpFl (6c467053): 20000000 - 28000000
        SrIn (6e497253): 10000000 - 10020000
        VexD (44786556): efff0000 - efff1000
   kernel: 75200 bytes long, loaded from 000000c4 to 00200000 with entrypoint @ 00200004, and 5624 bytes of data @ 00280000
   init: 31464 bytes long, loaded from 00012684 to 20000000 with entrypoint @ 20000004 and 0 bytes of data @ 10000000
   Bflg: -no_copy -absolute +DEBUG

Runtime will require 36916 bytes to track memory allocations
Image created in file ../tools/args.bin
$
```

You can then verify this file is correct by running `read-tags` on it:

```sh
$ cargo run --bin read-tags -- args.bin
    Finished dev [unoptimized + debuginfo] target(s) in 0.14s
     Running `target/debug/read-tags args.bin`
Found Xous Args Size at offset 8, setting total_words to 208
67724158 (XArg) (20 bytes, crc: 48b0): 00000034 00000001 40000000 01000000 78457253
7845524d (MREx) (96 bytes, crc: 671a): e0000000 00001000 69647541 00000000 f0000000 0000c000 73525343 00000000 b0000000 00006000 70736944 00000000 20000000 08000000 6c467053 00000000 10000000 00020000 6e497253 00000000 efff0000 00001000 44786556 00000000
6e724b58 (XKrn) (24 bytes, crc: 14e2): 000000c4 000125c0 00200000 00280000 000015f8 00200004
74696e49 (Init) (24 bytes, crc: 9da0): 00012684 00007ae8 20000000 10000000 00000000 20000004
676c6642 (Bflg) (4 bytes, crc: 8e32): 00000004
$
```

## Internationalization Helper

For more about `i18n_helper.py` please see the locales [README](../locales/README.md#internationalization-helper)

## Testing

_TBD_

## Contribution Guidelines

[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-v2.0%20adopted-ff69b4.svg)](../CODE_OF_CONDUCT.md)

Please see [CONTRIBUTING](../CONTRIBUTING.md) for details on
how to make a contribution.

Please note that this project is released with a
[Contributor Code of Conduct](../CODE_OF_CONDUCT.md).
By participating in this project you agree to abide its terms.

## License

Copyright © 2020

Licensed under the [Apache License 2.0](http://opensource.org/licenses/Apache-2.0) [LICENSE](LICENSE)
