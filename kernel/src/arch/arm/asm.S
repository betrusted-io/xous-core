/*
 * SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#define AIC         0xFC020000
#define SAIC        0xF803C000
#define AIC_SMR     0x04
#define AIC_IVR     0x10
#define AIC_EOICR   0x38

#define MODE_MSK         0x1F

#define ARM_MODE_FIQ     0x11
#define ARM_MODE_IRQ     0x12
#define ARM_MODE_SVC     0x13
#define ARM_MODE_ABT     0x17
#define ARM_MODE_UND     0x1B
#define ARM_MODE_SYS     0x1F
#define ARM_MODE_USR     0x10

#define I_BIT            0x80
#define F_BIT            0x40

.section .text.reset_vector, "ax"

.global reset
.align 4

reset:

_exception_vectors_kernel:
  b   reset_vector          /* Entrypoint */
  b   undef_vector          /* Undefined Instruction */
  b   trap_handler          /* Supervisor Call (used in syscalls) */
  b   trap_handler          /* Prefetch Abort */
  b   trap_handler          /* Data Abort */
.word  0 /* Empty */
  b   trap_handler          /* IRQ : read the AIC */
  b   trap_handler          /* FIQ */

undef_vector:
  bkpt /* Issue debugger breakpoint */
  b   undef_vector
reset_vector:

/* Branch to the kernel init and then to the kernel entrypoint */
_branch_main:
    # Configure VBAR to point to the kernel's interrupt vector table defined above
    mrc p15, 0, r4, c12, c0, 0 /* Read VBAR into r4 */
    ldr r4, =_exception_vectors_kernel
    mcr p15, 0, r4, c12, c0, 0

    /* Note that registers r0-r3 still contain the kernel arguments */
    bl init
    bl kmain
    b .

/*
    Syscall/IRQ/Abort trap handler.

    Saves all process context into a structure that is
    mapped to each thread at 0xff804000 (THREAD_CONTEXT_AREA).
    Return happens elsewhere.
*/
trap_handler:
    ldr ip, =(0xff804000)

    # If this is a syscall, then r0 contains a pointer to the syscall arguments structure
    # Due to it being clobbered by this routine's code, we have save it into the scratch area for future use
    str r0, [ip]     /* Preserve r0 in the scratch field */
    ldr r0, [ip, #4] /* Load current context number */
    lsl r0, #7       /* Multiply by 32 */
    add ip, ip, r0   /* ip = 0xff804000 + (context_number * 32) */

    # If the trap has originated in the user/system mode, then we need to store
    # the user/system mode registers that are currently banked out
    mrs r0, spsr          /* Access the previous mode register */
    and r0, r0, #0x1f     /* Extract the mode field */
    cmp r0, #ARM_MODE_SYS /* Compare with 'System' mode */
    beq _store_banked     /* Save banked out user registers if coming from System mode */
    cmp r0, #ARM_MODE_USR /* Compare with 'User' mode */
    beq _store_banked     /* Save banked out user registers if coming from User mode */

    ldr r0, =(0xff804000) /* Restore r0 from the scratch field */
    ldr r0, [r0]

    stm ip, {r0 - r15}    /* Save all the registers */

    b _skip_store_banked
_store_banked:
    # Save the hardware thread pointer register
    mrc p15, 0, r0, c13, c0, 2  /* Read hardware thread pointer register */
    add ip, ip, #(17 * 4)
    str r0, [ip]
    sub ip, ip, #(17 * 4)

    ldr r0, =(0xff804000) /* Restore r0 from the scratch field */
    ldr r0, [r0]

    stm ip, {r0 - r7}     /* Save the common registers */
    add ip, ip, #(4 * 8)
    stm ip, {r8 - r15}^   /* Save the banked registers */
    sub ip, ip, #(4 * 8)
_skip_store_banked:
    # Save SVC/IRQ mode's LR as a resume address
    add ip, #(4 * 19)
    str lr, [ip]
    sub ip, #(4 * 19)

    # Set a default exception/irq stack pointer (EXCEPTION_STACK_TOP)
    ldr sp, =(0xffff0000)

    # Choose where to go next depending on the current mode: SVC (syscall), abort or IRQ
    mrs ip, cpsr                /* Access the current mode register */
    and ip, ip, #0x1f           /* Extract the mode field */
    cmp ip, #ARM_MODE_SVC       /* Compare with 'SVC' mode */
    bleq _swi_handler_rust      /* Jump to SVC handler */
    cmp ip, #ARM_MODE_ABT       /* Compare with 'Abort' mode */
    bleq _abort_handler_rust    /* Jump to abort handler */
    cmp ip, #ARM_MODE_IRQ       /* Compare with 'IRQ' mode */
    bleq irq_handler            /* Jump to IRQ handler */
    cmp ip, #ARM_MODE_FIQ       /* Compare with 'FIQ' mode */
    bleq fiq_handler            /* Jump to FIQ handler */

    # Any other unhandled exception goes here
    bkpt
    b .

irq_handler:
fiq_handler:
  # Disable further IRQs
  mrs ip, cpsr
  orr ip, ip, #I_BIT | F_BIT  /* Disable interrupts */
  msr cpsr, ip

  bl _irq_handler_rust
  bkpt
  b .

/*
   A thread resume routine.
*/
.global _resume_trampoline
_resume_trampoline:
    # Restore banked SP and LR
    mov ip, r1
    add ip, ip, #(13 * 4)
    ldm ip, {sp, lr}^

    # Switch privilege modes.
    # Interrupts have to be disabled if we're in the IRQ mode because the code we're resuming to
    # will have to service the interrupt source first before allowing more interrupts
    mrs ip, cpsr                /* Access the current state register */
    and ip, ip, #0x1f           /* Extract the mode field */
    cmp ip, #ARM_MODE_IRQ
    mrs ip, spsr
    bic ip, ip, #I_BIT | F_BIT  /* Allow interrupts. They'll stay enabled unless we're in IRQ mode */
    blne skip_disabling_interrupts
    orr ip, ip, #I_BIT | F_BIT  /* Disable interrupts */
    skip_disabling_interrupts:
    bic ip, ip, #0x1f   /* Clear mode field */
    orr ip, ip, r0      /* Set the processor mode received in r0 argument */
    msr spsr, ip

    # Restore hardware thread pointer register
    mov ip, r1
    add r0, ip, #(17 * 4)
    ldr r0, [r0]
    mcr p15, 0, r0, c13, c0, 2 /* Set hardware thread pointer register */

    # Restore other registers
    mov ip, r1
    ldm ip, {r0 - r7}     /* Load the common registers */
    add ip, ip, #(4 * 8)
    ldm ip, {r8 - r11}^   /* Load the banked registers */
    sub ip, ip, #(4 * 8)

    # Save resume address into scratch cp15 register. This routine will jump to it if it's not zero
    mov sp, ip
    add sp, sp, #(4 * 19)
    ldr sp, [sp]
    mcr p15, 0, sp, c13, c0, 3   /* Store resume address into cp15 scratch register */

    # Set IP as a modified LR in case the thread/ISR return address is provided
    add ip, ip, #(4 * 18)
    ldr ip, [ip]   /* Load the IP register to hold optional return address */

    mrs sp, spsr
    msr cpsr, sp          /* Enter new processor mode */

    # ------------ <USER/SYSTEM MODE BOUNDARY> -------------

    # The IP register may hold an optional ISR/thread return address, check if it's not zero then set it as the LR
    cmp ip, #0x0
    beq skip_setting_ret_addr

    # Set alternative return address and jump to the resumed code
    mov lr, ip
    skip_setting_ret_addr:

    # Jump to resume address if it's not zero
    mrc p15, 0, ip, c13, c0, 3     /* Load resume address from cp15 scratch register */
    cmp ip, #0x0
    beq skip_jumping_to_resume_addr
    mov pc, ip     /* Jump to the resume address */
    skip_jumping_to_resume_addr:

    # (USER/SYSTEM MODE) Jump back to the syscall origin
    mov pc, lr

/*
    Return from a syscall with arguments.
*/
.global _xous_syscall_return_result
_xous_syscall_return_result:
    # Restore user mode's SP and LR from the context data
    mov ip, r0      /* r0 holds the resumed thread context */
    add ip, ip, #(13 * 4)
    ldm ip, {sp, lr}^

    # Restore hardware thread pointer register
    mov ip, r0      /* r0 holds the resumed thread context */
    add ip, ip, #(17 * 4) /* Load hardware thread pointer from context */
    ldr ip, [ip]
    mcr p15, 0, ip, c13, c0, 2   /* Set thread pointer register */

    # Restore thread context
    mov ip, r0                  /* r0 holds the resumed thread context */
    ldm ip, {r0 - r11}          /* Restore previously saved registers */

    # Restore resume address that was saved on syscall entry
    add ip, ip, #(4 * 19)
    ldr ip, [ip]
    mcr p15, 0, ip, c13, c0, 3  /* Set hardware return address (we use scratch register of cp15 for that) */

    # Switch to the user mode
    mrs ip, cpsr
    bic ip, ip, #0x1f
    orr ip, ip, #ARM_MODE_USR
    bic ip, ip, #I_BIT | F_BIT  /* Enable interrupts */
    msr cpsr, ip

    # ------------ <USER MODE BOUNDARY> -------------

    # (USER MODE) Jump back to the syscall origin stored in scratch cp15 register
    mrc p15, 0, ip, c13, c0, 3
    mov pc, ip

.global flush_mmu
flush_mmu:
    # Invalidate entire TLB
    mcr p15, 0, r0, c8, c3, 0
    mcr p15, 0, r0, c8, c7, 0
    isb
    dsb

    # Flush caches and branch predictor
    mcr p15, 0, r1, c7, c5, 0
    mcr p15, 0, r1, c7, c5, 6
    isb
    dsb

    # Return
    bx lr
