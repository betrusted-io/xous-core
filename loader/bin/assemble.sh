#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
# SPDX-License-Identifier: Apache-2.0

set -euxo pipefail

rm -f arm*.a

crate=loader

arm-none-eabi-gcc -ggdb3 -mfpu=vfpv4-d16 -mfloat-abi=hard -c -march=armv7-a ../src/platform/atsama5d27/asm.S -o $crate.o
ar crs armv7a-unknown-xous-elf.a $crate.o

rm $crate.o