# Syscalls in Xous

Syscalls enable communication between processes, as well as communication to the kernel.  These are guaranteed to never change, but new syscalls may be added.

Syscalls may take up to seven `usize` -bit arguments, and may return up to seven `usize` -bit output operands, plus a tag indicating success or failure. The size of `usize` may vary depending on processor type, and is always the width of a pointer.

## Syscall Representation

Depending on the platform, syscalls will have varying representation.

### Syscalls on RISC-V

RISC-V specifies eight registers as `argument` registers: `$a0` - `$a7` .  When performing a syscall, the following convention is used:

| Register | Usage (Calling) |
| -------- | --------------- |
| a0       | Syscall Number  |
| a1       | Arg 1           |
| a2       | Arg 2           |
| a3       | Arg 3           |
| a4       | Arg 4           |
| a5       | Arg 5           |
| a6       | Arg 6           |
| a7       | Arg 7           |

When returning from the syscall, these registers have the following meaning:

| Register | Usage (Return)  |
| -------- | --------------- |
| a0       | Return type tag |
| a1       | Arg 1           |
| a2       | Arg 2           |
| a3       | Arg 3           |
| a4       | Arg 4           |
| a5       | Arg 5           |
| a6       | Arg 6           |
| a7       | Arg 7           |

Note that this means that there is a hard limit on the number of arguments that can be passed. Additionally, the RISC-V calling convention specifies that only `$a0` and `$a1` may be used to return values. Xous expands this to allow eight return values, which currently requires an assembly shim.

### Syscalls on `std`

When built for Rust's `std` library, syscalls are sent via a network connection. Because pointers are unsafe to send, `usize` is defined on `std` as being 32-bits. Additionally, most syscalls will return `NotImplemented`.

Messages may be passed, however the contents of memory must be present on the wire.

| Offset (Bytes) | Usage (Calling)                           |
| -------------- | ----------------------------------------- |
| 0              | Syscall Number                            |
| 4              | Arg 1                                     |
| 8              | Arg 2                                     |
| 12             | Arg 3                                     |
| 16             | Arg 4                                     |
| 20             | Arg 5                                     |
| 24             | Arg 6                                     |
| 28             | Arg 7                                     |
| 32             | Contents of any buffer pointed to by args |

When returning, a memory buffer may be required. The contents of this buffer will be appended to the network packet in the same manner as the calling buffer.

| Offset (Bytes) | Usage (Return)                  |
| -------------- | ------------------------------- |
| 0              | Return type tag                 |
| 4              | Arg 1                           |
| 8              | Arg 2                           |
| 12             | Arg 3                           |
| 16             | Arg 4                           |
| 20             | Arg 5                           |
| 24             | Arg 6                           |
| 28             | Arg 7                           |
| 32             | Contents of any returned buffer |

## Syscall Types

Syscalls use specialized types, many of which are backed by `usize`. For example, a `MemoryAddress` is a `NoneZeroUsize`, which is the same size as `usize`. In this manner, programs can ensure that memory addresses cannot be `NULL`.

## Syscall Support Types

System calls are all tagged enums. Syscalls may not be made from within an interrupt context, unless the name ends in `I`, for example `ReturnToParentI`.

All syscalls contain a maximum of seven words (`usize`) of data, giving a total of eight words including the tag.

``` rust
pub type MemoryAddress = NonZeroUsize;
pub type MemorySize = NonZeroUsize;
pub type StackPointer = usize;
pub type MessageId = usize;

pub type PID = u8;
pub type MessageSender = usize;
pub type Connection = usize;

/// Server ID
pub type SID = (usize, usize, usize, usize);

/// Connection ID
pub type CID = usize;

/// Thread ID
pub type ThreadID = usize;

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

pub struct MemoryRange {
    pub addr: MemoryAddress,
    pub size: MemorySize,
}
```

## List of Syscalls

The list of syscalls is documented in the `xous` crate, inside [syscall.rs](../xous-rs/src/syscall.rs).
