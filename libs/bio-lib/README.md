# BIO Libraries

This is where various BIO applications live.

## User Notes

BIO applications are partially composable, with resource conflicts worked out at runtime.
Every BIO application declares its resource usage, and its constructor checks with the
BIO service to see if there are sufficient resources to run the application.

Resources are allocated on a first-come, first-serve basis. Generally speaking, there
should always be the resources to run one BIO application at a time. Some applications
may be able to run concurrently, but this would often be more the exception than
the rule.

## Library Author Notes

Conventions:

- All libraries should implement the `Resources` trait. This returns a `ResourceSpec` which
enables the runtime tracking of what resources are used by which application.
- All libraries should implement a `Drop` trait that frees up the resources when the object
goes out of scope.
- The `baosec` platform reserves `FIFO0` and one core (`Core0`) to process the output of the TRNG. Applications intending to be compatible with this platform should thus avoid using these resources.

### API references

Traits are documented in-line with their definition:

- See `libs/bao1x-api/src/bio_resources.rs` for the `BioResources` trait. These traits are concerned with communicating to the dynamic resource tracker to determine if the application can be run on the current system.
- See `libs/bao1x-api/src/bio.rs` for the `BioApi` trait. These traits are concerned with configuring the actual hardware itself.

BIO assembly code must be wrapped in the `bio_code` macro. Arguments are:

- a name for the function that returns the code as a `&[u8]`
- a unique name that identifies the start of the code. The name must be unique across the entire application image, and the only requirement is that the name is unique.
- a unique name that identifies the end of the code. The name must be unique across the entire application image, and the only requirement is that the name is unique.
- a comma-delimited list of strings that are the assembly instructions themselves

### Register map reference

FIFO - 8-deep fifo head/tail access. Cores halt on overflow/underflow.
- x16 r/w  fifo[0]
- x17 r/w  fifo[1]
- x18 r/w  fifo[2]
- x19 r/w  fifo[3]

Quantum - core will halt until host-configured clock divider pules occurs,
or an external event comes in on a host-specified GPIO pin.
- x20 -/w  halt to quantum

GPIO - note clear-on-0 semantics for bit-clear for data pins!
  This is done so we can do a shift-and-move without an invert to
  bitbang a data pin. Direction retains a more "conventional" meaning
  where a write of `1` to either clear or set will cause the action,
  as pin direction toggling is less likely to be in a tight inner loop.
- x21 r/w  write: (x26 & x21) -> gpio pins; read: gpio pins -> x21
- x22 -/w  (x26 & x22) -> `1` will set corresponding pin on gpio
- x23 -/w  (x26 & x23) -> `0` will clear corresponding pin on gpio
- x24 -/w  (x26 & x24) -> `1` will make corresponding gpio pin an output
- x25 -/w  (x26 & x25) -> `1` will make corresponding gpio pin an input
- x26 r/w  mask GPIO action outputs

Events - operate on a shared event register. Bits [31:24] are hard-wired to FIFO
level flags, configured by the host; writes to bits [31:24] are ignored.
- x27 -/w  mask event sensitivity bits
- x28 -/w  `1` will set the corresponding event bit. Only [23:0] are wired up.
- x29 -/w  `1` will clear the corresponding event bit Only [23:0] are wired up.
- x30 r/-  halt until ((x27 & events) != 0), and return unmasked `events` value

Core ID & debug:
- x31 r/-  [31:30] -> core ID; [29:0] -> cpu clocks since reset

### ERRATUM

BUG 1: ("phantom rs1"): For lui/auipc/jal (U&J type instructions),
the 5-bit field at instruction bits [19:15] is not gated off. If that field
decodes to register 16-19 (x16-x19) for just these instruction types,
a spurious pending read is made from the corresponding FIFO,
which affects correctnes and can block execution.

BUG 2: ("phantom rs2"): For non-R/S/B-type instructions, the 5-bit
field at instruction bits [24:20] should be gated off but isn't.
When that field equals 20 (0b10100), a spurious `quantum` signal
triggers, which can affect the instruction fetch pipeline.

For coders compiling from C, these erratum are transparently
patched by the `clang2rustasm.py` script. Thus these erratum
are primarily a challenge to coders hand-writing in assembly.

For assembly hand-coders, there is a script called `erratum_check.py`
which will inspect code inside a `bio_code!` macro for patterns
that match the above bugs. It can also auto-patch, if you so desire,
using `--autopatch`.

#### Examples

The primary pitfalls happen in dealing with immediates, as follows.

Bug 1 example:

`lui a1, 0xa2f96`

Here, the `9` in the second digit of `0xa2f96` translates to register
number x18, which triggers the bug. The fix is to create the constant
with a sequence such as:

```
lui a1, 0xa2f06
lui a2, 0x9      # can't use 0x90 because that triggers the bug
slli a2, a2, 4   # shift 0x9 into place
add a1, a1, a2   # Final result. Note that a2 is side-effected.
```

Bug 2 example:

`slli x1, x2, 20`

Workaround (splitting the shift):

`slli x1, x2, 19`
`slli x1, x1, 1`

Add, xor, andi, ori are also affected, and the fix requires
using a temporary variable to compose an intermediate.

Loads are affected, but not stores. Loads with an offset of
20 (or containing any coding of 0x14 in the bit field) are
worked around by pre-decrementing the source register, adding
the the decrement to the offset, and then re-incrementing the
source register.

#### Root Cause

The root cause was a misunderstanding of the register field
decoders in the PicoRV. I got confused over the register names
and when I checked for gating, I looked at the output variables
of the register file, and not the input. Turns out the inputs
to the register file are very simple:

```
			decoded_rs1 <= mem_rdata_latched[19:15];
			decoded_rs2 <= mem_rdata_latched[24:20];
```

The good news is that on the write-side, there is an additional
signal that detects writes, so the bug is not triggered for write
pattern. While hand-coded test cases were used to check the function
of the implementation, coverage was limited. Realistic code-freeze
dates would have informed deeper investment in automated test tooling, but,
management issues are not the topic of this readme.

The hardware fix would be to extract the intermediate signals that indicate
the decoded instruction type and use them to gate the `quantum`
and FIFO access lines. However, this would require a full mask
spin and at a couple million dollars. That's just not going to happen;
thus, I get to live with owning this bug for the rest of my life.
