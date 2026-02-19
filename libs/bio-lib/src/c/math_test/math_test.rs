use bao1x_api::bio_code;

#[rustfmt::skip]
bio_code!(math_test_bio_code, BM_MATH_TEST_BIO_START, BM_MATH_TEST_BIO_END,
    "20:", // _start: @_start
    // --- inline asm begin ---
    "lui	sp, 1",
    "j	21f",
    // --- inline asm end ---
    "21:", // main: @main
    "addi	sp, sp, -48",
    "sw	ra, 44(sp)", // 4-byte Folded Spill
    "sw	s0, 40(sp)", // 4-byte Folded Spill
    "sw	s1, 36(sp)", // 4-byte Folded Spill
    "lui	a1, 6",
    "lui	a3, 3",
    "li	ra, 1",
    "lui	s0, 1048396",
    "li	t0, -1",
    "lui	a4, 699051",
    "lui	a5, 0xa2f06",
    "lui    a2, 0x9",
    "slli   a2, a2, 4",
    "add    a5, a2, a5",
    "lui	a0, 2",
    "lui	s1, 1048574",
    "addi	a2, a1, 1160",
    "sw	a2, 12(sp)", // 4-byte Folded Spill
    "addi	a2, a3, 580",
    "sw	a2, 32(sp)", // 4-byte Folded Spill
    "addi	a2, a4, -1365",
    "sw	a2, 4(sp)", // 4-byte Folded Spill
    "addi	a1, a1, 1159",
    "sw	a1, 16(sp)", // 4-byte Folded Spill
    "addi	a1, a5, 1317",
    "sw	a1, 8(sp)", // 4-byte Folded Spill
    "addi	a0, a0, -1757",
    "sw	a0, 28(sp)", // 4-byte Folded Spill
    "addi	t1, s1, 1758",
    "22:", // .LBB1_1: =>This Loop Header: Depth=1
    // Child Loop BB1_2 Depth 2
    // Child Loop BB1_4 Depth 2
    // Child Loop BB1_16 Depth 2
    // Child Loop BB1_18 Depth 2
    "li	a3, 0",
    "li	s1, 0",
    // --- inline asm begin ---
    "mv	a1, x16",
    // --- inline asm end ---
    "slli	a1, a1, 12",
    "lw	a0, 32(sp)", // 4-byte Folded Reload
    // --- inline asm begin ---
    "mulh	a4, a1, a0",
    "mul	a1, a1, a0",
    // --- inline asm end ---
    "srli	a1, a1, 12",
    "slli	a4, a4, 19",
    "slli	a4, a4, 1",
    "or	t2, a4, a1",
    "srai	a4, a4, 31",
    "xor	a1, t2, a4",
    "sub	a4, a1, a4",
    "li	a5, 31",
    "23:", // .LBB1_2: Parent Loop BB1_1 Depth=1
    // =>  This Inner Loop Header: Depth=2
    "slli	a3, a3, 1",
    "srl	a1, a4, a5",
    "sll	a0, ra, a5",
    "addi	a5, a5, -1",
    "andi	a1, a1, 1",
    "or	a1, a1, a3",
    "srli	a3, a3, 14",
    "sltiu	a3, a3, 45",
    "addi	a3, a3, -1",
    "and	a0, a0, a3",
    "and	a3, a3, s0",
    "or	s1, s1, a0",
    "add	a3, a3, a1",
    "bne	a5, t0, 23b",
    // in Loop: Header=BB1_1 Depth=1
    "li	a1, 0",
    "li	a4, 0",
    "slli	a5, a3, 12",
    "li	a3, 31",
    "24:", // .LBB1_4: Parent Loop BB1_1 Depth=1
    // =>  This Inner Loop Header: Depth=2
    "slli	a1, a1, 1",
    "srl	a0, a5, a3",
    "sll	a2, ra, a3",
    "addi	a3, a3, -1",
    "andi	a0, a0, 1",
    "or	a0, a0, a1",
    "srli	a1, a1, 14",
    "sltiu	a1, a1, 45",
    "addi	a1, a1, -1",
    "and	a2, a2, a1",
    "and	a1, a1, s0",
    "or	a4, a4, a2",
    "add	a1, a1, a0",
    "bne	a3, t0, 24b",
    // in Loop: Header=BB1_1 Depth=1
    "slli	s1, s1, 12",
    "or	a4, a4, s1",
    "bgez	t2, 25f",
    // in Loop: Header=BB1_1 Depth=1
    "neg	a4, a4",
    "25:", // .LBB1_7: in Loop: Header=BB1_1 Depth=1
    "lw	a2, 12(sp)", // 4-byte Folded Reload
    "bgez	a4, 26f",
    // in Loop: Header=BB1_1 Depth=1
    "neg	a0, a4",
    "lw	a1, 4(sp)", // 4-byte Folded Reload
    "mulhu	a0, a0, a1",
    "srli	a0, a0, 14",
    "mul	a0, a0, a2",
    "add	a4, a4, a2",
    "add	a4, a4, a0",
    "26:", // .LBB1_9: in Loop: Header=BB1_1 Depth=1
    "mv	a1, a4",
    "lw	a0, 16(sp)", // 4-byte Folded Reload
    "blt	a0, a4, 27f",
    // in Loop: Header=BB1_1 Depth=1
    "lw	a1, 16(sp)", // 4-byte Folded Reload
    "27:", // .LBB1_11: in Loop: Header=BB1_1 Depth=1
    "lw	a0, 8(sp)", // 4-byte Folded Reload
    "mulhu	a0, a1, a0",
    "sub	a1, a1, a1",
    "srli	a0, a0, 14",
    "mul	a0, a0, a2",
    "add	a1, a1, a4",
    "sub	a1, a1, a0",
    "lw	a0, 32(sp)", // 4-byte Folded Reload
    "bge	a0, a1, 28f",
    // in Loop: Header=BB1_1 Depth=1
    "sub	a1, a2, a1",
    "28:", // .LBB1_13: in Loop: Header=BB1_1 Depth=1
    "mv	a3, a1",
    "lw	a0, 28(sp)", // 4-byte Folded Reload
    "blt	a1, a0, 29f",
    // in Loop: Header=BB1_1 Depth=1
    "lw	a0, 32(sp)", // 4-byte Folded Reload
    "sub	a3, a0, a1",
    "29:", // .LBB1_15: in Loop: Header=BB1_1 Depth=1
    "sw	a1, 24(sp)", // 4-byte Folded Spill
    "li	ra, 0",
    "li	s1, 0",
    "slli	a3, a3, 6",
    "srai	a0, a3, 31",
    "sw	a3, 20(sp)", // 4-byte Folded Spill
    "xor	a4, a3, a0",
    "sub	a4, a4, a0",
    "li	a3, 31",
    "li	s0, 1",
    "lui	a1, 2",
    "30:", // .LBB1_16: Parent Loop BB1_1 Depth=1
    // =>  This Inner Loop Header: Depth=2
    "slli	ra, ra, 1",
    "srl	a0, a4, a3",
    "addi	t2, a1, -1758",
    "sll	a2, s0, a3",
    "addi	a3, a3, -1",
    "andi	a0, a0, 1",
    "or	a0, a0, ra",
    "sltu	a5, ra, t2",
    "addi	a5, a5, -1",
    "and	a2, a2, a5",
    "and	a5, a5, t1",
    "or	s1, s1, a2",
    "add	ra, a5, a0",
    "bne	a3, t0, 30b",
    // in Loop: Header=BB1_1 Depth=1
    "li	a5, 0",
    "li	a4, 0",
    "slli	ra, ra, 12",
    "li	a3, 31",
    "31:", // .LBB1_18: Parent Loop BB1_1 Depth=1
    // =>  This Inner Loop Header: Depth=2
    "slli	a5, a5, 1",
    "srl	a0, ra, a3",
    "sll	a2, s0, a3",
    "addi	a3, a3, -1",
    "andi	a0, a0, 1",
    "or	a0, a0, a5",
    "sltu	a5, a5, t2",
    "addi	a5, a5, -1",
    "and	a2, a2, a5",
    "and	a5, a5, t1",
    "or	a4, a4, a2",
    "add	a5, a5, a0",
    "bne	a3, t0, 31b",
    // in Loop: Header=BB1_1 Depth=1
    "slli	a3, s1, 12",
    "or	a3, a3, a4",
    "lw	a1, 24(sp)", // 4-byte Folded Reload
    "addi   sp, sp, -4",
    "lw	a0, 24(sp)", // 4-byte Folded Reload
    "addi   sp, sp, 4",
    "bgez	a0, 32f",
    // in Loop: Header=BB1_1 Depth=1
    "neg	a3, a3",
    "32:", // .LBB1_21: in Loop: Header=BB1_1 Depth=1
    "lui	s0, 1048396",
    "li	ra, 1",
    "srai	a4, a3, 12",
    "li	a5, 63",
    "bge	a5, a4, 33f",
    // in Loop: Header=BB1_1 Depth=1
    "lui	a3, 1",
    "bge	a4, a5, 34f",
    "j	35f",
    "33:", // .LBB1_23: in Loop: Header=BB1_1 Depth=1
    "slli	a3, a3, 19",
    "slli	a3, a3, 1",
    "srli	a3, a3, 19",
    "srli	a3, a3, 1",
    "blt	a4, a5, 35f",
    "34:", // .LBB1_24: in Loop: Header=BB1_1 Depth=1
    "li	a4, 63",
    "35:", // .LBB1_25: in Loop: Header=BB1_1 Depth=1
    "slli	a4, a4, 1",
    "lui	a0, %hi(37f)",
    "addi	a0, a0, %lo(37f)",
    "add	a4, a4, a0",
    "lh	a0, 0(a4)",
    "lh	a2, 2(a4)",
    "sub	a2, a2, a0",
    // --- inline asm begin ---
    "mulh	a4, a2, a3",
    "mul	a2, a2, a3",
    // --- inline asm end ---
    "srli	a2, a2, 12",
    "slli	a4, a4, 19",
    "slli	a4, a4, 1",
    "or	a2, a2, a4",
    "add	a3, a2, a0",
    "lw	a0, 28(sp)", // 4-byte Folded Reload
    "blt	a1, a0, 36f",
    // in Loop: Header=BB1_1 Depth=1
    "neg	a3, a3",
    "36:", // .LBB1_27: in Loop: Header=BB1_1 Depth=1
    "lui	a1, 10",
    // --- inline asm begin ---
    "mulh	a0, a3, a1",
    "mul	a1, a3, a1",
    // --- inline asm end ---
    "srli	a1, a1, 12",
    "slli	a0, a0, 19",
    "slli	a0, a0, 1",
    "or	a0, a0, a1",
    "srai	a0, a0, 12",
    // --- inline asm begin ---
    "mv	x17, a0",
    // --- inline asm end ---
    "j	22b",
    // --- rodata: cos_table (65 entries, 33 words) ---
    ".p2align 1",
    "37:", // cos_table
    ".word 0x10001000", // [0] 4096, 4096
    ".word 0x0ffe0fff", // [4] 4095, 4094
    ".word 0x0ffa0ffc", // [8] 4092, 4090
    ".word 0x0ff30ff7", // [12] 4087, 4083
    ".word 0x0fe90fee", // [16] 4078, 4073
    ".word 0x0fdc0fe3", // [20] 4067, 4060
    ".word 0x0fcc0fd4", // [24] 4052, 4044
    ".word 0x0fb90fc3", // [28] 4035, 4025
    ".word 0x0fa30fae", // [32] 4014, 4003
    ".word 0x0f8a0f97", // [36] 3991, 3978
    ".word 0x0f6e0f7c", // [40] 3964, 3950
    ".word 0x0f4f0f5f", // [44] 3935, 3919
    ".word 0x0f2d0f3e", // [48] 3902, 3885
    ".word 0x0f080f1b", // [52] 3867, 3848
    ".word 0x0edf0ef4", // [56] 3828, 3807
    ".word 0x0eb40eca", // [60] 3786, 3764
    ".word 0x0e860e9d", // [64] 3741, 3718
    ".word 0x0e550e6e", // [68] 3694, 3669
    ".word 0x0e210e3b", // [72] 3643, 3617
    ".word 0x0deb0e06", // [76] 3590, 3563
    ".word 0x0db20dcf", // [80] 3535, 3506
    ".word 0x0d770d95", // [84] 3477, 3447
    ".word 0x0d3a0d59", // [88] 3417, 3386
    ".word 0x0cfa0d1a", // [92] 3354, 3322
    ".word 0x0cb80cd9", // [96] 3289, 3256
    ".word 0x0c740c96", // [100] 3222, 3188
    ".word 0x0c2e0c51", // [104] 3153, 3118
    ".word 0x0be60c0a", // [108] 3082, 3046
    ".word 0x0b9c0bc1", // [112] 3009, 2972
    ".word 0x0b510b77", // [116] 2935, 2897
    ".word 0x0b040b2b", // [120] 2859, 2820
    ".word 0x0ab60add", // [124] 2781, 2742
    ".word 0x00000a8e" // [128] 2702
);
