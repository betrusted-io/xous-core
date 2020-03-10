
# Memory Layout

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

| Address    | Description
| ---------- | -----------
     0x10000
| 0x00100000 | Default entrypoint for riscv64-unknown-elf-ld (as shown by `riscv64-unknown-elf-ld --verbose`)
| 0x80000000 | Process stack top
| 0xff000000 | End of memory available to processes
| 0xff400000 | Page tables
| 0xff800000 | Process-specific data (such as root page table)
| 0xff801000 | Context data (registers, etc.)
| 0xff802000 | Return address from syscalls (never allocated)
| 0xffc00000 | Kernel arguments, allocation tables
| 0xffd00000 | Kernel binary image and data section
| 0xffff0000 | Kernel stack top
| 0xfff00000 | {unused}

Note that the stack pointer is not necessarily fixed, and may be changed
in a later revision.

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
