# Getting Started with Baochip Targets

Baochip-based boards use the `bao1x` SoC. There are two boards supported out-of-the-box:

- `dabao` is a minimal SoM-style breakout board. Basically just the chip on a board.
- `baosec` is a hardware security token. It features a camera, display, external memory, buttons, and a supplemental hardware TRNG.

The build idiom is `cargo xtask <target>`, i.e. `cargo xtask dabao`. The build will generate a [`UF2`](https://makecode.com/blog/one-chip-to-flash-them-all) artifact, which can be found in `target/riscv32imac-unknown-[xous|none]-elf/release/`. You will need to copy all three artifacts generated (loader.uf2, xous.uf2, and apps.uf2) initially to ensure that the loader, kernel, and applications are at the same revision. After that point if the loader and kernel are not updated, one can just update apps.uf2.

Holding down the `PROG` button while plugging the device into USB will cause it to enter a bootloader that enumerates a mass storage device. The build artifacts can then be copied onto the device. Pressing `PROG` again will cause the device to run the program. Targets also enumerate a serial port over USB, which will activate a debug console.

## Bootloaders

Regardless of the board, the `bao1x` chip comes from the factory programmed with `boot0` and `boot1` stages. The code for these can be found in `bao1x-boot/`.

- `boot0` is a permanent root of trust burned onto the chip. Its sole purpose is to validate `boot1`. See [security model](./README-baochip.md#security-model) for details on the trust chain
- `boot1` contains a USB driver, serial-over-USB driver, and serial driver capable of accepting code updates in the form of .u2f files or serial commands. It also contains a small command terminal for managing configurations, keys, and device lifecycle state.

### Reserved pins (Board Designers Take Note!)

`boot1` assumes the following pins

- `PC13` and `PF5` are both briefly driven to 0 and then 1 before USB enumeration. A USB switch such as the EMS4000 will ensure "clean" enumeration as the USB PHY on Baochip has no way to definitively enter the SE0 state on its own. After exiting the bootloader, the pin not corresponding to the board type is set to an input (`PC13` is dabao, `PF5` is baosec).
- `PB14` and `PB13` are `TX` and `RX` pairs of a serial console, set to 1,000,000 baud 8N1.

### Updating Boot1

`boot1` is responsible for managing application loading. As such, updating `boot1` requires an intermediate step, because the actively executing program cannot overwrite its contents safely. The overview for updating `boot1` is as follows:

1. Load `boot1-alt` into the `baremetal` region.
2. Run `boot1-alt`, thus freeing `boot1` to be updated.
3. Copy the updated `boot1` record while in the `boot1-alt` environment.
4. Reboot back into the `boot1` environment.

#### Detailed Boot1 Update for Dabao Users

[!TIP]
You can fetch pre-built verions of the .uf2 files from the CI pipeline [here](https://ci.betrusted.io/latest-ci/baochip/bootloader/). Once Baochip hits release status, we'll drop a link for a stable release version here as well.

1. Build alt-boot1: `cargo xtask bao1x-alt-boot1`
2. Build boot1: `cargo xtask bao1x-boot1`
3. Plug the dabao board into the host. Confirm that the mass storage device has the volume label of `BAOCHIP`.
4. Copy `target/riscv32imac-unknown-none-elf/release/bao1x-alt-boot1.uf2` into the `BAOCHIP` volume.
5. Press `PROG` (the button closest to the USB connector)
6. The board should unmount itself and re-mount as a volume with the label of `ALTCHIP`.
7. Copy `target/riscv32imac-unknown-none-elf/release/bao1x-boot1.uf2` into the `ALTCHIP` volume.
8. Press `RESET` (the button farthest from the USB connector)
9. The device should re-appear as `BAOCHIP`
10. Confirm boot1 update by running the `audit` command on the boot1 console and checking that the reported git revision matches that of the source build.

### Applications

Three application targets are supported by Xous:

- `baremetal` is an unsecured, bare-iron environment. It is `no-std`, but comes with `alloc` pre-initialized and a USB serial console.
- `dabao` is a Xous environment. It boots a full kernel, supports `std`, and features "detached-apps", i.e., users can develop stand-alone applications that run on the OS without having to touch the kernel image.
- `baosec` is a Xous environment. It is like `dabao`, but supports swap memory. As a result it can run much larger, more complicated applications. The on-chip RRAM is reserved for the kernel, while the off-chip swap contains user applications.

Code is delivered via `boot1`. `boot1` is entered by holding down a button while power the device on. The device will show up as a USB mass storage device, at which point a .uf2 file containing the application image is copied to the device, and the update is applied.

## Security Model

The default `bao1x` chips come from the factory with four public keys burned into them:

- Two code deployment signing keys
- One beta signing key
- One developer key (anyone can use the [private key](./devkey/README.md) to sign a developer image)

In addition to these public keys, a collection of initial identifiers and secrets are generated on-device which serve as the security root. Each boot stage has a copy of these public keys; thus the key set can be updated/changed for downstream artifacts. boot0/boot1 should be rarely, if ever, updated; meanwhile detached application images running within the OS environment may have more permissive signing policies since memory protection is enforced by the OS.

By default, the `bao1x` will accept and run developer images; however upon encountering a developer image anywhere in the boot chain, all the initial secrets are erased, and a one-way counter is incremented to indicate the device is a developer device. From this point developers can generate new keys, but any device attestation or on-chip secrets programmed into the device are lost.

The developer key can be revoked by running `lockdown` at the `boot1` stage. `baosec` boards have this done at the factory prior to shipment, to prevent tampering in the supply chain.

## API Organization

The Baochip platform has the following API structure.

`apps-dabao`: Applications for the dabao platform

`apps-baosec`: Applications for the baosec platform

`bao1x-boot`: Secure bootloader chain for the Baochip platform. The
bootloader code has settings for three targets, `dabao`, `baosec`,
and `oem`. The `oem` setting is intended for developers who want
to do their own custom board.

`libs/bao1x-api`: contains hardware-abstracted API code to the hardware
layers (traits, some constants, enums, etc.). For example, "here is
an IO trait that lets you configure and set GPIO pins", which could
then be implemented in hardware or emulation. The APIs aren't entirely
generic across all SoCs because they are tweaked to accommodate
quirks of the bao1x SoC.

`libs/bao1x-hal`: contains board-specific driver codes that do not
require persistent services to be started. Possibly misnamed because
it also includes not just bao1x-chip items, but also e.g. peripherals
to the bao1x SoC, such as the PMIC and camera.

`services/bao1x-hal-service`: contains the `main` process that manages
shared resources, such as UDMA, IO pins, IFRAM. These drivers cannot
be delegated to a `lib` crate because there can be only one instance
of these resources, and instead we have to dynamically allocate
access to these through IPC messaging.

`services/bao1x-emu`: hosted mode emulation of things in
bao1x-hal-service.

`services/bao-video`: contains a `main` process that integrates
the camera and OLED driver. This means that all graphic drawing
primitives also interface with this crate. These are condensed
into a single process space to speed up execution, and kept
separate from bao1x-hal-services because we want the video
services to not be blocked by, for example, a thread that is
handling I2C things.

## Glossary

The history of names in this SoC are complicated because the drivers
were developed in parallel with the legal entity that makes the chip
being formed and funded. Thus the name of the company and chip don't
even match compared to the final product. Here is a glossary of terms
you may encounter in this project.

`daric`: Internal code name for the SoC while in development.

`crossbar`: Baochip made it's SoC by "hitchiking" on Crossbar's tape-out. Crossbar
thus shouldered the cost of taping out the SoC, while Baochip made light-fingered
changes that made the chip amenable to running Xous. Most of the OSS SoC code
has its copyright assigned to this organization; so, this is probably the
most relevant third-party legal entity, even though its name doesn't appear
in marketing materials.

`baochip`: The name of a new company formed to market an OSS- and Xous-focused
variant of the `daric` chip. This is the relevant legal organization in terms
of purchasing chips and systems related to this code base; thus from a user
perspective this is the brand used in Xous.

`bao1x`: The short name of the Xous target that targets the `Baochip1x` SoC
(full part number: BAO1X2S4F-WA). It is also used as a target for "pure SoC"
simulations; i.e., verilog RTL simulations where the peripherals are entirely
virtual test benches.

`baosec`: Internal code name of a board that contains the `Baochip1x` with
a USB security token form factor. This is likewise the name of the xtask target
to build images for this board. Contains a camera, display, storage, USB
and buttons. `board-baosec` is the flag for the board target, and `loader-baosec`
is an analogous flag but for no-std environments. `hosted-baosec` likewise
is for `baosec` but running on an x86 host (for UX development).

`baosec-emu`: xtask target for hosted mode emulation for `baosec`. `bao1x-emu`
contains hosted mode shims for the `baosec` target. `bao1x-emu` mis-named and
should probably be renamed to `baosec-emu`.

`baosor`: Internal code name of a board that contains the `baochip1x` with
the Precursor form factor. `board-basor` is the flag for the board target,
and `loader-baosor` is an analogous flag but for no-std environments.

`dabao`: Internal codename of a breakout board for the `Baochip1x` that
contains nothing more than the chip, a power regulator and a USB connector.

