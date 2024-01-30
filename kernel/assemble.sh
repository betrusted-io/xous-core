#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
# SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
# SPDX-License-Identifier: Apache-2.0

set -euxo pipefail

crate=xous-kernel

usage() {
    echo "Usage: $0 [-a <riscv|arm>]"
    exit 1
}

while getopts "a:" o; do
    case "${o}" in
        a)
            arch=$OPTARG
            ;;
        *)
            usage
            ;;
    esac
done

arch=${arch:-riscv}

mkdir -p bin

case $arch in
    riscv)
        # Remove existing blobs because otherwise this will append object
        # files to the old blobs
        rm -f bin/riscv*.a

        riscv-none-elf-gcc -ggdb3 -c -mabi=ilp32 -march=rv32imac_zicsr src/arch/riscv/asm.S -o bin/$crate.o
        ar crs bin/riscv32imac-unknown-none-elf.a bin/$crate.o
        ar crs bin/riscv32imac-unknown-xous-elf.a bin/$crate.o
        ar crs bin/riscv32imc-unknown-none-elf.a bin/$crate.o

        riscv-none-elf-gcc -ggdb3 -c -mabi=ilp32 -march=rv32i_zicsr src/arch/riscv/asm.S -DSKIP_MULTICORE -o bin/$crate.o
        ar crs bin/riscv32i-unknown-none-elf.a bin/$crate.o

        riscv-none-elf-gcc -ggdb3 -c -mabi=lp64 -march=rv64imac_zicsr src/arch/riscv/asm.S -o bin/$crate.o
        ar crs bin/riscv64imac-unknown-none-elf.a bin/$crate.o
        ar crs bin/riscv64gc-unknown-none-elf.a bin/$crate.o
        ;;

    arm)
        rm -f bin/arm*.a

        arm-none-eabi-gcc -ggdb3 -c -march=armv7-a src/arch/arm/asm.S -o bin/$crate.o
        ar crs bin/armv7a-unknown-xous-elf.a bin/$crate.o
        ;;

    default)
        usage
        ;;
esac

rm bin/$crate.o
