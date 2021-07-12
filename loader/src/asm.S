.section .text.init, "ax"
.global _start

_start:
    li          t0, 0xffffffff
    csrw        mideleg, t0
    csrw        medeleg, t0

    // decorate our stack area with a canary pattern
    li          t1, 0xACE0BACE
    li          t0, 0x40FFE01C  // currently allowed stack extent - 8k - (7 words) - 7 words for kernel backup args - see bootloader in betrusted-soc
    li          t2, 0x41000000
fillstack:
    sw          t1, 0(t0)
    addi        t0, t0, 4
    bltu        t0, t2, fillstack

    // Place the stack pointer at the end of RAM
    li          t0, 0x40000000 // SRAM start   hard-coded into loader -- don't trust kernel boot args to tell us where RAM is, we haven't validated them yet!
    li          t1, 0x01000000 // SRAM length
    add         sp, t0, t1

    // Install a machine mode trap handler
    la          t0, abort
    csrw        mtvec, t0

    // Start Rust
    j   rust_entry

// This is used only in debug mode.
.global abort
abort:
    j           abort

.global start_kernel
start_kernel:
    // Delegate as much as we can supervisor mode
    li          t0, 0xffffffff
    csrw        mideleg, t0
    csrw        medeleg, t0

    // Return to Supervisor mode (1 << 11) when we call `reti`.
    // Disable interrupts (0 << 5)
    li		    t0, (1 << 11) | (0 << 5)
    // If arg6 is "true", also set mstatus.SUM to allow the kernel
    // to access userspace memory.
    andi        a6, a6, 1
    slli        a6, a6, 18
    or          t0, t0, a6
    csrw	    mstatus, t0

    // Enable the MMU (once we issue `mret`) and flush the cache
    csrw        satp, a3
    sfence.vma

    // Return to the address pointed to by $a4
    csrw        mepc, a4

    // Reposition the stack at the offset passed by $a5
    move        sp, a5

    // Issue the return, which will jump to $mepc in Supervisor mode
    mret
