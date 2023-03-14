# UTRA (Unambiguous Thin Register Abstraction) Library Crate

`utralib` is responsible for mapping a hardware target specification
to the set of physical memory locations used by the hardware.

The mapping is derived from an [SVD](https://www.keil.com/pack/doc/CMSIS/SVD/html/svd_Format_pg.html)
file. SVD files are XML descriptions of a set of registers and fields that
has been adopted by LiteX, but is also broadly used by other silicon
vendors such as [ST](https://github.com/tinygo-org/stm32-svd).

Rust strongly discourages [auto-generated source content](https://github.com/rust-lang/cargo/issues/5073)
so this crate pre-generates the various configurations from SVD files
and stores them in the directory "generated".

However, the generated UTRA descriptions will automatically
regenerate if the source SVD files are modified, so during development
one may replace an SVD file and expect that the generated.rs files
should automatically update to reflect the changed SVD file.

## Target selection

Target selection is done with a pair of flags, one which specifies
the general target, and the other which whittles it down to a revision
of the target.

For traditional silicon platforms, the revision almost never changes.
For FPGA platforms, the revision can change and the convention adopted
here is to use the short 32-bit gitrev to specify a revision. Because
the 32-bit namespace is fairly small, the target name is still bundled
in with the revision specifier.

Thus, to fully specify a Precursor target, one will need to pass two
flags:

- `--features precursor`: specifies the target
- `--features precursor-c809403`: specifies the revision

The target specifier is meant to simplify top-level crate decisions
about which module to use to implement hardware features.

The revision is provided for very specific situations where a hardware
feature may or may not be present in a specific revision of an FPGA SoC.

The feature system could also be (ab)used to, for example, specify
an STM32 family and then the specific chip configuration of that family
with the revision.

## Notes for the Maintainer

Before publishing the package, build each of the possible configurations
to ensure the statically generated files are up to date:

```
 cargo build
 cargo build --no-default-features --features renode
 cargo build --no-default-features --features hosted
 cargo build --no-default-features --features precursor --features precursor-pvt
 cargo build --no-default-features --features atsama5d27
```