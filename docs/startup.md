# Xous Startup Sequence

This document describes how a running system is established.  It is, broadly,
divided into four stages:

0: Pre-boot -- setting the machine up to run
1: Initializing the allocator
2: Loading the kernel and PID1
3: Userspace setup

Once userspace is set up, it will continue to spawn additional processes
and maintain the system.  However, after this point there is no longer a
kernel-specific "stack".

Additionally, stages 1 and 2 may use a separate throw-away kernel that is
independent of the main kernel.  This could be because they are compiled
with PIC, whereas the main kernel is position-dependent.

## Pre-Boot Environment

A pre-boot environment is needed to pass kernel arguments.  It simply loads the argument structure into RAM, sets `$a0`, and jumps to the stage-1 kernel.

## Stage 1: Allocating memory pages

This stage-1 kernel allocates space for various kernel data structures according to kernel arguments.  At the end of this, the kernel will not rely on anything from the pre-boot environment.

1. Determine offsets of the following:
    * `PageAllocations`: This is based on the contents of `MBLK`
    * `ProcessTable`: Allocate enough bytes to store the process table
    * Exception stack: Add 4096 bytes to the amount of RAM
    * `KernelArguments`: If `NO_COPY` is not set, copy kernel arguments to RAM
    * `Kernel`: If `NO_COPY` is not set, add `LOAD_SIZE` to the amount copied
    * `Processes`: If `NO_COPY` is not set, add `LOAD_SIZE` for each `Init` process
1. Allocate that number of pages from the end of RAM
1. Create `PageAllocations` tables from args, beginning at the end of RAM
1. Allocate the `ProcessTable` structure just before `PageAllocations`
1. If `NO_COPY` is not set, copy other arguments to RAM.
1. Allocate one more page for stack, and set $sp to point there.

At this point, we don't need to rely on external memory anymore.  There is no MMU, so we're still running in Machine Mode.  We still need to set up memory mapping and allocate pages for various processes.

At the end of stage 1, the memory allocator is working.

## Stage 2: Loading the kernel and enabling the MMU

The kernel runs as PID1, with the MMU enabled.
The first process will eventually get turned into PID1, however at the start we're running without an associated userspace process.  In fact, we're even running without the kernel loaded.

1. Assign pages to the kernel and initial processes
1. Allocate page to hold root-level page table kernel and each initial process
1. Allocate second-level pages as necessary
1. Delegate all interrupts and exceptions to supervisor mode
1. Return to kernel and set the stack pointer, enabling MMU

The kernel is now running in Supervisor mode.  The MMU is enabled, but there still is no PID1.

## Stage 3: Getting the first process running

In this final stage, PID1 is transformed into a full-fledged process.

1. Allocate process ID for PID1
1. Allocate stack pages for PID1
1. Map text section for PID1
1. Enable MMU by returning to PID1 -- free existing stack beforehand.

## Stage 4: No more kernel process

At this point, the kernel has no more process.  If there is an exception, it will be handled using this stack.
