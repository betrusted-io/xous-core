# Processes in Xous

Xous supports multiple processes, each with their own address space.
Xous has no concept of threads, priority, or indeed even a runqueue.  It
only knows how to pass messages, and that it should call the `idle
process` when there are no other tasks to run.

Multi-threading is optionally handled in userspace.

This document covers the Xous kernel-native multi-processing
implementation, along with several possible implementations of the idle
process.

## Kernel Processes

Each process is a self-contained entity.  It has its own memory space,
and can only interact with the kernel by making syscalls.  Processes are
heavyweight.  Each process takes up at least 16 kB of RAM:

- Root pagetable
- At least one leaf pagetable
- Context data
- RAM

Naturally, if a process uses more than one page for `.stack` + `.data`,
or if the `.text` section isn't XIP, or if it has a heap, it will
require more memory.

Note that because Xous will auto-expand the stack when it expires, to
save memory you can structure your program's memory space such that the
`.data` section occupies the upper portion of a page, with stack growing
down from beneath.  For example:

| Start | End | Description
| ===== | === | ===========
| 0xd0000900 | 0xd0000ffc | Data section
| .... | 0xd00008fc | Stack (grows down)

In this example, stack will start at address `0xd00008fc` and occupy the
same page as the data section.  If it grows past `0xd0000000`, then it
will pagefault and the kernel will allocate a new page.

### Process Data Structures

```rust
/// PIDs are 8 bits to reduce the size of the MMU table.
/// This may be revisited at a later time.  A `XousPid` of
/// 0 is invalid, and a `XousPid` of !0 (i.e. 0xff) is owned by the kernel
type XousPid = u8;

/// Represents one process inside the Xous operating system.
/// The memory manager will have memory tables that use the pid to
/// keep track of where memory is allocated, however that is not
/// part of the process table.
struct XousProcess {

    /// Process ID
    pid: XousPid,

    /// Parent Process ID
    ppid: XousPid,

    /// Various process-specific flags
    flags: XousFlags,

    /// MMU offset -- i.e. contents of the `satp` register
    // for this process.
    mmu_offset: usize,

    /// Last address of the stack pointer.
    sp: usize,

    /// The number of bytes that have been allocated to this
    /// process heap.  This can be changed with `sbrk`.
    heap_size: usize,
}
```

The kernel keeps track of what memory is owned by a particular process,
and handles the actual context switch (i.e. remapping the MMU and
saving/restoring registers when moving between processes).  It also
handles message routing.

### Message Data Structures

When a Xous process starts a server, the following structure is
allocated in the kernel:

```rust
struct XousServer {
    /// The textual name of this server, used when calling `client_connect()`
    name: String,

    /// The process ID of the controlling server
    pid: XousPid,

    /// Stack pointer of the thread blocked by `server_receive()`.
    /// If the process is not blocked, then this is `None`.
    sp: Option<usize>,
}

struct XousMessage {
    /// The process ID of the process that sent this message
    pid: XousPid,

    /// The stack pointer of the process that sent this message
    sp: usize,
}
```

### Message Routing

When a process wants to start a message server, it calls
`server_register()` to register itself with a particular name.  Then it
can call `server_receive()` in order to actually start to process
messages.  To reply to a message, the server must call `server_reply()`.

If a process wants to send a message to a server, it calls
`message_send()`.  If another process is waiting on a message (by
blocking on `server_receive()`), then that process is activated
immediately and it takes over the remaining quantum of the sending
process.  If there is no pending server, then the process yields its
timeslice and the idle process is resumed.

### Context Switches

During a context switch away from a process, all of its registers are
pushed to its stack, then the stack pointer is saved into the process'
`sp` entry.  Similarly, when switching to a process the memory tables
are restored and `sp` is recovered, and then all registers are restored
from the stack.

**If the kernel has nothing to do, it activates the parent process**.
If a process sends a message, or calls yield, or indicates that it has
no more work to do, the kernel will resume the parent process.  If there
is no parent process, the kernel will wait for an interrupt, and await a
message to be delivered from userspace.

### Implementing Multi Threading

A process' parent is allowed several specialized syscalls.  For example,
it has the ability to change the `sp` of a child process.  In order to
implement threads in userspace, the parent would keep track of what
threads are owned by a particular process and then call `resume_at()` in
order to resume a process at a different program counter.

## Resource-constrained "Embedded-style"

It is possible to use Xous to create an "embedded-style" PID 1 that runs
all computing inside a single process.  In this system, there would only
be a single process.

An interrupt handler would fire that calls `resume_at()` on the single
process in order to switch between various threads.  However, from the
perspective of the kernel, this is all taking place within a single
process.

## Multi-Threaded "Desktop-style"

This describes one possible task launcher that could be used to provided
multiple threads across multiple processes.

Note that in this system, `spawn()` would be implemented as a message
that gets sent to the task manager in order to call
`sys_process_spawn()`.  This would take care of setting up threads and
running the process.

### Process Structures

The following process structures are kept within the task manager,
hereafter called `task_manager`:

```rust

/// Runnable priority level -- higher values interrupt lower ones
type UserPriority = u8;

// type UserThreadList = [&UserThread];
type UserThreadList = Vec<UserThread>;

enum UserProcessState {
    /// This process does not exist, and is free to be allocated.
    /// Seen when using statically-allocated process tables.
    Empty,

    /// The process was created, but hasn't been set up yet.
    /// It is still missing things such as a text section.
    Created,

    /// Process is ready for execution
    Ready,

    /// This process is currently running
    Running,

    /// All threads are blocking waiting on a message
    WaitMessage,
}

/// Desktop-style process wrapper in userspace around the kernel structure
struct UserProcess {

    /// Kernel PID of this process
    pid: XousPid,

    /// Priority of this process -- higher values interrupt lower ones
    priority: UserPriority,

    /// Current runnable state of this process
    state: UserProcessState,

    /// All threads of this process that share a memory space.
    /// If this goes to 0, then the process must terminate.
    threads: UserThreadList,
}
```

### Threads

Each process has one or more threads.  If all of the threads exit, then a process will crash.

```rust
/// TIDs are 16-bits so we can have lots of threads per-process.
type UserTid = u16;

/// Current runnable state of an individual thread.
enum UserThreadState {
    /// This thread doesn't exist, and is free for use.  This
    /// is used when thread tables are statically allocated.
    Empty,

    /// Thread is ready for execution
    Runnable,

    /// Thread is currently running
    Active,

    /// Thread has terminated, and this slot is free for reuse
    Terminated,
}

/// One thread of execution. Every process must have at least one
/// thread.  Note that the current definition here is two words (8 bytes)
/// of overhead per thread, plus the stack space required to do a
/// context switch.
struct UserThread {
    /// Thread ID
    tid: UserTid,

    /// Current runnable state of this thread
    state: UserThreadState,

    /// Priority of this thread -- higher values interrupt lower ones
    priority: UserThreadPriority,

    /// Address of the stack pointer.  This is only valid when
    /// the `state` is `Ready`.
    /// Upon context switch, the entire register set is pushed
    /// to the stack and the resulting `$sp` value is stored here.
    /// When resuming, `$sp` is restored first, and then the register
    /// set is restored from the stack.
    sp: usize,

    /// Proces that this thread is waiting on.  When this thread
    /// is to be resumed, this process will be woken up instead.
    wait_process: Option<XousPid>,
}
```

## Process Creation

A process is created with the `process_create()` syscall:

```rust
pub fn sys_process_create(pages: [XousPage], base_address: usize) -> XousResult<XousPid, XousError>
```

This will move the memory pages passed in to the new process to the base
address specified in `base_address`, and return the new PID.  These
pages are removed from the current memory space regardless of whether
the call succeeds.

1. The kernel allocates a new `XousProcess`
    * A new MMU page is allocated to the kernel, which will become
      `mmu_offset`.  This page is cleared to 0.
    * The `heap_size` is set to 0
    * `$sp` is set to `$STACK_BASE-$REG_COUNT*usize`

At this point, the process is ready to be run.  The value of `$pc`
should be correctly set in the initial pages that get passed to
`process_create()`.

The initial program loader should take care of extending its own address
space if necessary.  For processes that don't require additional memory,
the entire program can be passed to `process_create()`.
