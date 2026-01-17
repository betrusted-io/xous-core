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

### Third Party Firmware

Baochip cannot be the ultimate guarantor of third party signature integrity. It doesn't have the resources or
risk profile to (for example) sign cryptocurrency wallets. Thus, developers looking to build high-value applications
on Baochip must also manage their own signing keys. To this end, a policy of "mutual distrust" is enforced by `boot0`.

#### Preventing Third Party Access to Baochip Secrets

Boot0 and Boot1 check the public key block presented by the next stage against a set of expected public
keys that should match the Baochip OEM keys. If any of these do not match, most of the Baochip secret
keys are erased before running the next stage. Thus any third party firmware may be free to inspect
Baochip's keyslots, but by the time it runs, those keyslots have been erased.

#### Preventing Baochip Access to Third Party Secrets

An bank of 4x 256-bit keys known as `collateral` keys are provided. These keys are always erased by
the boot0 / boot1 firmware whenever the public key block embedded in the header of the next stage matches Baochip's
expected public keys. The only condition when they are not erased is when key slots 0, 1, and 2 are all
different from Baochip's public keys (key slot 3 is a devkey slot, and anyone can sign an image for
that key slot, so it is not erased).

#### Details of the Mutual-Distrust Mechanism

The Mutual-Distrust mechanism aims to achieve the following goals:

1. Any firmware signed by a third party can't be used to decrypt Baochip secrets
2. Any firmware signed by Baochip can't be used to decrypt third party secrets

Before we get into how the above is implemented, some definitions:

`third party firmware`: Code signed and maintained by a third party. Once a third party has a Baochip-signed `boot1`, they can freely sign further downstream application images using their own signing keys; updates to `boot1` would require a fresh Baochip signature.

`key manifest`: Each stage contains an embedded header which contains a signature for the code in that stage.
Immediately after the signature and within the signed region, there is a `key manifest` that
consists of four ED25519 public keys. The `key manifest` declares what public keys the current
stage intends to use for verifying the next stage's code.

`reference keys`: Baochip's expected signing keys. These are burned into multiple redundant indelible locations. Collectively they are referred to as `reference keys`.

`Baochip secrets`: Baochip stores its secret keys in a block of key slots expressly reserved for Baochip's use. These are erased anytime trust is transferred to a new entity, or if trust is lost (e.g. going into developer mode).

`collateral`: A set of keys used to effect key destruction when crossing trust domains. To accomplish this, third party firmware *must* use at least 256 bits of the `collateral` key set in its master key derivation scheme.

`developer key`: A public key stored in slot 3 of the `key manifest` that corresponds to a well-known private key. Anyone can sign an image with a `developer key`.

Based on the above definitions, here are the mutual-distrust policies enforced by the Baochip bootloader:

1. `boot0` verifies that its `key manifest` matches the `reference keys`. If not, `Baochip secrets` are erased.
2. `boot0` verifies that `boot1`'s `key manifest` matches the `reference keys`. If any keys do not match, `Baochip secrets` are erased.
3. `boot0` checks if any non-`developer key` keys in `key manifest` match the `reference keys`. If any matching keys are found, the `collateral` is erased. Otherwise, `collateral` is preserved.

Let's observe what properties are guaranteed by this arrangement:

- If `boot0` and `boot1`'s `key manifest`s match the `reference keys`, then `Baochip secrets` are intact.
- If any of `boot1`'s `key manifest` non-developer entries match any of the `reference keys`, `collateral` is erased. Thus, any attempt to "downgrade" the firmware by loading a Baochip-signed image would not lead to third-party secret disclosure, because the `collateral` keys are part of the third party firmware's key derivation mechanism.
- If any of `boot1`'s `key manifest` does not match the `reference keys` or if `boot1` is signed by the `developer key`, most of Baochip's secrets are erased. Thus the process of loading third party firmware would also cause any Baochip secrets to be lost.

#### Conditions for Getting Signed Third-Party `boot1`

Baochip will *only* sign third-party `boot1` images after the proposed firmware meets the following tests:

1. The `boot1` `key manifest` block is entirely different (except for optionally the developer key) from Baochip's `reference keys`
2. The proposed firmware can demonstrate that it has initialized the `collateral` key slots by revealing the contents of `collateral` slot 3 via an introspection command (slots 0, 1, and 2 are private; each slot is 256 bits in length).
3. The proposed firmware demonstrates permanent loss of access to encrypted data if a Baochip-signed `boot1` is swapped in, and then reverted back to the third-party-signed `boot1`.
4. The same introspection command used in step 2 is run again. The resulting value must be different from the value reported in the original run of step 2.

The above four tests are written such that the test can be run without inspection of the details of the third party firmware, but ideally, Baochip can inspect the firmware to ensure the intended policies are in place.

Note that if the third party firmware developer fails to use the `collateral` keys correctly to derive its master key, it can be subject to exploitation by a Baochip-signed image. Baochip takes no responsibility for any damages that may occur in that event.

Baochip would also entertain giving third parties self-signed `boot0`s with indelible `reference keys` linked to their own keys, *but* this requires a minimum order of around 50,000 chips plus a per-lot engineering fee to retool the wafer probe infrastructure used to burn the keys into the chip (these numbers are just ballpark estimates; contact Baochip to finalize details). Thus for high-volume applications this is a viable option, while the third-party firmware mechanism is an economical option to bootstrap self-managed secure ecosystems.


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

