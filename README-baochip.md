# "Baochip" / "Cramium" Series API Organization & Glossary

## API Organization

The Baochip platform has the following API structure. Note that
`cramium` is the original code name for the target chip, which after
two years of extensive code development migrated to the `baochip`
brand name. At the moment the misnaming stands, due to the amount
of code that carries the legacy name.

`libs/cramium-api`: contains hardware-abstracted API code to the hardware
layers (traits, some constants, enums, etc.). For example, "here is
an IO trait that lets you configure and set GPIO pins", which could
then be implemented in hardware or emulation. The APIs aren't entirely
generic across all SoCs because they are tweaked to accommodate
quirks of the cramium SoC.

`libs/cramium-hal`: contains board-specific driver codes that do not
require persistent services to be started. Possibly misnamed because
it also includes not just cramium-chip items, but also e.g. peripherals
to the cramium SoC, such as the PMIC and camera.

`services/cramium-hal-service`: contains the `main` process that manages
shared resources, such as UDMA, IO pins, IFRAM. These drivers cannot
be delegated to a `lib` crate because there can be only one instance
of these resources, and instead we have to dynamically allocate
access to these through IPC messaging.

`services/cramium-emu`: hosted mode emulation of things in
cramium-hal-service.

`services/bao-video`: contains a `main` process that integrates
the camera and OLED driver. This means that all graphic drawing
primitives also interface with this crate. These are condensed
into a single process space to speed up execution, and kept
separate from cramium-hal-services because we want the video
services to not be blocked by, for example, a thread that is
handling I2C things.

## Glossary

The history of names in this SoC are complicated because the drivers
were developed in parallel with the legal entity that makes the chip
being formed and funded. Thus the name of the company and chip don't
even match compared to the final product. Here is a glossary of terms
you may encounter in this project.

`daric`: Code name for the MPW (multi-project-wafer) test SoC. It was
also supposed to be a product name but apparently this was already
taken and thus while lots of code uses the name, it can't be used
as a product name.

`cramium`: Putative name for a company that was supposed to be started
to sell what was the `daric` SoC, and was assumed to be the brand
name for the chip. As of writing the entity may still not exist.

`crossbar`: The parent company that was supposed to spin out `cramium`.
Most of the OSS SoC code has its copyright assigned to this organization;
so, this is probably the most relevant legal entity even though its
name doesn't appear in marketing materials.

`baochip`: The name of a new company formed to market an OSS- and Xous-focused
variant of the `daric` chip. This is the relevant legal organization in terms
of purchasing chips and systems related to this code base; thus from a user
perspective this is the brand used in Xous.

`baochip zero`: Actual brand name of the chip. Internally code named `nto` (which
stands for "Next Tape Out" - look, I don't come up with half of these names,
I just use them).

`cramium-soc`: The name of the Xous target that is actually the `baochip zero`.
It is also used as a target for "pure SoC" simulations; i.e., verilog RTL
simulations where the peripherals are entirely virtual test benches.

`cramium-fpga`: A placeholder target that is meant to be a down-sized
FPGA implementation of the SoC. This will likely be an Artix-7 100T
implementation, targeting the Digilent Arty A7.

`baosec`: Internal code name of a board that contains the `baochip zero` with
a USB security token form factor. This is likewise the name of the xtask target
to build images for this board. Contains a camera, display, storage, USB
and buttons. `board-baosec` is the flag for the board target, and `loader-baosec`
is an analogous flag but for no-std environments. `hosted-baosec` likewise
is for `baosec` but running on an x86 host (for UX development).

`baosec-emu`: xtask target for hosted mode emulation for `baosec`. `cramium-emu`
contains hosted mode shims for the `baosec` target. `cramium-emu` mis-named and
should probably be renamed to `baosec-emu`.

`baosor`: Internal code name of a board that contains the `baochip zero` with
the Precursor form factor. `board-basor` is the flag for the board target,
and `loader-baosor` is an analogous flag but for no-std environments.

`dabao`: Internal codename of a breakout board for the `baochip zero` that
contains nothing more than the chip, a power regulator and a USB connector.

