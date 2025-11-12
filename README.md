# Xous Core

Core files for the Xous microkernel operating system.

The [Xous Book](https://betrusted.io/xous-book/) covers the architecture and structure of the kernel.

The [wiki](https://github.com/betrusted-io/betrusted-wiki/wiki) is a community resource that answers many FAQ.

The [Baochip README](./README-baochip.md) is the starting point for Baochip users.

The [Precursor README](./README-precursor.md) is the starting point for Precursor users.

Xous is a mono-repo project. It contains the kernel, libraries, applications, and tools necessary to build full device images. Here is a brief description of the more important directories:

* **api**: "Secular" API libraries - minimal-dependency, common-core APIs used by core Xous services
* **apps**: Precursor applications
* **apps-dabao**: Dabao applications
* **apps-baosec**: Baosec applications
* **bao1x-boot**: Bao1x secure boot chain
* **baremetal**: Baremetal target - runs after the boot chain, for developers that don't want Xous but can still use Xous' `no-std` features
* **emulation**: Renode scripts used to emulate Xous
* **imports**: Vendored-in libraries that had to be forked for Xous compatibility
* **kernel**: core memory manager, irq manager, and syscall implementations.
* **libs**: Device driver libraries. The common thread is that none of these code bases contain a `main.rs`, just a `lib.rs`.
* **loader**: Sets up virtual memory space and boots the kernel
* **locales**: Tools for handling internationalization
* **signing**: Scripts for generating signed images
* **services**: Xous programs that support Xous apps. Middleware, if you will.
* **svd2utra**: A program for converting SVD chip descriptions in XML to the UTRA "header file" format
* **tools**: Programs used to construct a final boot image. Also the home of various diagnostic and test utilities.
* **utralib**: The [Unambigous Thin Register Abtraction](./utralib/README.md). The core hardware interface for Xous.
* **xous-ipc**: Inter-process communication convenience APIs for Xous
* **xous-rs**: API files for the kernel. Where the syscall interfaces live.
* **xtask**: The build system manager for Xous.

## Dependencies

Install the latest [Rust](https://rust-lang.org/tools/install/) or run `rustup update` to update your Rust installation prior to building. Xous development assumes a recent version of Rust.

## Build Commands

- Precursor: `cargo xtask app-image`
- Dabao: `cargo xtask dabao`
- Baosec: `cargo xtask baosec`
- Baochip baremetal: `cargo xtask baremetal-bao1x`

Additional apps to be bundled into images can be specified as extra arguments on the command line, e.g. `cargo xtask dabao helloworld` will generate a Dabao image that includes `helloworld` in the detached-app section. Features, app-features, loader-features, and so forth can also be passed as command line arguments; run `cargo xtask` on its own for more help.

## Emulation

- `cargo xtask run` will bring up an emulated Precursor
- `cargo xtask baosec-emu` will bring up an emulated Baosec
- `cargo xtask renode-image` will build an image suitable for [Renode](https://renode.io/#downloads) emulation of a Precursor target. Start renode by running `renode emulation/xous-release.resc`

## Building Documentation
A flag of `--feature doc-deps` must be passed when running `cargo doc`, like this:

`cargo doc --no-deps --feature doc-deps`

`doc-deps` is a dummy hardware target that satisfies the requirement of
a "board" when building documentation.

## Local-vs-crates.io Verification

`xtask` does a check every build to ensure that any `crates.io` dependencies
are synchronized with the contents of the monorepo. It protects against the
scenario where you edited a crate that exists in the Xous repo, but the
build system ignores the local edits because it's fetching an old version from
`crates.io`.

Developers working on published crates should patch them in the root `Cargo.toml`
file and bypass the check with `--no-verify`.

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
