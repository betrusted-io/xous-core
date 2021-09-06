# Wycheproof x25519 Test Vectors

This package contains code to make the Diffie-Hellman Key exchange test vectors for Curve25519
from [Project Wycheproof](https://github.com/google/wycheproof) usable for Xous.

Project Wycheproof is published under the [Apache-2.0 License](../LICENSES/Apache-2.0.txt). The
file [x25519_test.json](x25519_test.json) was imported from Project Wycheproof. See the newest corresponding commit
message to map the local file to the upstream file's version. A subset of the information therein is compiled to binary
using this package.

## Usage

The repository contains the precompiled test vectors. To run them, use the `engine wycheproof` command when running on
Renode or on real hardware.

### (Re-)compiling the test vectors

Run `cargo xtask wychproof-import` in order to compile the test cases in `wycheproof-import/x25519_test.json`
to `services/shellchat/src/cmds/x25519_test.bin` which will be included when compiling
the `services/shellchat/src/cmds/engine.rs`. The `xtask` command runs the local binary crate with the proper arguments.
