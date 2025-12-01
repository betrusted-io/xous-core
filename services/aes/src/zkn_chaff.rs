// See https://github.com/riscv/riscv-crypto/tree/main/benchmarks/aes/zscrypto_rv32
// for reference assembly code from which the routines in here are derived.
// Note that the order of the AES ops is shuffled across blocks versus ref code
// to break data dependencies and remove pipeline bubbles.

use core::fmt;

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

use core::cell::RefCell;

use rand_chacha::rand_core::{RngCore, SeedableRng};
struct LazyRng {
    initialized: core::sync::atomic::AtomicBool,
    rng: core::cell::UnsafeCell<Option<rand_chacha::ChaCha8Rng>>,
}
unsafe impl Sync for LazyRng {}
static RNG: LazyRng = LazyRng {
    initialized: core::sync::atomic::AtomicBool::new(false),
    rng: core::cell::UnsafeCell::new(None),
};

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
pub(crate) type VexKeys128 = ([u32; 60], [u32; 60], RefCell<u64>);

fn aes_key_schedule_128_wrapper(ck: &[u8]) -> VexKeys128 {
    let mut ck_a = AlignedCk128::default();
    ck_a.d.copy_from_slice(&ck);
    let mut rk: VexKeys128 = ([0; 60], [0; 60], RefCell::new(0));
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

            in("a0") rk.0.as_mut_ptr(),
            in("a1") ck.as_ptr(),
        );
    };
}

fn aes128_dec_key_schedule_asm_wrapper(user_key: &[u8]) -> VexKeys128 {
    let mut rk: VexKeys128 = ([0; 60], [0; 60], RefCell::new(0));
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

            in("a0") rk.0.as_mut_ptr(),
            in("a1") user_key.as_ptr(),
        );
    };
}

#[repr(align(32))]
#[derive(Default)]
struct AlignedCk {
    d: [u8; 32],
}

/// This sets up the ChaCha8 CSPRNG for the current process if it's not already setup.
fn rand_prelude() {
    use core::sync::atomic::Ordering;

    if !RNG.initialized.load(Ordering::Acquire) {
        let mut seed = [0u8; 32];

        #[cfg(target_os = "xous")]
        let (_, value) =
            std::env::vars().find(|(key, _value)| key == "SEED").expect("Fatal: No SEED in env!");
        #[cfg(target_os = "xous")]
        seed.copy_from_slice(&hex::decode(value).expect("Couldn't decode SEED"));

        // NOTE: this assumes that the TRNG was set up *elsewhere* in the system already by
        // the bootloader. This is for using AES in the loader to e.g. initialize swap.
        #[cfg(not(target_os = "xous"))]
        let mut trng =
            bao1x_hal::sce::trng::Trng::new(utralib::utra::trng::HW_TRNG_BASE /* 0x4002e000 */);
        #[cfg(not(target_os = "xous"))]
        for s in seed.chunks_mut(4) {
            s.copy_from_slice(&trng.get_raw().to_ne_bytes());
        }
        unsafe {
            *RNG.rng.get() = Some(rand_chacha::ChaCha8Rng::from_seed(seed));
        }
        RNG.initialized.store(true, Ordering::Release);
    }
}

fn aes_key_schedule_256_wrapper(ck: &[u8]) -> VexKeys256 {
    let mut ck_a = AlignedCk::default();
    ck_a.d.copy_from_slice(&ck);
    let mut rk: VexKeys256 = ([0; 60], [0; 60], RefCell::new(0));

    // safety: only safe if the TRNG was previously initialized by the bootloader
    // and also assumes no interrupts/concurrency is possible
    unsafe {
        rand_prelude();
        let rng = (*RNG.rng.get()).as_mut().unwrap();
        for elem in &mut rk.1 {
            *elem = rng.next_u32();
        }
        rk.2 = RefCell::new(rng.next_u64());
    }

    // Safety: safe because our target has the "zkn" RV32 extensions.
    unsafe { aes_key_schedule_256(&mut rk.0, &ck_a.d) };
    rk
}

#[target_feature(enable = "zkn")]
unsafe fn aes_key_schedule_256(rk: &mut [u32; 60], ck: &[u8]) {
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

    let order_vector = if *key.2.borrow() == 0 {
        let mut vector_storage = unsafe {
            rand_prelude();
            (*RNG.rng.get()).as_mut().unwrap().next_u64()
        };
        let order_vector: u16 = (vector_storage & 0xFFFF) as u16;
        vector_storage >>= 16;
        *key.2.borrow_mut() = vector_storage;
        order_vector
    } else {
        let vector = *key.2.borrow();
        *key.2.borrow_mut() >>= 16;
        (vector & 0xFFFF) as u16
    };

    // safe because our target architecture supports "zkn"
    unsafe { aes256_vexriscv_decrypt_asm(&key.0, &ct, &mut pt, rounds * 16, &key.1, order_vector) };
    pt.data
}

#[repr(C, align(16))]
pub struct AlignedBlock {
    pub data: [u8; 16],
}
#[target_feature(enable = "zkn")]
pub unsafe fn aes256_vexriscv_decrypt_asm(
    key: &[u32; 60],
    ct: &AlignedBlock,
    pt: &mut AlignedBlock,
    rounds: u32,
    chaff: &[u32; 60],
    order_vector: u16,
) {
    #[rustfmt::skip]
    #[rustfmt::skip]
    unsafe {
        core::arch::asm!(
            // ============================================================
            // Prologue: Save callee-saved registers
            // ============================================================
            "addi    sp, sp, -48",
            "sw      s0,  0(sp)",
            "sw      s1,  4(sp)",
            "sw      s2,  8(sp)",
            "sw      s3, 12(sp)",
            "sw      s4, 16(sp)",
            "sw      s5, 20(sp)",
            "sw      s6, 24(sp)",
            "sw      s7, 28(sp)",
            "sw      s8, 32(sp)",
            "sw      s9, 36(sp)",
            "sw      s10, 40(sp)",
            "sw      s11, 44(sp)",

            // ============================================================
            // Setup
            // Inputs: a2=key, a3=rounds (bytes), a4=chaff, a5=order_vector
            // ============================================================
            "mv      s6, a5",               // s6 = order_vector
            "mv      s7, a2",               // s7 = real key base (loop termination)
            "add     s4, a2, a3",           // s4 = real key end pointer
            "add     s5, a4, a3",           // s5 = chaff key end pointer

            // ============================================================
            // Load ciphertext into both states
            // ============================================================
            "lw      a4, 0(a1)",
            "lw      a5, 4(a1)",
            "lw      a6, 8(a1)",
            "lw      a7, 12(a1)",
            "lw      s0, 0(a1)",
            "lw      s1, 4(a1)",
            "lw      s2, 8(a1)",
            "lw      s3, 12(a1)",

            // ============================================================
            // Initial AddRoundKey (last round key for decryption)
            // ============================================================
            "lw      t0,  0(s4)",  // real
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",
            "xor     a4, a4, t0",
            "xor     a5, a5, t1",
            "xor     a6, a6, t2",
            "xor     a7, a7, t3",

            "lw      t0,  0(s5)",  // chaff
            "lw      t1,  4(s5)",
            "lw      t2,  8(s5)",
            "lw      t3, 12(s5)",
            "xor     s0, s0, t0",
            "xor     s1, s1, t1",
            "xor     s2, s2, t2",
            "xor     s3, s3, t3",

            "addi    s4, s4, -16",
            "addi    s5, s5, -16",

            // ============================================================
            // Main loop
            // ============================================================
        "10:",
            "beq     s4, s7, 30f",  // end condition
            "andi    t4, s6, 1",    // check LSB of order vector
            "srli    s6, s6, 1",    // shift order vector right
            "bnez    t4, 20f",

            "nop",   // compensate for branch not taken
            "nop",   // constant time confirmed in simulation
            "nop",

            // --- Path A: Real first ---
            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32dsmi  t0, t0, a4, 0",
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
            "aes32dsmi  t3, t3, a4, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32dsmi  s8,  s8,  s0, 0",
            "aes32dsmi  s9,  s9,  s1, 0",
            "aes32dsmi  s10, s10, s2, 0",
            "aes32dsmi  s11, s11, s3, 0",
            "aes32dsmi  s8,  s8,  s3, 1",
            "aes32dsmi  s9,  s9,  s0, 1",
            "aes32dsmi  s10, s10, s1, 1",
            "aes32dsmi  s11, s11, s2, 1",
            "aes32dsmi  s8,  s8,  s2, 2",
            "aes32dsmi  s9,  s9,  s3, 2",
            "aes32dsmi  s10, s10, s0, 2",
            "aes32dsmi  s11, s11, s1, 2",
            "aes32dsmi  s8,  s8,  s1, 3",
            "aes32dsmi  s9,  s9,  s2, 3",
            "aes32dsmi  s10, s10, s3, 3",
            "aes32dsmi  s11, s11, s0, 3",

            "mv       s0, s8",
            "mv       s1, s9",
            "mv       s2, s10",
            "mv       s3, s11",

            "j        25f",

        "20:",  // --- Path B: Chaff first ---
            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32dsmi  s8,  s8,  s0, 0",
            "aes32dsmi  s9,  s9,  s1, 0",
            "aes32dsmi  s10, s10, s2, 0",
            "aes32dsmi  s11, s11, s3, 0",
            "aes32dsmi  s8,  s8,  s3, 1",
            "aes32dsmi  s9,  s9,  s0, 1",
            "aes32dsmi  s10, s10, s1, 1",
            "aes32dsmi  s11, s11, s2, 1",
            "aes32dsmi  s8,  s8,  s2, 2",
            "aes32dsmi  s9,  s9,  s3, 2",
            "aes32dsmi  s10, s10, s0, 2",
            "aes32dsmi  s11, s11, s1, 2",
            "aes32dsmi  s8,  s8,  s1, 3",
            "aes32dsmi  s9,  s9,  s2, 3",
            "aes32dsmi  s10, s10, s3, 3",
            "aes32dsmi  s11, s11, s0, 3",

            "mv       s0, s8",
            "mv       s1, s9",
            "mv       s2, s10",
            "mv       s3, s11",

            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32dsmi  t0, t0, a4, 0",
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
            "aes32dsmi  t3, t3, a4, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            "j        25f",   // dummy jump to even out paths

        "25:",  // Loop tail
            "addi    s4, s4, -16",
            "addi    s5, s5, -16",
            "j       10b",

            // ============================================================
            // Final round (no InvMixColumns)
            // ============================================================
        "30:",
            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32dsi   t0, t0, a4, 0",
            "aes32dsi   t1, t1, a5, 0",
            "aes32dsi   t2, t2, a6, 0",
            "aes32dsi   t3, t3, a7, 0",
            "aes32dsi   t0, t0, a7, 1",
            "aes32dsi   t1, t1, a4, 1",
            "aes32dsi   t2, t2, a5, 1",
            "aes32dsi   t3, t3, a6, 1",
            "aes32dsi   t0, t0, a6, 2",
            "aes32dsi   t1, t1, a7, 2",
            "aes32dsi   t2, t2, a4, 2",
            "aes32dsi   t3, t3, a5, 2",
            "aes32dsi   t0, t0, a5, 3",
            "aes32dsi   t1, t1, a6, 3",
            "aes32dsi   t2, t2, a7, 3",
            "aes32dsi   t3, t3, a4, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            // Chaff final round (computed but discarded)
            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32dsi   s8,  s8,  s0, 0",
            "aes32dsi   s9,  s9,  s1, 0",
            "aes32dsi   s10, s10, s2, 0",
            "aes32dsi   s11, s11, s3, 0",
            "aes32dsi   s8,  s8,  s3, 1",
            "aes32dsi   s9,  s9,  s0, 1",
            "aes32dsi   s10, s10, s1, 1",
            "aes32dsi   s11, s11, s2, 1",
            "aes32dsi   s8,  s8,  s2, 2",
            "aes32dsi   s9,  s9,  s3, 2",
            "aes32dsi   s10, s10, s0, 2",
            "aes32dsi   s11, s11, s1, 2",
            "aes32dsi   s8,  s8,  s1, 3",
            "aes32dsi   s9,  s9,  s2, 3",
            "aes32dsi   s10, s10, s3, 3",
            "aes32dsi   s11, s11, s0, 3",

            // ============================================================
            // Store result
            // ============================================================
            "sw      a4, 0(a0)",
            "sw      a5, 4(a0)",
            "sw      a6, 8(a0)",
            "sw      a7, 12(a0)",

            // ============================================================
            // Epilogue
            // ============================================================
            "lw      s0,  0(sp)",
            "lw      s1,  4(sp)",
            "lw      s2,  8(sp)",
            "lw      s3, 12(sp)",
            "lw      s4, 16(sp)",
            "lw      s5, 20(sp)",
            "lw      s6, 24(sp)",
            "lw      s7, 28(sp)",
            "lw      s8, 32(sp)",
            "lw      s9, 36(sp)",
            "lw      s10, 40(sp)",
            "lw      s11, 44(sp)",
            "addi    sp, sp, 48",

            in("a0") pt.data.as_mut_ptr(),
            in("a1") ct.data.as_ptr(),
            in("a2") key.as_ptr(),
            in("a3") rounds,
            in("a4") chaff.as_ptr(),
            in("a5") order_vector,
            options(nostack)
        );
    };
}

/// AES-256 round keys
pub(crate) type VexKeys256 = ([u32; 60], [u32; 60], RefCell<u64>);

fn aes256_dec_key_schedule_asm_wrapper(user_key: &[u8]) -> VexKeys256 {
    let mut rk: VexKeys256 = ([0; 60], [0; 60], RefCell::new(0));
    let mut uk_a = AlignedCk::default();
    uk_a.d.copy_from_slice(&user_key);

    // safety: only safe if the TRNG was previously initialized by the bootloader
    // and also assumes no interrupts/concurrency is possible
    unsafe {
        rand_prelude();
        let rng = (*RNG.rng.get()).as_mut().unwrap();
        for elem in &mut rk.1 {
            *elem = rng.next_u32();
        }
        rk.2 = RefCell::new(rng.next_u64());
    }

    unsafe {
        aes_key_schedule_256(&mut rk.0, &uk_a.d);
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

            in("a0") rk.0.as_mut_ptr(),
            in("a1") user_key.as_ptr(),
        );
    };
}

pub fn aes_vexriscv_encrypt_asm_wrapper(key: &VexKeys256, block: &[u8], rounds: u32) -> [u8; 16] {
    let mut pt = AlignedBlock { data: [0u8; 16] };
    pt.data.copy_from_slice(block);
    let mut ct = AlignedBlock { data: [0u8; 16] };

    let order_vector = if *key.2.borrow() == 0 {
        let mut vector_storage = unsafe {
            rand_prelude();
            (*RNG.rng.get()).as_mut().unwrap().next_u64()
        };
        let order_vector: u16 = (vector_storage & 0xFFFF) as u16;
        vector_storage >>= 16;
        *key.2.borrow_mut() = vector_storage;
        order_vector
    } else {
        let vector = *key.2.borrow();
        *key.2.borrow_mut() >>= 16;
        (vector & 0xFFFF) as u16
    };

    // safe because our target architecture supports "zkn"
    unsafe { aes256_vexriscv_encrypt_asm(&key.0, &pt, &mut ct, rounds * 16, &key.1, order_vector) };
    ct.data
}

#[target_feature(enable = "zkn")]
pub unsafe fn aes256_vexriscv_encrypt_asm(
    key: &[u32; 60],
    pt: &AlignedBlock,
    ct: &mut AlignedBlock,
    rounds: u32,
    chaff: &[u32; 60],
    order_vector: u16,
) {
    #[rustfmt::skip]
    unsafe {
        core::arch::asm!(
            // ============================================================
            // Prologue: Save callee-saved registers
            // ============================================================
            "addi    sp, sp, -48",
            "sw      s0,  0(sp)",
            "sw      s1,  4(sp)",
            "sw      s2,  8(sp)",
            "sw      s3, 12(sp)",
            "sw      s4, 16(sp)",
            "sw      s5, 20(sp)",
            "sw      s6, 24(sp)",
            "sw      s7, 28(sp)",
            "sw      s8, 32(sp)",
            "sw      s9, 36(sp)",
            "sw      s10, 40(sp)",
            "sw      s11, 44(sp)",

            // ============================================================
            // Setup
            // Inputs: a2=key, a3=rounds (bytes), a4=chaff, a5=order_vector
            // ============================================================
            "mv      s6, a5",               // s6 = order_vector
            "add     s7, a2, a3",           // s7 = real key end pointer
            "mv      s4, a2",               // s4 = real key current
            "mv      s5, a4",               // s5 = chaff key current

            // ============================================================
            // Load plaintext into both states
            // ============================================================
            "lw      a4, 0(a1)",
            "lw      a5, 4(a1)",
            "lw      a6, 8(a1)",
            "lw      a7, 12(a1)",
            "lw      s0, 0(a1)",
            "lw      s1, 4(a1)",
            "lw      s2, 8(a1)",
            "lw      s3, 12(a1)",

            // ============================================================
            // Initial AddRoundKey
            // ============================================================
            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",
            "xor     a4, a4, t0",
            "xor     a5, a5, t1",
            "xor     a6, a6, t2",
            "xor     a7, a7, t3",

            "lw      t0,  0(s5)",
            "lw      t1,  4(s5)",
            "lw      t2,  8(s5)",
            "lw      t3, 12(s5)",
            "xor     s0, s0, t0",
            "xor     s1, s1, t1",
            "xor     s2, s2, t2",
            "xor     s3, s3, t3",

            "addi    s4, s4, 16",
            "addi    s5, s5, 16",

            // ============================================================
            // Main loop
            // ============================================================
        "10:",
            "beq     s4, s7, 30f",
            "andi    t4, s6, 1",
            "srli    s6, s6, 1",
            "bnez    t4, 20f",

            "nop",   // compensate for branch not taken
            "nop",   // constant time confirmed in simulation
            "nop",

            // --- Path A: Real first ---
            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32esmi  t0, t0, a4, 0",
            "aes32esmi  t1, t1, a5, 0",
            "aes32esmi  t2, t2, a6, 0",
            "aes32esmi  t3, t3, a7, 0",
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
            "aes32esmi  t3, t3, a6, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32esmi  s8,  s8,  s0, 0",
            "aes32esmi  s9,  s9,  s1, 0",
            "aes32esmi  s10, s10, s2, 0",
            "aes32esmi  s11, s11, s3, 0",
            "aes32esmi  s8,  s8,  s1, 1",
            "aes32esmi  s9,  s9,  s2, 1",
            "aes32esmi  s10, s10, s3, 1",
            "aes32esmi  s11, s11, s0, 1",
            "aes32esmi  s8,  s8,  s2, 2",
            "aes32esmi  s9,  s9,  s3, 2",
            "aes32esmi  s10, s10, s0, 2",
            "aes32esmi  s11, s11, s1, 2",
            "aes32esmi  s8,  s8,  s3, 3",
            "aes32esmi  s9,  s9,  s0, 3",
            "aes32esmi  s10, s10, s1, 3",
            "aes32esmi  s11, s11, s2, 3",

            "mv       s0, s8",
            "mv       s1, s9",
            "mv       s2, s10",
            "mv       s3, s11",

            "j        25f",

        "20:",  // --- Path B: Chaff first ---
            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32esmi  s8,  s8,  s0, 0",
            "aes32esmi  s9,  s9,  s1, 0",
            "aes32esmi  s10, s10, s2, 0",
            "aes32esmi  s11, s11, s3, 0",
            "aes32esmi  s8,  s8,  s1, 1",
            "aes32esmi  s9,  s9,  s2, 1",
            "aes32esmi  s10, s10, s3, 1",
            "aes32esmi  s11, s11, s0, 1",
            "aes32esmi  s8,  s8,  s2, 2",
            "aes32esmi  s9,  s9,  s3, 2",
            "aes32esmi  s10, s10, s0, 2",
            "aes32esmi  s11, s11, s1, 2",
            "aes32esmi  s8,  s8,  s3, 3",
            "aes32esmi  s9,  s9,  s0, 3",
            "aes32esmi  s10, s10, s1, 3",
            "aes32esmi  s11, s11, s2, 3",

            "mv       s0, s8",
            "mv       s1, s9",
            "mv       s2, s10",
            "mv       s3, s11",

            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32esmi  t0, t0, a4, 0",
            "aes32esmi  t1, t1, a5, 0",
            "aes32esmi  t2, t2, a6, 0",
            "aes32esmi  t3, t3, a7, 0",
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
            "aes32esmi  t3, t3, a6, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            "j        25f", // dummy jump to even out paths

        "25:",  // Loop tail
            "addi    s4, s4, 16",
            "addi    s5, s5, 16",
            "j       10b",

            // ============================================================
            // Final round (no MixColumns)
            // ============================================================
        "30:",
            "lw      t0,  0(s4)",
            "lw      t1,  4(s4)",
            "lw      t2,  8(s4)",
            "lw      t3, 12(s4)",

            "aes32esi   t0, t0, a4, 0",
            "aes32esi   t1, t1, a5, 0",
            "aes32esi   t2, t2, a6, 0",
            "aes32esi   t3, t3, a7, 0",
            "aes32esi   t0, t0, a5, 1",
            "aes32esi   t1, t1, a6, 1",
            "aes32esi   t2, t2, a7, 1",
            "aes32esi   t3, t3, a4, 1",
            "aes32esi   t0, t0, a6, 2",
            "aes32esi   t1, t1, a7, 2",
            "aes32esi   t2, t2, a4, 2",
            "aes32esi   t3, t3, a5, 2",
            "aes32esi   t0, t0, a7, 3",
            "aes32esi   t1, t1, a4, 3",
            "aes32esi   t2, t2, a5, 3",
            "aes32esi   t3, t3, a6, 3",

            "mv       a4, t0",
            "mv       a5, t1",
            "mv       a6, t2",
            "mv       a7, t3",

            // Chaff final round (computed but discarded)
            "lw      s8,   0(s5)",
            "lw      s9,   4(s5)",
            "lw      s10,  8(s5)",
            "lw      s11, 12(s5)",

            "aes32esi   s8,  s8,  s0, 0",
            "aes32esi   s9,  s9,  s1, 0",
            "aes32esi   s10, s10, s2, 0",
            "aes32esi   s11, s11, s3, 0",
            "aes32esi   s8,  s8,  s1, 1",
            "aes32esi   s9,  s9,  s2, 1",
            "aes32esi   s10, s10, s3, 1",
            "aes32esi   s11, s11, s0, 1",
            "aes32esi   s8,  s8,  s2, 2",
            "aes32esi   s9,  s9,  s3, 2",
            "aes32esi   s10, s10, s0, 2",
            "aes32esi   s11, s11, s1, 2",
            "aes32esi   s8,  s8,  s3, 3",
            "aes32esi   s9,  s9,  s0, 3",
            "aes32esi   s10, s10, s1, 3",
            "aes32esi   s11, s11, s2, 3",

            // ============================================================
            // Store result
            // ============================================================
            "sw      a4, 0(a0)",
            "sw      a5, 4(a0)",
            "sw      a6, 8(a0)",
            "sw      a7, 12(a0)",

            // ============================================================
            // Epilogue
            // ============================================================
            "lw      s0,  0(sp)",
            "lw      s1,  4(sp)",
            "lw      s2,  8(sp)",
            "lw      s3, 12(sp)",
            "lw      s4, 16(sp)",
            "lw      s5, 20(sp)",
            "lw      s6, 24(sp)",
            "lw      s7, 28(sp)",
            "lw      s8, 32(sp)",
            "lw      s9, 36(sp)",
            "lw      s10, 40(sp)",
            "lw      s11, 44(sp)",
            "addi    sp, sp, 48",

            in("a0") ct.data.as_mut_ptr(),
            in("a1") pt.data.as_ptr(),
            in("a2") key.as_ptr(),
            in("a3") rounds,
            in("a4") chaff.as_ptr(),
            in("a5") order_vector,
            options(nostack)
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
                let nuke = self.enc_key.0.as_mut_ptr();
                for i in 0..self.enc_key.0.len() {
                    unsafe { nuke.add(i).write_volatile(0) };
                }
                let nuke = self.dec_key.0.as_mut_ptr();
                for i in 0..self.dec_key.0.len() {
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
