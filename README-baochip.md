# "Baochip" / "bao1x" Series API Organization & Glossary

## API Organization

The Baochip platform has the following API structure. Note that
`bao1x` is the original code name for the target chip, which after
two years of extensive code development migrated to the `baochip`
brand name. At the moment the misnaming stands, due to the amount
of code that carries the legacy name.

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

`crossbar`: The parent company that shouldered the cost of taping out the SoC.
Most of the OSS SoC code has its copyright assigned to this organization;
so, this is probably the most relevant legal entity even though its
name doesn't appear in marketing materials.

`baochip`: The name of a new company formed to market an OSS- and Xous-focused
variant of the `daric` chip. This is the relevant legal organization in terms
of purchasing chips and systems related to this code base; thus from a user
perspective this is the brand used in Xous.

`bao1x`: The short name of the Xous target that targets the `Baochip1x` SoC.
It is also used as a target for "pure SoC" simulations; i.e., verilog RTL
simulations where the peripherals are entirely virtual test benches.

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

