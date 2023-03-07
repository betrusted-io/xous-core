
# Memory Layout

You definitely want to refer to the [Xous Book](https://betrusted.io/xous-book/ch03-01-memory-layout.html#virtual-memory-regions) if you're trying to debug a kernel panic, before reading on here.

Xous assumes a memory-mapped IO system.  Furthermore, it assumes there
is one section of "general-purpose RAM", and zero or more additional
memory sections.  Finally, it assumes there is an MMU.

Memory is divided into "pages" of 4096 bytes, and are allocated based on
this number.  It is considered an error to request memory that isn't
aligned to this address, and it is an error to request a multiple of pages
that is different from this number.  For example, you cannot request 4097
bytes -- you must request 8192 bytes.

Memory is allocated on a first-come, first-served basis.  Physical addresses
may be specified when allocating memory, in which case they are taken from
that physical address.  Otherwise, they are pulled from the "general-purpose
RAM" section.

A process can request specific memory ranges to be allocated.  For
example, a `uart_server` might request the UART memory region be
allocated so that it can handle that device and provide a service.  This
region cannot be re-mapped to another process until it is freed.

A process can request more memory for its heap.  This will pull memory
from the global pool and add it to that process' `heap_size`.  Processes
start out with a `heap_size` of 0, which does not include the contents
of the `.text` or `.data` sections.

If a process intends to spawn multiple threads, then it must malloc that
memory prior to creating the thread.

## Special Virtual Memory Addresses

The last 16 megabytes of memory are reserved for use by the kernel.

Many of these pages are kernel bookkeeping on a per-process basis.  For
example, a process' pagetables are always mapped at `0xff400000`.

The last 4 megabytes are universal across all processes, and represent
the kernel's address space.  These are owned by the kernel, and are
available in every process in "Supervisor" mode.  This should prevent
the need from ever switching back to process 1.

Note that the kernel takes up a single 4 MB megapage, so it can be
assigned to every process simply by mapping megapage 1023 (`0xffc00000`).

```
| Address    | Description
| ---------- | -----------
| 0x00100000 | Default entrypoint for riscv64-unknown-elf-ld (as shown by `riscv64-unknown-elf-ld --verbose`)
| 0x80000000 | Process stack top
| 0xff000000 | End of memory available to processes
| 0xff400000 | Page tables
| 0xff800000 | Process-specific data (such as root page table)
| 0xff801000 | Context data (registers, etc.)
| 0xff802000 | Return address from syscalls (never allocated)
| 0xffc00000 | Kernel arguments, allocation tables
| 0xffcc0000 | Kernel GDB UART CSR page
| 0xffcd0000 | Kernel WFI CSR page
| 0xffce0000 | Kernel TRNG CSR page
| 0xffcf0000 | Supervisor UART CSR page
| 0xffd00000 | Kernel binary image and data section
| 0xfff80000 | Kernel stack top
| 0xffff0000 | "default" stack pointer (used by interrupt handlers)
| 0xfff00000 | {unused}
```

Note that the stack pointer is not necessarily fixed, and may be changed
in a later revision.

## Special Physical Memory Addresses

| Address    | Description
| ---------- | -----------
| 0x40000000 | Bottom of battery-backed main RAM (16MiB)
| 0x40FFDFFF | Top of memory available to Xous
| 0x40FFE000 | Clean suspend record - used by the system to indicate if we are coming out of a clean suspend state
| 0x40FFF000 | Loader stack - this is corrupted by a reboot, and should not be allocated by the kernel
| 0x40FFFFFF | Top of battery-backed main RAM


## Memory Whitelist

Memory is kept in a whitelist.  That is, when calling
`sys_memory_allocate()`, the address is first validated against a list
of known ranges.  This has two major benefits:

1. It prevents attacks where memory mirrors can be reused to access
   another process' memory.  For example, on Litex, the peripheral space
   is mirrored at both `0x70000000` and `0xe0000000`.  Without special
   handling, two different processes could map these two spaces and
   share memory.
2. We can limit the amount of memory that is needed to keep track of
   memory.  For example, if we had generic tables that expanded every
   time an invalid region was accessed, then a process could use up the
   kernel's memory by simply requesting every possible address.  In
   having a whitelist, we can statically allocate memory blocks to track
   memory usage.

## Memory Tables

Each valid memory page has an associated table entry.  This entry simply
contains a `XousPid`, to indicate which process the memory block belongs
to.  A `XousPid` of `0` is invalid, and indicates the region is free.  A
`XousPid` of `1` indicates the page belongs to the kernel.

In a system with ample amounts of memory, all valid memory page would
get its own memory table.  However, in resource-constrained systems, a
simple array is not suitable, and so a programmatic lookup table is used
instead.

## Kernel Arguments

There are several arguments that specify where kernel structures should
go.

### Extra Memory Regions

If additional memory is available, then it is passed in an `MREx` block.
This is a list in the following form, with each field being 32 bits of
little endian data:

```
MREx,$count,
$start1,$len1,$name
$start2,$len2,$name
...
```

This does not include system memory, which is passed via XArg.  Instead,
this is used for additional memory such as framebuffer ranges, IO
ranges, or memory-mapped SPI flash.  The name should be printable ASCII,
and is primarily used for debugging.

## Allocation Tables

Each page of memory has an entry in the allocation tables.  When
allocating a new page, Xous ensures that page is not currently allocated
to another process.  This ensures that each page of memory is only
assigned to one process at a time, unless that page is handed out as
shared.

Allocation tables have the following layout:

```rust
struct AllocationEntry {
    /// PID that owns this page.  `0` if this
    /// page is unallocated.
    pid: XousPid,
}

struct PageRange {
    /// A slice of all allocations within this range.
    entries: &[AllocationEntry],
}

struct PageAllocations {
    /// Each range of memory gets its own allocation table.
    /// ranges[0] is always defined as RAM,
    /// and is where memory comes from when
    /// no physical address is specified.
    ranges: &[PageRange],
}
```

## Page Tables

Each process requires its own page table.  The kernel will be mapped to
a fixed offset in each process, in order to save some RAM and make
context switches easier.

## RISC-V `RSW` and `V` Page Table Entry Fields

The RISC-V Page Table Entry specification reserves two bits in a field
called `RSW`.  Additionally, it states that if `V` is `0`, then the
entry is invalid and all other entries are `don't care`.  In particular,
the `RSW` fields are for use by the kernel for its own purposes.  Xous
uses these to keep track of borrowed memory and pre-allocating pages.

For the purposes of Xous, `PTE[8]` shall have the bit name `S`, and
`PTE[9]` shall have the bit name `P`.

| P[9] | S[8] | X[3] | W[2] | R[1] | V[0] | Meaning               |
| ---- | ---- | ---- | ---- | ---- | ---- | --------------------- |
|  0   |  0   |  0   |  0   |  0   |  0   | Page is unallocated |
|  0   |  0   |  0   |  1   |  0   |  0   | _Invalid_ |
|  0   |  0   |  1   |  1   |  0   |  0   | _Invalid_ |
|  0   |  0   |  0   |  0   |  0   |  1   | _Invalid_ |
|  0   |  0   |  0   |  1   |  0   |  1   | _Invalid_ |
|  0   |  0   |  1   |  1   |  0   |  1   | _Invalid_ |
|  0   |  0   | _x_  | _x_  | _x_  |  0   | Page is on-demand allocated, with permissions according to _RWX_ |
|  0   |  0   | _x_  | _x_  | _x_  |  1   | Page is allocated and valid, with permissions according to _RWX_ |
| _x_  |  1   | _x_  |  0   | _x_  |  1   | Page is immutably shared |
| _x_  |  1   |  0   |  0   | _x_  |  0   | Page is mutably shared (and is therefore unavailable) |

The `P[9]` bit indicates whether the page was writable prior to the borrow.
