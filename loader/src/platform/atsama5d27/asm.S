/*
 * SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#define FPU_NON_SECURE_ACCESS_OFFSET 10
#define FPU_ACCESS_CONTROL_OFFSET    20
#define FPU_FPEXC_EN_BIT             0x40000000

.section .text.init, "ax"
.text

.global reset
.align 4

reset:

/* Exception vectors (should be a branch to be detected as a valid code by the rom */
_exception_vectors:
  b   reset_vector    /* reset */
  b   undef_vector   /* Undefined Instruction */
  b   swi_vector     /* Software Interrupt */
  b   pabt_vector    /* Prefetch Abort */
  b   dabt_vector    /* Data Abort */
.word  _romsize  /* Size of the binary for ROMCode loading */
  b   irq_vector  /* IRQ : read the AIC */
  b   fiq_vector      /* FIQ */

undef_vector:
  bkpt /* Issue debugger breakpoint */
  b   undef_vector
swi_vector:
  bkpt /* Issue debugger breakpoint */
  b   swi_vector
pabt_vector:
  bkpt /* Issue debugger breakpoint */
  b   pabt_vector
dabt_vector:
  bkpt /* Issue debugger breakpoint */
  b dabt_vector
rsvd_vector:
  bkpt /* Issue debugger breakpoint */
  b   rsvd_vector
irq_vector:
  bkpt /* Issue debugger breakpoint */
  b   irq_vector
fiq_vector:
  bkpt /* Issue debugger breakpoint */
  b   fiq_vector
reset_vector:

/*
 * We must configure VBAR for the reset vectors to be at
 * the start of memory (_mem_start)
 */
mrc p15, 0, r2, c12, c0, 0 /* Read VBAR into R2 */
ldr  r2, =_mem_start
mcr  p15, 0, r2, c12, c0, 0

/* Enable FPU */
/* Grant non secure access for CP10 and CP11 */
mrc  p15, 0, r0, c1, c1, 2
orr  r0, r0, #3 << FPU_NON_SECURE_ACCESS_OFFSET
mcr  p15, 0, r0, c1, c1, 2
/* Set CP10 and CP11 access permission (Privileged and User mode) */
ldr  r0, =(0xF << FPU_ACCESS_CONTROL_OFFSET)
mcr  p15, 0, r0, c1, c0, 2
/* Set the FPEXC EN bit to enable the FPU (and NEON instructions) */
mov  r1, #FPU_FPEXC_EN_BIT
vmsr FPEXC, r1

/* Init the stack */
_init_stack:

ldr r0, =_top_of_memory

# Set the stack for IRQ mode
msr cpsr_c, #0xd2
mov sp, r0

# Set the stack for FIQ mode
msr cpsr_c, #0xd1
mov sp, r0

# Set the stack for Abort mode
msr cpsr_c, #0xd7
mov sp, r0

# Set the stack for Undefined Instruction mode
msr cpsr_c, #0xdb
mov sp, r0

# Back to Supervisor mode, (IRQ and FIQ both masked, Arm instruction
# set) set the stack for Supervisor mode
msr cpsr_c, #0xd3
mov sp, r0

/* Clear Abort condition if it is pending with help of the abort handler */
ldr  r1, =(0x100)
mrs  r0, cpsr
eor  r0, r0, r1
msr  cpsr_x, r0
nop
eor  r0, r0, r1
msr  cpsr_x, r0

/* Jump to the entrypoint */
_branch_loader_main:
    # Loader arguments: kernel image address, signature
    ldr r0, =(0x20100000) /* The 0x20100000 is a hardcoded kernel image address in the DRAM for dev purposes */
    mov r1, #0
    bl rust_entry
    b .

.global start_kernel
start_kernel:
  # Set TTBR0 and TTBR1 to point to the kernel page table stored in arg #2 (r1)
  mcr p15, 0, r1, c2, c0, 0
  mcr p15, 0, r1, c2, c0, 1

  # Set TTBCR to the default value
  ldr r1, =(0x00)
  mcr p15, 0, r1, c2, c0, 2

  # Disable domain permission checks for now
  ldr r1, =(0xffffffff)
  mcr p15, 0, r1, c3, c0, 0

  # Configure the system
  mrc p15, 0, r1, c1, c0, 0
  orr r1, #0x1 /* MMU enable */
  orr r1, #0x10000000 /* TRE=1 */
  mcr p15, 0, r1, c1, c0, 0
  nop
  nop
  nop

  # Prevent speculative access after MMU is enabled
  isb
  dsb

  # Flush caches and branch predictor
  mcr p15, 0, r1, c7, c5, 0
  mcr p15, 0, r1, c7, c5, 6
  isb
  dsb

_prep_args:
  # Back up kernel stack pointer into r4
  mov r4, r0
  # Back up kernel entrypoint into r5
  mov r5, r2

  # Prepare arguments for the kernel `init` function (entrypoint)
  mov r0, r3
  pop {r2, r1}

  # Restore kernel stack pointer from the r4
  mov sp, r4

  # Make room for `r11` and `lr` on the stack.
  # Because the kernel `init` function's prelude will push these registers onto the stack.
  # But currently the `sp` points at the very end of the stack area and without this correction,
  # pushing more values overflows it
  sub sp, #8

  # Jump to kernel
  bx r5

.global __aeabi_unwind_cpp_pr0
__aeabi_unwind_cpp_pr0:
  b __aeabi_unwind_cpp_pr0
