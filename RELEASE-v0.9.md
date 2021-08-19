# Relase 0.9 notes

This release signals the stabilization of most Xous APIs; however, we reserve
the right to make breaking changes, but with some justification. You could call
this a "beta" release.

Notably, in 0.9, we have `libstd` integrated into Xous, and solidly intergated
into several servers; thus this release is not backward compatible with 0.8 (and
was never intended to be).

Aside from the introduction of `libstd`, few other breaking changes were implemented
in the APIs. Thus, we refer you to the [0.8 release notes](https://github.com/betrusted-io/xous-core/blob/main/RELEASE-v0.8.md#xous-08-messaging-api-in-practice)
for examples of idiomatic ways to write code for Xous.

## Major new features of 0.9
- Xous now targets `riscv32-imac-uknown-xous-elf` instead of `riscv32-imac-unknown-none-elf`.
  - `cargo xtask`, when run interactively, will offer to install the `xous` target.
  - Kernel debug primitives are accessible via the kernel console
  - GDB improvements
  - Improved `MemoryRange` API
  - Various stability improvements and major bug fixes (ELF loader bugs, stack pointer alignment, etc.)
  - Elimination of `heapless` crate
- All hardware drivers are available in some form:
  - AES (using VexRiscV CPU extensions)
  - Audio Codec (low-level interface only, 8kHz rec/playback)
  - Curve25519 accelerator
  - SHA512 (based on Google's OpenTitan core)
  - JTAG self-introspection drivers
  - Real time Clock
  - FLASH memory writing
  - Suspend/Resume
  - Clock throttling when idle
  - TRNG upgraded with health monitoring and CSPRNG whitener
  - Upgrades to previous drivers (keyboard, graphics, etc.)
  - Wifi enhancements including SSID scanning and AP join commands
- Secured boot flow:
  - Signed gateware, loader and kernel
  - Compatible with BBRAM and eFuse keyed gateware images
  - Semi-automated BBRAM key burning
  - Self-provisioning of root keys (update and boot unlock keys)
  - Local re-encryption of gateware updates to a secret key known only to your device
  - Re-signing of loader/OS updates to protect code-at-rest
  - Detection of image "downgrade" attacks that use developer keys
  - Indication of downgrade through hardware-enforced hash marks on the status bar
- Enhanced graphical user interface primitives:
  - Modal dialog boxes
  - Menus
  - Progress bars
  - Text and password entry forms with input validators
  - Radio buttons
  - Check boxes
  - Notifications
  - Defacement of less secure items: random hashes on background areas; complicates phishing attacks that present fake UI elements
- Enforcement of delimited attack surfaces
  - `xous-names` can optionally enforce a fixed number of connections to a server
    - Once all the connections are occupied after boot, new connections are no longer possible
    - Developers must increment the connection count, specified inside the target server itself, when adding new resources that require server access
  - `gam` enforces a registry of all UX elements
    - Rogue processes cannot create new trusted UX resources
    - Developers must register their UX tokens in the `gam` inside `tokens.rs`
- Extended simulation support
  - More peripherals working in Renode
  - Hosted mode performance improvements

## Roadmap to 1.0

The items that are still missing before we can hit a 1.0 release include:
- PDDB (plausibly deniable database): a key/value store that is the equivalent of a "filesystem" for Xous
- Networking capabilities: a simple (think UDP-only) network stack
- Password changes in the `rootkey` context
- Post-boot loadable applications (currently we have a kernel, there is no notion of a separate "application" file from the kernel code base; although this might be a 1.1-release feature as it depends on the PDDB, which is a major item in and of itself)
- Lots of testing and bug fixes
