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

.global vex_aes_enc_id_0
vex_aes_enc_id_0:
    .word       0x00b5050b // vex_aes_enc_id a0, a1, a0, #0
    ret

.global vex_aes_enc_id_1
vex_aes_enc_id_1:
    .word       0x10b5050b // vex_aes_enc_id a0, a1, a0, #1
    ret

.global vex_aes_enc_id_2
vex_aes_enc_id_2:
    .word       0x20b5050b // vex_aes_enc_id a0, a1, a0, #2
    ret

.global vex_aes_enc_id_3
vex_aes_enc_id_3:
    .word       0x30b5050b // vex_aes_enc_id a0, a1, a0, #3
    ret

.global vex_aes_enc_id_last_0
vex_aes_enc_id_last_0:
    .word       0x04b5050b // vex_aes_enc_id_last a0, a1, a0, #0
    ret

.global vex_aes_enc_id_last_1
vex_aes_enc_id_last_1:
    .word       0x14b5050b // vex_aes_enc_id_last a0, a1, a0, #1
    ret

.global vex_aes_enc_id_last_2
vex_aes_enc_id_last_2:
    .word       0x24b5050b // vex_aes_enc_id_last a0, a1, a0, #2
    ret

.global vex_aes_enc_id_last_3
vex_aes_enc_id_last_3:
    .word       0x34b5050b // vex_aes_enc_id_last a0, a1, a0, #3
    ret

.global vex_aes_dec_id_0
vex_aes_dec_id_0:
    .word       0x02b5050b // vex_aes_dec_id a0, a1, a0, #0
    ret

.global vex_aes_dec_id_1
vex_aes_dec_id_1:
    .word       0x12b5050b // vex_aes_dec_id a0, a1, a0, #1
    ret

.global vex_aes_dec_id_2
vex_aes_dec_id_2:
    .word       0x22b5050b // vex_aes_dec_id a0, a1, a0, #2
    ret

.global vex_aes_dec_id_3
vex_aes_dec_id_3:
    .word       0x32b5050b // vex_aes_dec_id a0, a1, a0, #3
    ret

.global vex_aes_dec_id_last_0
vex_aes_dec_id_last_0:
    .word       0x06b5050b // vex_aes_dec_id_last a0, a1, a0, #0
    ret

.global vex_aes_dec_id_last_1
vex_aes_dec_id_last_1:
    .word       0x16b5050b // vex_aes_dec_id_last a0, a1, a0, #1
    ret

.global vex_aes_dec_id_last_2
vex_aes_dec_id_last_2:
    .word       0x26b5050b // vex_aes_dec_id_last a0, a1, a0, #2
    ret

.global vex_aes_dec_id_last_3
vex_aes_dec_id_last_3:
    .word       0x36b5050b // vex_aes_dec_id_last a0, a1, a0, #3
    ret

.global riscv_cache_flush
riscv_cache_flush:
    fence
    ret
