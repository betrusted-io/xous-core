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

### API references

Traits are documented in-line with their definition:

- See `libs/bao1x-api/src/bio_resources.rs` for the `BioResources` trait.
- See `libs/bao1x-api/src/bio.rs` for the `BioApi` trait.

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