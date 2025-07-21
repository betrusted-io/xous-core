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

use core::{convert::TryInto, fmt};

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
core::arch::global_asm!(
    ".global vex_aes_enc_id_0",
    "vex_aes_enc_id_0:",
    "    .word 0x26b50533", // vex_aes_enc_id a0, a1, a0, #0
    "    ret",
    ".global vex_aes_enc_id_1",
    "vex_aes_enc_id_1:",
    "    .word 0x66b50533", // vex_aes_enc_id a0, a1, a0, #1
    "    ret",
    ".global vex_aes_enc_id_2",
    "vex_aes_enc_id_2:",
    "    .word 0xa6b50533", // vex_aes_enc_id a0, a1, a0, #2
    "    ret",
    ".global vex_aes_enc_id_3",
    "vex_aes_enc_id_3:",
    "    .word 0xe6b50533", // vex_aes_enc_id a0, a1, a0, #3
    "    ret",
    ".global vex_aes_enc_id_last_0",
    "vex_aes_enc_id_last_0:",
    "    .word 0x22b50533", // vex_aes_enc_id_last a0, a1, a0, #0
    "    ret",
    ".global vex_aes_enc_id_last_1",
    "vex_aes_enc_id_last_1:",
    "    .word 0x62b50533", // vex_aes_enc_id_last a0, a1, a0, #1
    "    ret",
    ".global vex_aes_enc_id_last_2",
    "vex_aes_enc_id_last_2:",
    "    .word 0xa2b50533", // vex_aes_enc_id_last a0, a1, a0, #2
    "    ret",
    ".global vex_aes_enc_id_last_3",
    "vex_aes_enc_id_last_3:",
    "    .word 0xe2b50533", // vex_aes_enc_id_last a0, a1, a0, #3
    "    ret",
);

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
            "aes32dsmi  t0, t0, a7, 1",
            "aes32dsmi  t0, t0, a6, 2",
            "aes32dsmi  t0, t0, a5, 3",

            "aes32dsmi  t1, t1, a5, 0",
            "aes32dsmi  t1, t1, a4, 1",
            "aes32dsmi  t1, t1, a7, 2",
            "aes32dsmi  t1, t1, a6, 3",

            "aes32dsmi  t2, t2, a6, 0",
            "aes32dsmi  t2, t2, a5, 1",
            "aes32dsmi  t2, t2, a4, 2",
            "aes32dsmi  t2, t2, a7, 3",

            "aes32dsmi  t3, t3, a7, 0",
            "aes32dsmi  t3, t3, a6, 1",
            "aes32dsmi  t3, t3, a5, 2",
            "aes32dsmi  t3, t3, a4, 3",                    // U* contains new state

            "lw      a4,  0(a3)",                      // Load Round Key
            "lw      a5,  4(a3)",
            "lw      a6,  8(a3)",
            "lw      a7, 12(a3)",

            "beq     a2, a3, 30f", // aes_dec_block_l_finish Break from loop
            "addi    a3, a3, -32",                     // Step Key pointer

            "aes32dsmi  a4, a4, t0, 0",                    // Odd Round
            "aes32dsmi  a4, a4, t3, 1",
            "aes32dsmi  a4, a4, t2, 2",
            "aes32dsmi  a4, a4, t1, 3",

            "aes32dsmi  a5, a5, t1, 0",
            "aes32dsmi  a5, a5, t0, 1",
            "aes32dsmi  a5, a5, t3, 2",
            "aes32dsmi  a5, a5, t2, 3",

            "aes32dsmi  a6, a6, t2, 0",
            "aes32dsmi  a6, a6, t1, 1",
            "aes32dsmi  a6, a6, t0, 2",
            "aes32dsmi  a6, a6, t3, 3",

            "aes32dsmi  a7, a7, t3, 0",
            "aes32dsmi  a7, a7, t2, 1",
            "aes32dsmi  a7, a7, t1, 2",
            "aes32dsmi  a7, a7, t0, 3",                    // T* contains new state

            "j 20b", // .aes_dec_block_l0                         // repeat loop

        "30:", //.aes_dec_block_l_finish:

            "aes32dsi    a4, a4, t0, 0",                       // Final round, no MixColumns
            "aes32dsi    a4, a4, t3, 1",
            "aes32dsi    a4, a4, t2, 2",
            "aes32dsi    a4, a4, t1, 3",

            "aes32dsi    a5, a5, t1, 0",
            "aes32dsi    a5, a5, t0, 1",
            "aes32dsi    a5, a5, t3, 2",
            "aes32dsi    a5, a5, t2, 3",

            "aes32dsi    a6, a6, t2, 0",
            "aes32dsi    a6, a6, t1, 1",
            "aes32dsi    a6, a6, t0, 2",
            "aes32dsi    a6, a6, t3, 3",

            "aes32dsi    a7, a7, t3, 0",
            "aes32dsi    a7, a7, t2, 1",
            "aes32dsi    a7, a7, t1, 2",
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

#[derive(Clone, Copy, Debug)]
pub(crate) enum AesByte {
    Byte0 = 0,
    Byte1 = 1,
    Byte2 = 2,
    Byte3 = 3,
}

pub(crate) fn aes_enc_round(arg1: u32, arg2: u32, id: AesByte) -> u32 {
    extern "C" {
        fn vex_aes_enc_id_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesByte::Byte0 => unsafe { vex_aes_enc_id_0(arg1, arg2) },
        AesByte::Byte1 => unsafe { vex_aes_enc_id_1(arg1, arg2) },
        AesByte::Byte2 => unsafe { vex_aes_enc_id_2(arg1, arg2) },
        AesByte::Byte3 => unsafe { vex_aes_enc_id_3(arg1, arg2) },
    }
}

pub(crate) fn aes_enc_round_last(arg1: u32, arg2: u32, id: AesByte) -> u32 {
    extern "C" {
        fn vex_aes_enc_id_last_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesByte::Byte0 => unsafe { vex_aes_enc_id_last_0(arg1, arg2) },
        AesByte::Byte1 => unsafe { vex_aes_enc_id_last_1(arg1, arg2) },
        AesByte::Byte2 => unsafe { vex_aes_enc_id_last_2(arg1, arg2) },
        AesByte::Byte3 => unsafe { vex_aes_enc_id_last_3(arg1, arg2) },
    }
}

pub(crate) fn get_u32_le(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap())
}

pub(crate) fn set_u32(output: &mut [u8], offset: usize, value: u32) {
    let tmp = value.to_le_bytes();
    output[offset + 0] = tmp[0];
    output[offset + 1] = tmp[1];
    output[offset + 2] = tmp[2];
    output[offset + 3] = tmp[3];
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

// We leave this one encoded in legacy style to prove interop between styles
pub fn aes_vexriscv_encrypt(key: &VexKeys256, block: &[u8], rounds: u32) -> [u8; 16] {
    let rk = key;
    let mut input: [u8; 16] = [0; 16];
    for (&src, dst) in block.iter().zip(input.iter_mut()) {
        *dst = src;
    }

    // We do two rounds per loop
    let mut round_count = rounds / 2;

    let mut s0 = get_u32_le(&input, 0);
    let mut s1 = get_u32_le(&input, 4);
    let mut s2 = get_u32_le(&input, 8);
    let mut s3 = get_u32_le(&input, 12);

    let mut t0 = rk[0];
    let mut t1 = rk[1];
    let mut t2 = rk[2];
    let mut t3 = rk[3];

    s0 ^= t0;
    s1 ^= t1;
    s2 ^= t2;
    s3 ^= t3;

    let mut rk_offset = 0;
    loop {
        t0 = rk[rk_offset + 4];
        t1 = rk[rk_offset + 5];
        t2 = rk[rk_offset + 6];
        t3 = rk[rk_offset + 7];

        t0 = aes_enc_round(t0, s0, AesByte::Byte0);
        t1 = aes_enc_round(t1, s1, AesByte::Byte0);
        t2 = aes_enc_round(t2, s2, AesByte::Byte0);
        t3 = aes_enc_round(t3, s3, AesByte::Byte0);

        t0 = aes_enc_round(t0, s1, AesByte::Byte1);
        t1 = aes_enc_round(t1, s2, AesByte::Byte1);
        t2 = aes_enc_round(t2, s3, AesByte::Byte1);
        t3 = aes_enc_round(t3, s0, AesByte::Byte1);

        t0 = aes_enc_round(t0, s2, AesByte::Byte2);
        t1 = aes_enc_round(t1, s3, AesByte::Byte2);
        t2 = aes_enc_round(t2, s0, AesByte::Byte2);
        t3 = aes_enc_round(t3, s1, AesByte::Byte2);

        t0 = aes_enc_round(t0, s3, AesByte::Byte3);
        t1 = aes_enc_round(t1, s0, AesByte::Byte3);
        t2 = aes_enc_round(t2, s1, AesByte::Byte3);
        t3 = aes_enc_round(t3, s2, AesByte::Byte3);

        rk_offset += 8;
        round_count -= 1;
        if round_count == 0 {
            break;
        }

        s0 = rk[rk_offset + 0];
        s1 = rk[rk_offset + 1];
        s2 = rk[rk_offset + 2];
        s3 = rk[rk_offset + 3];

        s0 = aes_enc_round(s0, t0, AesByte::Byte0);
        s1 = aes_enc_round(s1, t1, AesByte::Byte0);
        s2 = aes_enc_round(s2, t2, AesByte::Byte0);
        s3 = aes_enc_round(s3, t3, AesByte::Byte0);

        s0 = aes_enc_round(s0, t1, AesByte::Byte1);
        s1 = aes_enc_round(s1, t2, AesByte::Byte1);
        s2 = aes_enc_round(s2, t3, AesByte::Byte1);
        s3 = aes_enc_round(s3, t0, AesByte::Byte1);

        s0 = aes_enc_round(s0, t2, AesByte::Byte2);
        s1 = aes_enc_round(s1, t3, AesByte::Byte2);
        s2 = aes_enc_round(s2, t0, AesByte::Byte2);
        s3 = aes_enc_round(s3, t1, AesByte::Byte2);

        s0 = aes_enc_round(s0, t3, AesByte::Byte3);
        s1 = aes_enc_round(s1, t0, AesByte::Byte3);
        s2 = aes_enc_round(s2, t1, AesByte::Byte3);
        s3 = aes_enc_round(s3, t2, AesByte::Byte3);
    }

    s0 = rk[rk_offset + 0];
    s1 = rk[rk_offset + 1];
    s2 = rk[rk_offset + 2];
    s3 = rk[rk_offset + 3];

    s0 = aes_enc_round_last(s0, t0, AesByte::Byte0);
    s1 = aes_enc_round_last(s1, t1, AesByte::Byte0);
    s2 = aes_enc_round_last(s2, t2, AesByte::Byte0);
    s3 = aes_enc_round_last(s3, t3, AesByte::Byte0);

    s0 = aes_enc_round_last(s0, t1, AesByte::Byte1);
    s1 = aes_enc_round_last(s1, t2, AesByte::Byte1);
    s2 = aes_enc_round_last(s2, t3, AesByte::Byte1);
    s3 = aes_enc_round_last(s3, t0, AesByte::Byte1);

    s0 = aes_enc_round_last(s0, t2, AesByte::Byte2);
    s1 = aes_enc_round_last(s1, t3, AesByte::Byte2);
    s2 = aes_enc_round_last(s2, t0, AesByte::Byte2);
    s3 = aes_enc_round_last(s3, t1, AesByte::Byte2);

    s0 = aes_enc_round_last(s0, t3, AesByte::Byte3);
    s1 = aes_enc_round_last(s1, t0, AesByte::Byte3);
    s2 = aes_enc_round_last(s2, t1, AesByte::Byte3);
    s3 = aes_enc_round_last(s3, t2, AesByte::Byte3);

    let mut output: [u8; 16] = [0; 16];
    set_u32(&mut output, 0, s0);
    set_u32(&mut output, 4, s1);
    set_u32(&mut output, 8, s2);
    set_u32(&mut output, 12, s3);
    output
}

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
    aes_vexriscv_encrypt,
    "AES-256 block cipher instance"
);
