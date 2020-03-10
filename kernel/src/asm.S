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

    // Jump to a program to initialize global variables.
    // Note that at this point, the .data section should
    // have already been set up by the loader.
    jal x1, init

    // Call the main function.  This function diverges, so
    // shouldn't ever return.
    j main


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

    // Finally, save SP
    csrr        t0, sscratch
    STORE       t0, 1*REGBYTES(sp)

    // Restore a default stack pointer
    li          sp, 0xfffefffc

    // Note that registers $a0-$a7 still contain the arguments
    j           _start_trap_rust


/*
    Resume a context (_xous_resume_context)

    Restores all registers from a ProcessContext passed in $a0.
    Note that the SEPC and SSTATUS should be set already.
*/
.global _xous_resume_context
_xous_resume_context:
    move        sp, a0

    LOAD        x1, 0*REGBYTES(sp)
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
    // Skip past the 4-byte `ecall` instruction
    csrr        t0, sepc
    addi        t0, t0, 4
    csrw        sepc, t0

    LOAD        a7, 7*REGBYTES(a0)
    LOAD        a6, 6*REGBYTES(a0)
    LOAD        a5, 5*REGBYTES(a0)
    LOAD        a4, 4*REGBYTES(a0)
    LOAD        a3, 3*REGBYTES(a0)
    LOAD        a2, 2*REGBYTES(a0)
    LOAD        a1, 1*REGBYTES(a0)
    LOAD        a0, 0*REGBYTES(a0)

    li          sp, 0xff801000
    LOAD        t0, 1*REGBYTES(sp)
    csrw        sscratch, t0

    LOAD        x1, 0*REGBYTES(sp)
    // Skip SP for now
    LOAD        x3, 2*REGBYTES(sp)
    LOAD        x4, 3*REGBYTES(sp)
    LOAD        x5, 4*REGBYTES(sp)
    LOAD        x6, 5*REGBYTES(sp)
    LOAD        x7, 6*REGBYTES(sp)
    LOAD        x8, 7*REGBYTES(sp)
    LOAD        x9, 8*REGBYTES(sp)
    // Skip argument registers
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

    // Save SP
    csrr        t0, sscratch
    LOAD        x2, 1*REGBYTES(sp)
    sret


.global flush_mmu
flush_mmu:
    sfence.vma
    ret
