#define STORE    sw
#define LOAD     lw
#define LOG_REGBYTES 2
#define REGBYTES (1 << LOG_REGBYTES)

.global _xous_syscall
.global _xous_syscall_rust
_xous_syscall_rust:
_xous_syscall:
    ecall
    lw          t0, 0(sp)
    sw          a0, 0*REGBYTES(t0)
    sw          a1, 1*REGBYTES(t0)
    sw          a2, 2*REGBYTES(t0)
    sw          a3, 3*REGBYTES(t0)
    sw          a4, 4*REGBYTES(t0)
    sw          a5, 5*REGBYTES(t0)
    sw          a6, 6*REGBYTES(t0)
    sw          a7, 7*REGBYTES(t0)
    ret

.global riscv_cache_flush
riscv_cache_flush:
    fence
    ret
