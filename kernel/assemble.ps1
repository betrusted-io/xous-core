# SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
# SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
# SPDX-License-Identifier: Apache-2.0

param ($Arch="riscv") 

$crate = "xous-kernel"

New-Item -Force -Path bin -Type Directory | Out-Null

Switch ($Arch) {
    "riscv" {
        # remove existing blobs because otherwise this will append object files to the old blobs
        Remove-Item -Force bin/riscv*.a

        riscv64-unknown-elf-gcc -ggdb3 -c -mabi=ilp32 -march=rv32imac src/arch/riscv/asm.S -o bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv32imac-unknown-none-elf.a bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv32imac-unknown-xous-elf.a bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv32imc-unknown-none-elf.a bin/$crate.o

        riscv64-unknown-elf-gcc -ggdb3 -c -mabi=ilp32 -march=rv32i src/arch/riscv/asm.S -DSKIP_MULTICORE -o bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv32i-unknown-none-elf.a bin/$crate.o

        riscv64-unknown-elf-gcc -ggdb3 -c -mabi=lp64 -march=rv64imac src/arch/riscv/asm.S -o bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv64imac-unknown-none-elf.a bin/$crate.o
        riscv64-unknown-elf-ar crs bin/riscv64gc-unknown-none-elf.a bin/$crate.o
    }
    "arm" {
        Remove-Item -Force bin/arm*.a

        arm-none-eabi-gcc -ggdb3 -c -march=armv7-a src/arch/arm/asm.S -o bin/$crate.o
        ar crs bin/armv7a-unknown-xous-elf.a bin/$crate.o
    }
}

Remove-Item bin/$crate.o
