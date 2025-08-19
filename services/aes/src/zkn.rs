// See https://github.com/riscv/riscv-crypto/tree/main/benchmarks/aes/zscrypto_rv32
// for reference assembly code from which the routines in here are derived.
// Note that the order of the AES ops is shuffled across blocks versus ref code
// to break data dependencies and remove pipeline bubbles.

use cipher::{
    AlgorithmName, BlockBackend, BlockCipher, BlockClosure, BlockDecrypt, BlockEncrypt, BlockSizeUser, Key,
    KeyInit, KeySizeUser, ParBlocksSizeUser,
    consts::{U1, U16, U32},
    generic_array::GenericArray,
    inout::InOut,
};

use crate::*;

/// 128-bit AES block
pub type Block = GenericArray<u8, U16>;

pub(crate) type BatchBlocks = GenericArray<Block, U1>;

use core::fmt;

/*
    The ratified AES instruction format is as follows:
    SS10_DM1X_XXXXYYYYY000ZZZZ_Z011_0011
    - XXXXX is the register file source 2 (RS2)
    - YYYYY is the register file source 1 (RS1)
    - ZZZZZ is the register file destination
    - D=1 means decrypt, D=0 mean encrypt
    - M=1 means middle (full) round, M=0 means last round (=> e.g., !L)
    - SS specify which byte should be used from RS2 for the processing

    enc-mid
    0010_0110 => 26, 66, A6, E6
    enc-last
    0010_0010 => 22, 62, A2, E2
    dec-mid
    0010_1110 => 2E, 6E, AE, EE
    dec-last
    0010_1010 => 2A, 6A, AA, EA
*/

#[repr(align(32))]
#[derive(Default)]
struct AlignedCk128 {
    d: [u8; 16],
}

/// AES-128 round keys
pub(crate) type VexKeys128 = [u32; 60];

fn aes_key_schedule_128_wrapper(ck: &[u8]) -> VexKeys128 {
    let mut ck_a = AlignedCk128::default();
    ck_a.d.copy_from_slice(&ck);
    let mut rk: VexKeys128 = [0; 60];
    // Safety: safe because our target has the "zkn" RV32 extensions.
    unsafe { aes_key_schedule_128(&mut rk, &ck_a.d) };
    rk
}

#[target_feature(enable = "zkn")]
unsafe fn aes_key_schedule_128(rk: &mut VexKeys128, ck: &[u8]) {
    #[rustfmt::skip]
    unsafe {
        // a0 - uint32_t rk [AES_256_RK_WORDS]
        // a1 - uint8_t  ck [AES_256_CK_BYTE ]
        core::arch::asm!(
            "lw  a2,  0(a1)", // C0
            "lw  a3,  4(a1)", // C1
            "lw  a4,  8(a1)", // C2
            "lw  a5, 12(a1)", // C3
            // RK a0
            // RKP a6
            // CK a1
            // RKE t0
            // RCP t1
            // RCT t2
            // T1 t3
            // T2 t4

            "mv      a6, a0",
            "addi    t0, a0, 160",
            "la      t1, 50f",// t1 = round constant pointer

        "30:",            // Loop start

            "sw      a2, 0(a6)",         // rkp[0] = a2
            "sw      a3, 4(a6)",         // rkp[1] = a3
            "sw      a4, 8(a6)",         // rkp[2] = a4
            "sw      a5, 12(a6)",         // rkp[3] = a5

                                // if rke==rkp, return - loop break
            "beq     t0, a6, 40f",

            "addi    a6, a6, 16",        // increment rkp

            "lbu     t2, 0(t1)",         // Load round constant byte
            "addi    t1, t1, 1",         // Increment round constant byte
            "xor     a2, a2, t2",         // c0 ^= rcp

            // "ror32i t3, t4, a5, 8",        // tr = ROR32(c3, 8)
            "srli t4, a5, 8",
            "slli t3, a5, 32-8",
            "or   t3, t3, t4",

            "aes32esi a2, a2, t3, 0",   // tr = sbox(tr)
            "aes32esi a2, a2, t3, 1",   //
            "aes32esi a2, a2, t3, 2",   //
            "aes32esi a2, a2, t3, 3",   //

            "xor     a3, a3, a2",          // C1 ^= C0
            "xor     a4, a4, a3",          // C1 ^= C0
            "xor     a5, a5, a4",          // C1 ^= C0

            "j 30b",                   // Loop continue

        "50:",
            ".byte 0x01, 0x02, 0x04, 0x08, 0x10",
            ".byte 0x20, 0x40, 0x80, 0x1b, 0x36",

        "40:",
            "nop",  // was ret

            in("a0") rk.as_mut_ptr(),
            in("a1") ck.as_ptr(),
        );
    };
}

fn aes128_dec_key_schedule_asm_wrapper(user_key: &[u8]) -> VexKeys128 {
    let mut rk: VexKeys128 = [0; 60];
    let mut uk_a = AlignedCk128::default();
    uk_a.d.copy_from_slice(&user_key);

    unsafe {
        aes_key_schedule_128(&mut rk, &uk_a.d);
    }
    unsafe { aes128_dec_key_schedule_asm(&mut rk, &uk_a.d) };
    rk
}

#[target_feature(enable = "zkn")]
unsafe fn aes128_dec_key_schedule_asm(rk: &mut VexKeys128, user_key: &[u8]) {
    #[rustfmt::skip]
    unsafe {
        core::arch::asm!(
            // a0 - uint32_t RK [AES_256_RK_WORDS]
            // a1 - uint8_t  CK [AES_256_CK_BYTE ]
            // a2 - RKP
            // a3 - RKE
            // T0 - t0
            // T1 - t1

            "addi    a2, a0, 16",              // a0 = &rk[ 4]
            "addi    a3, a0, 160",             // a1 = &rk[40]

        "20:",

            "lw   t0, 0(a2)",              // Load key word

            "li        t1, 0",
            "aes32esi  t1, t1, t0, 0",     // Sub Word Forward
            "aes32esi  t1, t1, t0, 1 ",
            "aes32esi  t1, t1, t0, 2",
            "aes32esi  t1, t1, t0, 3",

            "li        t0, 0",
            "aes32dsmi t0, t0, t1, 0",     // Sub Word Inverse & Inverse MixColumns
            "aes32dsmi t0, t0, t1, 1",
            "aes32dsmi t0, t0, t1, 2",
            "aes32dsmi t0, t0, t1, 3",

            "sw   t0, 0(a2)",             // Store key word.

            "addi a2, a2, 4",            // Increment round key pointer
            "bne  a2, a3, 20b", // Finished yet?

            in("a0") rk.as_mut_ptr(),
            in("a1") user_key.as_ptr(),
        );
    };
}

#[repr(align(32))]
#[derive(Default)]
struct AlignedCk {
    d: [u8; 32],
}

fn aes_key_schedule_256_wrapper(ck: &[u8]) -> VexKeys256 {
    let mut ck_a = AlignedCk::default();
    ck_a.d.copy_from_slice(&ck);
    let mut rk: VexKeys256 = [0; 60];
    // Safety: safe because our target has the "zkn" RV32 extensions.
    unsafe { aes_key_schedule_256(&mut rk, &ck_a.d) };
    rk
}

#[target_feature(enable = "zkn")]
unsafe fn aes_key_schedule_256(rk: &mut VexKeys256, ck: &[u8]) {
    #[rustfmt::skip]
    unsafe {
        // a0 - uint32_t rk [AES_256_RK_WORDS]
        // a1 - uint8_t  ck [AES_256_CK_BYTE ]
        core::arch::asm!(
            "lw  a2,  0(a1)",
            "lw  a3,  4(a1)",
            "lw  a4,  8(a1)",
            "lw  a5, 12(a1)",
            "lw  a7, 16(a1)",
            "lw  t5, 20(a1)",
            "lw  t6, 24(a1)",
            "lw  t2, 28(a1)",

            "mv      a6, a0",
            "addi    t0, a0, 56*4",       //
            "la      t1, 50f",// t1 = round constant pointer

            "sw      a2,  0(a6)",         // rkp[0]
            "sw      a3,  4(a6)",         // rkp[1]
            "sw      a4,  8(a6)",         // rkp[2]
            "sw      a5, 12(a6)",         // rkp[3]

        "30:",            // Loop start

            "sw      a7, 16(a6)",         // rkp[4]
            "sw      t5, 20(a6)",         // rkp[5]
            "sw      t6, 24(a6)",         // rkp[6]
            "sw      t2, 28(a6)",         // rkp[7]

            "addi    a6, a6, 32",        // increment rkp


            "lbu     t4, 0(t1)",         // Load round constant byte
            "addi    t1, t1, 1",         // Increment round constant byte
            "xor     a2, a2, t4",         // c0 ^= rcp

            // "ROR32I t3, t4, t2, 8",        // tr = ROR32(c3, 8)
            "srli t4, t2, 8",
            "slli t3, t2, 32-8",
            "or   t3, t3, t4",

            "aes32esi a2, a2, t3, 0",   // tr = sbox(tr)
            "aes32esi a2, a2, t3, 1",   //
            "aes32esi a2, a2, t3, 2",   //
            "aes32esi a2, a2, t3, 3",   //

            "xor     a3, a3, a2",          // a3 ^= a2
            "xor     a4, a4, a3",          // a4 ^= a3
            "xor     a5, a5, a4",          // a5 ^= a4

            "sw      a2,  0(a6)",         // rkp[0]
            "sw      a3,  4(a6)",         // rkp[1]
            "sw      a4,  8(a6)",         // rkp[2]
            "sw      a5, 12(a6)",         // rkp[3]

            "beq     t0, a6, 40f",

            "aes32esi a7, a7, a5, 0",   // tr = sbox(tr)
            "aes32esi a7, a7, a5, 1",   //
            "aes32esi a7, a7, a5, 2",   //
            "aes32esi a7, a7, a5, 3",   //

            "xor     t5, t5, a7",          // t5 ^= a7
            "xor     t6, t6, t5",          // t6 ^= t5
            "xor     t2, t2, t6",          // t2 ^= t6

            "j 30b",                   // Loop continue

        "50:",
            ".byte 0x01, 0x02, 0x04, 0x08, 0x10",
            ".byte 0x20, 0x40, 0x80, 0x1b, 0x36",

        "40:",
            "nop",  // was ret

            in("a0") rk.as_mut_ptr(),
            in("a1") ck.as_ptr(),
        );
    };
}

pub fn aes_vexriscv_decrypt_asm_wrapper(key: &VexKeys256, block: &[u8], rounds: u32) -> [u8; 16] {
    let mut ct = AlignedBlock { data: [0u8; 16] };
    ct.data.copy_from_slice(block);
    let mut pt = AlignedBlock { data: [0u8; 16] };

    // safe because our target architecture supports "zkn"
    unsafe { aes256_vexriscv_decrypt_asm(key, &ct, &mut pt, rounds * 16) };
    pt.data
}

#[repr(C, align(16))]
pub struct AlignedBlock {
    pub data: [u8; 16],
}
#[target_feature(enable = "zkn")]
pub unsafe fn aes256_vexriscv_decrypt_asm(
    key: &VexKeys256,
    ct: &AlignedBlock,
    pt: &mut AlignedBlock,
    rounds: u32,
) {
    #[rustfmt::skip]
    unsafe {
        // a0 - uint8_t     pt [16],
        // a1 - uint8_t     ct [16],
        // a2 - uint32_t  * rk,
        core::arch::asm!(
            "add     a3, a2, a3",                       // kp = rk + 4*nr

            "lw      a4, 0(a1)",
            "lw      a5, 4(a1)",
            "lw      a6, 8(a1)",
            "lw      a7, 12(a1)",

            "lw      t0,  0(a3)",                          // Load Round Key
            "lw      t1,  4(a3)",
            "lw      t2,  8(a3)",
            "lw      t3, 12(a3)",

            "xor     a4, a4, t0",                          // Add Round Key
            "xor     a5, a5, t1",
            "xor     a6, a6, t2",
            "xor     a7, a7, t3",

            "addi    a3, a3, -32",                         // Loop counter

        "20:", // .aes_dec_block_l0:

            "lw      t0, 16(a3)",                      // Load Round Key
            "lw      t1, 20(a3)",
            "lw      t2, 24(a3)",
            "lw      t3, 28(a3)",

            "aes32dsmi  t0, t0, a4, 0",                    // Even Round
            "aes32dsmi  t1, t1, a5, 0",
            "aes32dsmi  t2, t2, a6, 0",
            "aes32dsmi  t3, t3, a7, 0",

            "aes32dsmi  t0, t0, a7, 1",
            "aes32dsmi  t1, t1, a4, 1",
            "aes32dsmi  t2, t2, a5, 1",
            "aes32dsmi  t3, t3, a6, 1",

            "aes32dsmi  t0, t0, a6, 2",
            "aes32dsmi  t1, t1, a7, 2",
            "aes32dsmi  t2, t2, a4, 2",
            "aes32dsmi  t3, t3, a5, 2",

            "aes32dsmi  t0, t0, a5, 3",
            "aes32dsmi  t1, t1, a6, 3",
            "aes32dsmi  t2, t2, a7, 3",
            "aes32dsmi  t3, t3, a4, 3",                    // U* contains new state

            "lw      a4,  0(a3)",                      // Load Round Key
            "lw      a5,  4(a3)",
            "lw      a6,  8(a3)",
            "lw      a7, 12(a3)",

            "beq     a2, a3, 30f", // aes_dec_block_l_finish Break from loop
            "addi    a3, a3, -32",                     // Step Key pointer

            "aes32dsmi  a4, a4, t0, 0",                    // Odd Round
            "aes32dsmi  a5, a5, t1, 0",
            "aes32dsmi  a6, a6, t2, 0",
            "aes32dsmi  a7, a7, t3, 0",

            "aes32dsmi  a4, a4, t3, 1",
            "aes32dsmi  a5, a5, t0, 1",
            "aes32dsmi  a6, a6, t1, 1",
            "aes32dsmi  a7, a7, t2, 1",

            "aes32dsmi  a4, a4, t2, 2",
            "aes32dsmi  a5, a5, t3, 2",
            "aes32dsmi  a6, a6, t0, 2",
            "aes32dsmi  a7, a7, t1, 2",

            "aes32dsmi  a4, a4, t1, 3",
            "aes32dsmi  a5, a5, t2, 3",
            "aes32dsmi  a6, a6, t3, 3",
            "aes32dsmi  a7, a7, t0, 3",                    // T* contains new state

            "j 20b", // .aes_dec_block_l0                         // repeat loop

        "30:", //.aes_dec_block_l_finish:

            "aes32dsi    a4, a4, t0, 0",                       // Final round, no MixColumns
            "aes32dsi    a5, a5, t1, 0",
            "aes32dsi    a6, a6, t2, 0",
            "aes32dsi    a7, a7, t3, 0",

            "aes32dsi    a4, a4, t3, 1",
            "aes32dsi    a5, a5, t0, 1",
            "aes32dsi    a6, a6, t1, 1",
            "aes32dsi    a7, a7, t2, 1",

            "aes32dsi    a4, a4, t2, 2",
            "aes32dsi    a5, a5, t3, 2",
            "aes32dsi    a6, a6, t0, 2",
            "aes32dsi    a7, a7, t1, 2",

            "aes32dsi    a4, a4, t1, 3",
            "aes32dsi    a5, a5, t2, 3",
            "aes32dsi    a6, a6, t3, 3",
            "aes32dsi    a7, a7, t0, 3",                       // T* contains new state

            "sw  a4, 0(a0)",
            "sw  a5, 4(a0)",
            "sw  a6, 8(a0)",
            "sw  a7, 12(a0)",

            in("a0") pt.data.as_mut_ptr(),
            in("a1") ct.data.as_ptr(),
            in("a2") key.as_ptr(),
            in("a3") rounds,
        );
    };
}

/// AES-256 round keys
pub(crate) type VexKeys256 = [u32; 60];

fn aes256_dec_key_schedule_asm_wrapper(user_key: &[u8]) -> VexKeys256 {
    let mut rk: VexKeys256 = [0; 60];
    let mut uk_a = AlignedCk::default();
    uk_a.d.copy_from_slice(&user_key);

    unsafe {
        aes_key_schedule_256(&mut rk, &uk_a.d);
    }
    unsafe { aes256_dec_key_schedule_asm(&mut rk, &uk_a.d) };
    rk
}

#[target_feature(enable = "zkn")]
unsafe fn aes256_dec_key_schedule_asm(rk: &mut VexKeys256, user_key: &[u8]) {
    #[rustfmt::skip]
    unsafe {
        core::arch::asm!(
            // a0 - uint32_t rk [AES_256_RK_WORDS]
            // a1 - uint8_t  ck [AES_256_CK_BYTE ]

            "addi    a2, a0, 16",              // a0 = &a0[ 4]
            "addi    a3, a0, 56*4",            // a1 = &a0[40]

        "20:",

            "lw   t0, 0(a2)",              // Load key word

            "li        t1, 0",
            "aes32esi  t1, t1, t0, 0",     // Sub Word Forward
            "aes32esi  t1, t1, t0, 1 ",
            "aes32esi  t1, t1, t0, 2",
            "aes32esi  t1, t1, t0, 3",

            "li        t0, 0",
            "aes32dsmi t0, t0, t1, 0",     // Sub Word Inverse & Inverse MixColumns
            "aes32dsmi t0, t0, t1, 1",
            "aes32dsmi t0, t0, t1, 2",
            "aes32dsmi t0, t0, t1, 3",

            "sw   t0, 0(a2)",             // Store key word.

            "addi a2, a2, 4",            // Increment round key pointer
            "bne  a2, a3, 20b", // Finished yet?

            in("a0") rk.as_mut_ptr(),
            in("a1") user_key.as_ptr(),
        );
    };
}

pub fn aes_vexriscv_encrypt_asm_wrapper(key: &VexKeys256, block: &[u8], rounds: u32) -> [u8; 16] {
    let mut pt = AlignedBlock { data: [0u8; 16] };
    pt.data.copy_from_slice(block);
    let mut ct = AlignedBlock { data: [0u8; 16] };

    // safe because our target architecture supports "zkn"
    unsafe { aes256_vexriscv_encrypt_asm(key, &pt, &mut ct, rounds * 16) };
    ct.data
}

#[target_feature(enable = "zkn")]
pub unsafe fn aes256_vexriscv_encrypt_asm(
    key: &VexKeys256,
    pt: &AlignedBlock,
    ct: &mut AlignedBlock,
    rounds: u32,
) {
    #[rustfmt::skip]
    unsafe {
        // a0 - uint8_t     ct [16],
        // a1 - uint8_t     pt [16],
        // a2 - uint32_t  * rk,
        core::arch::asm!(
            "add     a3, a2, a3",                       // kp = rk + 4*nr (rk = a2, kp = a3)

            "lw      a4, 0(a1)",
            "lw      a5, 4(a1)",
            "lw      a6, 8(a1)",
            "lw      a7, 12(a1)",

            "lw      t0,  0(a2)",                          // Load Round Key
            "lw      t1,  4(a2)",
            "lw      t2,  8(a2)",
            "lw      t3, 12(a2)",

            "xor     a4, a4, t0",                          // Add Round Key
            "xor     a5, a5, t1",
            "xor     a6, a6, t2",
            "xor     a7, a7, t3",

        "20:", // .aes_enc_block_l0:

            "lw      t0, 16(a2)",                      // Load Round Key
            "lw      t1, 20(a2)",
            "lw      t2, 24(a2)",
            "lw      t3, 28(a2)",

            "aes32esmi  t0, t0, a4, 0",  // s0           // Even Round
            "aes32esmi  t1, t1, a5, 0",  // s1
            "aes32esmi  t2, t2, a6, 0",  // s2
            "aes32esmi  t3, t3, a7, 0",  // s3

            "aes32esmi  t0, t0, a5, 1",
            "aes32esmi  t1, t1, a6, 1",
            "aes32esmi  t2, t2, a7, 1",
            "aes32esmi  t3, t3, a4, 1",

            "aes32esmi  t0, t0, a6, 2",
            "aes32esmi  t1, t1, a7, 2",
            "aes32esmi  t2, t2, a4, 2",
            "aes32esmi  t3, t3, a5, 2",

            "aes32esmi  t0, t0, a7, 3",
            "aes32esmi  t1, t1, a4, 3",
            "aes32esmi  t2, t2, a5, 3",
            "aes32esmi  t3, t3, a6, 3",                    // U* contains new state

            "lw      a4,  32(a2)",                      // Load Round Key
            "lw      a5,  36(a2)",
            "lw      a6,  40(a2)",
            "lw      a7,  44(a2)",

            "addi    a2, a2, 32",                     // Step Key pointer
            "beq     a2, a3, 30f", // aes_enc_block_l_finish Break from loop

            "aes32esmi  a4, a4, t0, 0",                    // Odd Round
            "aes32esmi  a5, a5, t1, 0",
            "aes32esmi  a6, a6, t2, 0",
            "aes32esmi  a7, a7, t3, 0",

            "aes32esmi  a4, a4, t1, 1",
            "aes32esmi  a5, a5, t2, 1",
            "aes32esmi  a6, a6, t3, 1",
            "aes32esmi  a7, a7, t0, 1",

            "aes32esmi  a4, a4, t2, 2",
            "aes32esmi  a5, a5, t3, 2",
            "aes32esmi  a6, a6, t0, 2",
            "aes32esmi  a7, a7, t1, 2",

            "aes32esmi  a4, a4, t3, 3",
            "aes32esmi  a5, a5, t0, 3",
            "aes32esmi  a6, a6, t1, 3",
            "aes32esmi  a7, a7, t2, 3",                    // T* contains new state

            "j 20b", // .aes_enc_block_l0                         // repeat loop

        "30:", //.aes_enc_block_l_finish:

            "aes32esi    a4, a4, t0, 0",                       // Final round, no MixColumns
            "aes32esi    a5, a5, t1, 0",
            "aes32esi    a6, a6, t2, 0",
            "aes32esi    a7, a7, t3, 0",

            "aes32esi    a4, a4, t1, 1",
            "aes32esi    a5, a5, t2, 1",
            "aes32esi    a6, a6, t3, 1",
            "aes32esi    a7, a7, t0, 1",

            "aes32esi    a4, a4, t2, 2",
            "aes32esi    a5, a5, t3, 2",
            "aes32esi    a6, a6, t0, 2",
            "aes32esi    a7, a7, t1, 2",

            "aes32esi    a4, a4, t3, 3",
            "aes32esi    a5, a5, t0, 3",
            "aes32esi    a6, a6, t1, 3",
            "aes32esi    a7, a7, t2, 3",                       // T* contains new state

            "sw  a4, 0(a0)",
            "sw  a5, 4(a0)",
            "sw  a6, 8(a0)",
            "sw  a7, 12(a0)",

            in("a0") ct.data.as_mut_ptr(),
            in("a1") pt.data.as_ptr(),
            in("a2") key.as_ptr(),
            in("a3") rounds,
        );
    };
}

macro_rules! define_aes_impl {
    (
        $name:tt,
        $name_enc:ident,
        $name_dec:ident,
        $name_back_enc:ident,
        $name_back_dec:ident,
        $key_size:ty,
        $key_bits:expr,
        $vex_keys:ty,
        $rounds:expr,
        $vex_dec_key_schedule:path,
        $vex_enc_key_schedule:path,
        $vex_decrypt:path,
        $vex_encrypt:path,
        $doc:expr $(,)?
    ) => {
        #[doc=$doc]
        ///block cipher
        #[derive(Clone)]
        pub struct $name {
            enc_key: $vex_keys,
            dec_key: $vex_keys,
        }

        #[allow(dead_code)]
        impl $name {
            pub fn key_size(&self) -> usize { $key_bits as usize }

            pub fn clear(&mut self) {
                let nuke = self.enc_key.as_mut_ptr();
                for i in 0..self.enc_key.len() {
                    unsafe { nuke.add(i).write_volatile(0) };
                }
                let nuke = self.dec_key.as_mut_ptr();
                for i in 0..self.dec_key.len() {
                    unsafe { nuke.add(i).write_volatile(0) };
                }
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            }

            #[inline(always)]
            pub(crate) fn get_enc_backend(&self) -> $name_back_enc<'_> { $name_back_enc(self) }

            #[inline(always)]
            pub(crate) fn get_dec_backend(&self) -> $name_back_dec<'_> { $name_back_dec(self) }
        }

        impl KeySizeUser for $name {
            type KeySize = $key_size;
        }

        impl KeyInit for $name {
            #[inline]
            fn new(key: &Key<Self>) -> Self {
                Self { enc_key: $vex_enc_key_schedule(key), dec_key: $vex_dec_key_schedule(key) }
            }
        }

        impl BlockSizeUser for $name {
            type BlockSize = U16;
        }

        impl BlockCipher for $name {}

        impl BlockEncrypt for $name {
            fn encrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
                f.call(&mut self.get_enc_backend())
            }
        }

        #[allow(dead_code)]
        impl BlockDecrypt for $name {
            fn decrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
                f.call(&mut self.get_dec_backend())
            }
        }

        impl ParBlocksSizeUser for $name {
            type ParBlocksSize = U1;
        }

        impl From<$name_enc> for $name {
            #[inline]
            fn from(enc: $name_enc) -> $name { enc.inner }
        }

        impl From<&$name_enc> for $name {
            #[inline]
            fn from(enc: &$name_enc) -> $name { enc.inner.clone() }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                f.write_str(concat!(stringify!($name), " { .. }"))
            }
        }

        impl AlgorithmName for $name {
            fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(stringify!($name)) }
        }

        #[doc=$doc]
        ///block cipher (encrypt-only)
        #[derive(Clone)]
        pub struct $name_enc {
            inner: $name,
        }

        impl $name_enc {
            #[inline(always)]
            pub(crate) fn get_enc_backend(&self) -> $name_back_enc<'_> { self.inner.get_enc_backend() }
        }

        impl BlockCipher for $name_enc {}

        impl KeySizeUser for $name_enc {
            type KeySize = $key_size;
        }

        impl KeyInit for $name_enc {
            #[inline(always)]
            fn new(key: &Key<Self>) -> Self {
                let inner = $name::new(key);
                Self { inner }
            }
        }

        impl BlockSizeUser for $name_enc {
            type BlockSize = U16;
        }

        impl BlockEncrypt for $name_enc {
            fn encrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
                f.call(&mut self.get_enc_backend())
            }
        }

        impl fmt::Debug for $name_enc {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                f.write_str(concat!(stringify!($name_enc), " { .. }"))
            }
        }

        impl AlgorithmName for $name_enc {
            fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name_enc))
            }
        }

        #[doc=$doc]
        ///block cipher (decrypt-only)
        #[derive(Clone)]
        pub struct $name_dec {
            inner: $name,
        }

        #[allow(dead_code)]
        impl $name_dec {
            #[inline(always)]
            pub(crate) fn get_dec_backend(&self) -> $name_back_dec<'_> { self.inner.get_dec_backend() }
        }

        impl BlockCipher for $name_dec {}

        impl KeySizeUser for $name_dec {
            type KeySize = $key_size;
        }

        impl KeyInit for $name_dec {
            #[inline(always)]
            fn new(key: &Key<Self>) -> Self {
                let inner = $name::new(key);
                Self { inner }
            }
        }

        impl From<$name_enc> for $name_dec {
            #[inline]
            fn from(enc: $name_enc) -> $name_dec { Self { inner: enc.inner } }
        }

        impl From<&$name_enc> for $name_dec {
            #[inline]
            fn from(enc: &$name_enc) -> $name_dec { Self { inner: enc.inner.clone() } }
        }

        impl BlockSizeUser for $name_dec {
            type BlockSize = U16;
        }

        impl BlockDecrypt for $name_dec {
            fn decrypt_with_backend(&self, f: impl BlockClosure<BlockSize = U16>) {
                f.call(&mut self.get_dec_backend());
            }
        }

        impl fmt::Debug for $name_dec {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                f.write_str(concat!(stringify!($name_dec), " { .. }"))
            }
        }

        impl AlgorithmName for $name_dec {
            fn write_alg_name(f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name_dec))
            }
        }

        pub(crate) struct $name_back_enc<'a>(&'a $name);

        impl<'a> BlockSizeUser for $name_back_enc<'a> {
            type BlockSize = U16;
        }

        impl<'a> ParBlocksSizeUser for $name_back_enc<'a> {
            type ParBlocksSize = U1;
        }

        impl<'a> BlockBackend for $name_back_enc<'a> {
            #[inline(always)]
            fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
                let res = $vex_encrypt(&self.0.enc_key, block.clone_in().as_slice(), $rounds);
                *block.get_out() = *Block::from_slice(&res);
            }

            #[inline(always)]
            fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
                let res = $vex_encrypt(&self.0.enc_key, &blocks.get_in()[0], $rounds);
                *blocks.get_out() = *BatchBlocks::from_slice(&[*Block::from_slice(&res)]);
            }
        }

        pub(crate) struct $name_back_dec<'a>(&'a $name);

        impl<'a> BlockSizeUser for $name_back_dec<'a> {
            type BlockSize = U16;
        }

        impl<'a> ParBlocksSizeUser for $name_back_dec<'a> {
            type ParBlocksSize = U1;
        }

        impl<'a> BlockBackend for $name_back_dec<'a> {
            #[inline(always)]
            fn proc_block(&mut self, mut block: InOut<'_, '_, Block>) {
                let res = $vex_decrypt(&self.0.dec_key, block.clone_in().as_slice(), $rounds);
                *block.get_out() = *Block::from_slice(&res);
            }

            #[inline(always)]
            fn proc_par_blocks(&mut self, mut blocks: InOut<'_, '_, BatchBlocks>) {
                let res = $vex_decrypt(&self.0.dec_key, &blocks.get_in()[0], $rounds);
                *blocks.get_out() = *BatchBlocks::from_slice(&[*Block::from_slice(&res)]);
            }
        }
    };
}

define_aes_impl!(
    Aes128,
    Aes128Enc,
    Aes128Dec,
    Aes128BackEnc,
    Aes128BackDec,
    U16,
    128,
    VexKeys128,
    10,
    aes128_dec_key_schedule_asm_wrapper,
    aes_key_schedule_128_wrapper,
    aes_vexriscv_decrypt_asm_wrapper,
    aes_vexriscv_encrypt_asm_wrapper,
    "AES-128 block cipher instance"
);

define_aes_impl!(
    Aes256,
    Aes256Enc,
    Aes256Dec,
    Aes256BackEnc,
    Aes256BackDec,
    U32,
    256,
    VexKeys256,
    14,
    aes256_dec_key_schedule_asm_wrapper,
    aes_key_schedule_256_wrapper,
    aes_vexriscv_decrypt_asm_wrapper,
    aes_vexriscv_encrypt_asm_wrapper,
    "AES-256 block cipher instance"
);
