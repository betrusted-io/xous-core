#!/bin/sh

if [ -z "$1" ]
then
    echo "Usage: $0 target"
    echo "Missing target argument."
    exit 0
fi

riscv64-unknown-elf-gdb -ex 'set riscv use-compressed-breakpoints off' -ex 'file kernel/target/riscv32imac-unknown-none-elf/release/kernel' -ex "tar rem $1:3333"
