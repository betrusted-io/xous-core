.section .text.init, "ax"
.global _start

_start:
    li          t0, 0xffffffff
    csrw        mideleg, t0
    csrw        medeleg, t0

    // Place the stack pointer at the end of RAM
    lw          t0, 16(a0) // SRAM start
    lw          t1, 20(a0) // SRAM length
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

    // Return to Supervisor mode (and enable interrupts)
    li		    t0, (1 << 11) | (1 << 5)
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
