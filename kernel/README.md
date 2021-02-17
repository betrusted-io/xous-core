<!--
SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
SPDX-License-Identifier: Apache-2.0
-->


# Xous Kernel

This contains the core kernel for Xous.  It requires a stage 1 loader in
order to start up, as it assumes the system is already running in
Supervisor mode.

## Building

To build the kernel, you will need a riscv32 target for Rust.  Possible
targets include `riscv32i-unknown-none-elf`, `riscv32imac-unknown-none-elf`,
or `riscv32imac-unknown-xous-elf`.

1. Decide what target you want.  For simple, embedded systems this could
   be `riscv32i-unknown-none-elf`, and for more complex systems with
   compressed instructions you could use `riscv32imac-unknown-none-elf`.
2. Get Rust.  Go to https://rustup.rs/ and follow its instructions.
3. Install the proper toolchain: `rustup target add ${target_arch}`
4. Build the kernel: `cargo build --release --target ${target_arch}`

## Using

To use the kernel, you must package it up into an arguments binary with
`xous-tools`.

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

This project is licensed under the [Apache License 2.0](http://opensource.org/licenses/Apache-2.0) [LICENSE](LICENSE). For accurate information, please check individual files.
