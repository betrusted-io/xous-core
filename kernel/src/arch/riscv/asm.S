#if __riscv_xlen == 64
# define STORE    sd
# define LOAD     ld
# define LOG_REGBYTES 3
#else
# define STORE    sw
# define LOAD     lw
# define LOG_REGBYTES 2
#endif
#define REGBYTES (1 << LOG_REGBYTES)

/*
    Entry point of all programs (_start).

    It initializes DWARF call frame information, the stack pointer, the
    frame pointer (needed for closures to work in start_rust) and the global
    pointer. Then it calls _start_rust.
*/


.section .text.init, "ax"

.global _start

_start:
    // Set trap handler, which will be called
    // on interrupts and cpu faults
    la t0, _start_trap
    csrw stvec, t0

    /////// resume prep code
    // if resume flag is false (0), skip this section
    beq         a7, zero, coldboot

    /*
       This is the resume code. The overall strategy is to have a suspend/resume cycle
       occur within an interrupt context. Thus, at this point of the suspend/resume
       cycle (e.g. the resume half), our strategy is to restore a minimal environment which
       allows a Xous interrupt context to run, which allows the Xous return-from-interrupt
       handler to set up the remainder of the resume tasks.
    */
    // Restore a default stack pointer - used by interrupt handlers and considered temporary/smashable
    li          sp, 0xfffefffc

    // setup sie [ssoft, sext]. This corresponds to bits 1 and 9, or 0b10_0000_0010
    li          t0, 0x202
    csrw        sie, t0

    // restore the SIM with the value previously socked away by the interrupt handler routine during the suspend half
    jal         ra, _enable_all_irqs

    // set scause to simulate an interrupt's occurence -- this makes the resume "feel like" an interrupt
    // vectored into the same routine that caused the suspend, but without having gone through the typical
    // interrupt handler mechanisms
    li          t0, 0x80000000
    csrw        scause, t0

    // this is the same path the suspend interrupt took, but we set it up manually
    // and the loader had flipped the "resume" bit, so this should bring us into the resume branch.
    j           _start_trap_rust

coldboot:
    // Note that registers $a0-$a7 still contain the kernel arguments

    // Jump to a program to initialize global variables.
    // Note that at this point, the .data section should
    // have already been set up by the loader.
    jal x1, init

    // Call the kmain function.  This function diverges, so
    // shouldn't ever return.
    j kmain


/*
    Trap entry point (_start_trap)

    Saves all process context into a structure that is
    mapped to each thread at 0xff801000.  Return happens
    elsewhere.
*/
.section .trap, "ax"
.global _start_trap

_start_trap:
    csrw        sscratch, sp
    li          sp, 0xff801000
    STORE       x1, 0*REGBYTES(sp)  // Store x1 in the scratch field
    LOAD        x1, 1*REGBYTES(sp)  // Load current context number
    slli        x1, x1, 7           // Multiply current context number by 32
    add         sp, sp, x1          // Set $sp to 0xff801000 + (current_context * 32)

    STORE       x1, 0*REGBYTES(sp)
    // Skip SP for now
    STORE       x3, 2*REGBYTES(sp)
    STORE       x4, 3*REGBYTES(sp)
    STORE       x5, 4*REGBYTES(sp)
    STORE       x6, 5*REGBYTES(sp)
    STORE       x7, 6*REGBYTES(sp)
    STORE       x8, 7*REGBYTES(sp)
    STORE       x9, 8*REGBYTES(sp)
    STORE       x10, 9*REGBYTES(sp)
    STORE       x11, 10*REGBYTES(sp)
    STORE       x12, 11*REGBYTES(sp)
    STORE       x13, 12*REGBYTES(sp)
    STORE       x14, 13*REGBYTES(sp)
    STORE       x15, 14*REGBYTES(sp)
    STORE       x16, 15*REGBYTES(sp)
    STORE       x17, 16*REGBYTES(sp)
    STORE       x18, 17*REGBYTES(sp)
    STORE       x19, 18*REGBYTES(sp)
    STORE       x20, 19*REGBYTES(sp)
    STORE       x21, 20*REGBYTES(sp)
    STORE       x22, 21*REGBYTES(sp)
    STORE       x23, 22*REGBYTES(sp)
    STORE       x24, 23*REGBYTES(sp)
    STORE       x25, 24*REGBYTES(sp)
    STORE       x26, 25*REGBYTES(sp)
    STORE       x27, 26*REGBYTES(sp)
    STORE       x28, 27*REGBYTES(sp)
    STORE       x29, 28*REGBYTES(sp)
    STORE       x30, 29*REGBYTES(sp)
    STORE       x31, 30*REGBYTES(sp)

    // Save SEPC
    csrr        t0, sepc
    STORE       t0, 31*REGBYTES(sp)

    // Save x1, which was used to calculate the offset.  Prior to
    // calculating, it was stashed at 0xff801000.
    li          t0, 0xff801000
    LOAD        t1, 0*REGBYTES(t0)
    STORE       t1, 0*REGBYTES(sp)

    // Finally, save SP
    csrr        t0, sscratch
    STORE       t0, 1*REGBYTES(sp)

    // Restore a default stack pointer
    li          sp, 0xfffefffc

    // Note that registers $a0-$a7 still contain the arguments
    j           _start_trap_rust


/*
    Resume a context (_xous_resume_context)

    Restores all registers from a Context passed in $a0.
    Note that the SEPC and SSTATUS should be set already.
*/
.global _xous_resume_context
_xous_resume_context:
    move        sp, a0

    LOAD        x1, 0*REGBYTES(sp)
    // Clear any reservations, invalidating any atomic operations
#ifndef SKIP_MULTICORE
    sc.w	    zero, x1, 0*REGBYTES(sp)
#endif

    // Skip SP for now
    LOAD        x3, 2*REGBYTES(sp)
    LOAD        x4, 3*REGBYTES(sp)
    LOAD        x5, 4*REGBYTES(sp)
    LOAD        x6, 5*REGBYTES(sp)
    LOAD        x7, 6*REGBYTES(sp)
    LOAD        x8, 7*REGBYTES(sp)
    LOAD        x9, 8*REGBYTES(sp)
    LOAD        x10, 9*REGBYTES(sp)
    LOAD        x11, 10*REGBYTES(sp)
    LOAD        x12, 11*REGBYTES(sp)
    LOAD        x13, 12*REGBYTES(sp)
    LOAD        x14, 13*REGBYTES(sp)
    LOAD        x15, 14*REGBYTES(sp)
    LOAD        x16, 15*REGBYTES(sp)
    LOAD        x17, 16*REGBYTES(sp)
    LOAD        x18, 17*REGBYTES(sp)
    LOAD        x19, 18*REGBYTES(sp)
    LOAD        x20, 19*REGBYTES(sp)
    LOAD        x21, 20*REGBYTES(sp)
    LOAD        x22, 21*REGBYTES(sp)
    LOAD        x23, 22*REGBYTES(sp)
    LOAD        x24, 23*REGBYTES(sp)
    LOAD        x25, 24*REGBYTES(sp)
    LOAD        x26, 25*REGBYTES(sp)
    LOAD        x27, 26*REGBYTES(sp)
    LOAD        x28, 27*REGBYTES(sp)
    LOAD        x29, 28*REGBYTES(sp)
    LOAD        x30, 29*REGBYTES(sp)
    LOAD        x31, 30*REGBYTES(sp)

    // Restore SP
    LOAD        x2, 1*REGBYTES(sp)
    sret


/*
    Return from a syscall with arguments (_xous_syscall_return_result)

    Xous passes arguments in $a0-$a7, however the RISC-V calling convention
    states that only $a0 and $a1 are used.  To work around this, we pass
    a pointer to a 32-byte structure in $a0, and this function moves that
    into the registers.  It then restores the context from other registers.
*/
.global _xous_syscall_return_result
_xous_syscall_return_result:

    move        sp, a1
    LOAD        t0, 31*REGBYTES(sp)
    csrw        sepc, t0

    LOAD        x1, 0*REGBYTES(sp)
    // An atomic operation is broken up into multiple instructions.
    // A program first issues a `lr` instruction, which basically
    // raises a flag saying that an atomic operation is now live. It
    // then issues an `sc` instruction, and if the flag is still held
    // it succeeds and clears the flag.
    //
    // This flag is a bit of state within the processor, and the
    // RISC-V spec doesn't say how to access it. However, it is only
    // one bit, and we can clear the bit by performing a dummy `sc`
    // here. This will cause a failure in any atomic operation in the
    // program we're switching to.
#ifndef SKIP_MULTICORE
    sc.w	    zero, x1, 0*REGBYTES(sp)
#endif

    // Skip SP for now
    LOAD        x3, 2*REGBYTES(sp)
    LOAD        x4, 3*REGBYTES(sp)
    LOAD        x5, 4*REGBYTES(sp)
    LOAD        x6, 5*REGBYTES(sp)
    LOAD        x7, 6*REGBYTES(sp)
    LOAD        x8, 7*REGBYTES(sp)
    LOAD        x9, 8*REGBYTES(sp)

    // Load argument registers from the value pointed
    // to by $a0
    LOAD        a7, 7*REGBYTES(a0)
    LOAD        a6, 6*REGBYTES(a0)
    LOAD        a5, 5*REGBYTES(a0)
    LOAD        a4, 4*REGBYTES(a0)
    LOAD        a3, 3*REGBYTES(a0)
    LOAD        a2, 2*REGBYTES(a0)
    LOAD        a1, 1*REGBYTES(a0)
    LOAD        a0, 0*REGBYTES(a0)

    LOAD        x18, 17*REGBYTES(sp)
    LOAD        x19, 18*REGBYTES(sp)
    LOAD        x20, 19*REGBYTES(sp)
    LOAD        x21, 20*REGBYTES(sp)
    LOAD        x22, 21*REGBYTES(sp)
    LOAD        x23, 22*REGBYTES(sp)
    LOAD        x24, 23*REGBYTES(sp)
    LOAD        x25, 24*REGBYTES(sp)
    LOAD        x26, 25*REGBYTES(sp)
    LOAD        x27, 26*REGBYTES(sp)
    LOAD        x28, 27*REGBYTES(sp)
    LOAD        x29, 28*REGBYTES(sp)
    LOAD        x30, 29*REGBYTES(sp)
    LOAD        x31, 30*REGBYTES(sp)

    // Restore SP
    //csrr        t0, sscratch
    LOAD        x2, 1*REGBYTES(sp)
    sret


.global flush_mmu
flush_mmu:
    sfence.vma
    ret

/*
    Endcap of valid RISC-V code, to ensure speculative reads
    past the end of the .text section to not trigger an illegal
    instuction opcode. This cap is equal to the width of a cache line,
    using the 2-byte RV32C encoding.
*/
.section .endcap, "ax"
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
