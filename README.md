# Xous Core

Core files for the Xous microkernel operating system.

You might find this [wiki](https://github.com/betrusted-io/betrusted-wiki/wiki) handy, as well as the [Xous Book](https://betrusted.io/xous-book/).

This repository contains everything necessary to build the Xous kernel
from source.  It consists of the following projects:

* **kernel**: core memory manager, irq manager, and syscallhandler
* **loader**: initial loader used to start the kernel
* **tools**: programs used to construct a final boot image
* **docs**: documentation on various aspects of Xous
* **emulation**: Renode scripts used to emulate Xous
* **xous-rs**: userspace library

## Dependencies

- Xous requires its own Rust target, `riscv32imac-unknown-xous-elf`. If you run `cargo xtask` from the command line, you should be prompted to install the target automatically if it does not already exist.
- You may need to remove the `target/` directory before building, if `rustc` continues to behave like it can't find the `xous` target even after it is installed.
- If you plan on doing USB firmware updates, you'll need `progressbar2` (updates) and `pyusb` (updates). Note that `pyusb` has name space conflicts with similarly named packages, so if updates aren't working you may need to create a `venv` or uninstall conflicting packages.
- If you are doing development on the digital signatures with the Python helper scripts, you will need: `pycryptodome` (signing - PEM read), `cryptography` (signing - x509 read), `pynacl` (signing - ed25519 signatures) (most users won't need this).
- Some system packages are needed, which can be installed with `sudo apt install libssl-dev libxkbcommon-dev` or similar
- If you receive an error about `feature resolver is required`, try installing a newer version of `rustc` and `cargo` via [rustup](https://rustup.rs)

## Building Documentation
A flag of `--feature doc-deps` must be passed when running `cargo doc`, like this:

`cargo doc --no-deps --feature doc-deps`

This flag is required because Xous requires a target board to be specified in
all build configurations. `doc-deps` specifies a set of dummy dependencies
that satisfy board requirements for the purpose of building documentation.

## Local-vs-crates.io Verification
By default the `xtask` resolver runs a check to confirm that your local files
match the ones referenced in `crates.io`. For a handful of core crates, the
build preferentially runs from what is on `crates.io`, so local changes have
no effect until they are pushed as an update to an existing crate. If you see
an error complaining about local source files not being published, make sure
you have the correct patches in place in your top level `Cargo.toml` file,
and bypass the check with `--no-verify`.

## Quickstart using Hosted Mode

You can try out Xous in a "hosted mode" wherein programs are compiled
for your native platform and are run locally as processes within your
current operating system. System calls are replaced with network calls
to a kernel that simply shuffles messages around.

Xous uses the [xtask](https://github.com/matklad/cargo-xtask/) convention,
where various complex build commands are stored under `cargo xtask`.
This allows for us to create arbitrarily complex build sequences
without resorting to `make` (which is platform-dependent),
`sh` (which requires a lot of external tooling), or another build
system.

To build a set of sample programs and run them all using the
kernel for communication, clone this repository and run:

```sh
cargo xtask run
```

This will build several servers and a "shell" program to control them
all. Most notably, a `graphics-server` will appear and kernel messages
will begin scrolling in your terminal.

### Hosted Mode UI navigation

| Precursor           | Host        |
| ------------------- | ----------- |
| D-pad middle button | Home        |
| D-pad up            | up arrow    |
| D-pad down          | down arrow  |
| D-pad left          | left arrow  |
| D-pad right         | right arrow |


## Quickstart using an emulator

Xous uses [Renode](https://renode.io/) as the preferred emulator, because
it is easy to extend the hardware peripherals without recompiling the
entire emulator.

Due to a breaking change in Renode, this codebase is only compatible with Renode equal to or later than `1.15.2.7965 （e6e79aad-202408180425)`

[Download Renode](https://renode.io/#downloads) and ensure it is in your path.
For now, you need to [download the nightly build](https://dl.antmicro.com/projects/renode/builds/),
until `DecodedOperation` is included in the release.

Then, build Xous:

```sh
cargo xtask renode-image
```

This will compile everything in `release` mode for RISC-V, compile the tools
require to package it all up, then create an image file.

Finally, run Renode and specify the `xous-release.resc` REnode SCript:

```sh
renode emulation/xous-release.resc
```

Renode will start emulation automatically, and will run the same set of programs
as in "Hosted mode".

## Generating a hardware image

To build for real hardware, you must specify an `.svd` file. This
file is generated by the SoC build process and describes a single
Betrusted core. These addresses will change as hardware is modified,
so if you distribute a modified Betrusted core, you should be sure
to distribute the `.svd` file.

The [UTRA](./utralib/README.md) abstracts the details of the register
locations, by wrapping them in logical names that don't change.
For Precursor, the SVD files are tracked inside `utralib/precursor/soc-<gitref>.svd`.
Since each soc.svd can potentially change with a git reference, a gitref
is coded into the filename by convention.

Generally, one can create an image for hardware using the following command:

```sh
cargo xtask app-image-xip
```

And it will pull from the default soc.svd configuration.

The currently selected config is set by the constant `PRECURSOR_SOC_VERSION`
in [xtask/src/main.rs](./xtask/src/main.rs); it is one of the first constants
near the top.

If you have built your own custom soc.svd file, the most convenient way to update
to this is to simply replace the file referenced in the default with yours,
and then run `cargo build` inside the `utralib` directory (not in the Xous
root -- the `build` command must happen inside the directory to force a
regeneration of the generated UTRA bindings). This will likely result
in a complaint when you run `xtask` that your local tree does not match what
is checked into `git`; if you are building from your own configuration,
that is correct, and thus you should add `--no-verify` to your `xtask` command
to suppress the check.

Note that adding a full extra custom gitrev is more involved, it involves
editing the [utralib/Cargo.toml](./utralib/Cargo.toml) and [utralib/build.rs](./utralib/build.rs)
to reference your new artifact as a brand new feature flag.

The resulting images are in your target directory (typically `target/riscv32imac-unknown-xous-elf/release/`)
with the names `xous.img` (for the kernel) and `loader.bin` (for its bootloader). The corresponding
gateware is in `precursors/soc_csr-<gitref>.bin`. These can be written to your
device by following the [update guide](https://github.com/betrusted-io/betrusted-wiki/wiki/Updating-Your-Device).

## Acknowledgement
This project is funded through the NGI0 PET Fund, a fund established by NLnet
with financial support from the European Commission's Next Generation Internet
programme, under the aegis of DG Communications Networks, Content and Technology
under grant agreement No 825310.

<table>
    <tr>
        <td align="center" width="50%"><img src="https://nlnet.nl/logo/banner.svg" alt="NLnet foundation logo" style="width:90%"></td>
        <td align="center"><img src="https://nlnet.nl/image/logos/NGI0_tag.svg" alt="NGI0 logo" style="width:90%"></td>
    </tr>
</table>
