/*
COS_TABLE generated with:

#! /usr/bin/env python3
import math
print("const COS_TABLE: [u8; 256] = [", end="")
for i in range(256):
    if i % 16 == 0:
        print("\n", end="")
    print("{:03}, ".format(int(255.0 * math.cos( ((3.14159265359 / 2.0) / 256) * float(i) ))), end="")
print("];")
*/
/// One quarter of a cosine wave. The other four quarters are generated through symmetry.
const COS_TABLE: [u8; 256] = [
    255, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 253, 253, 253, 253, 253, 253,
    252, 252, 252, 252, 252, 251, 251, 251, 250, 250, 250, 250, 249, 249, 249, 248, 248, 248, 247, 247, 246,
    246, 246, 245, 245, 244, 244, 244, 243, 243, 242, 242, 241, 241, 240, 240, 239, 239, 238, 237, 237, 236,
    236, 235, 234, 234, 233, 233, 232, 231, 231, 230, 229, 229, 228, 227, 227, 226, 225, 224, 224, 223, 222,
    221, 221, 220, 219, 218, 217, 217, 216, 215, 214, 213, 212, 212, 211, 210, 209, 208, 207, 206, 205, 204,
    203, 202, 201, 201, 200, 199, 198, 197, 196, 195, 194, 193, 192, 191, 189, 188, 187, 186, 185, 184, 183,
    182, 181, 180, 179, 178, 176, 175, 174, 173, 172, 171, 170, 168, 167, 166, 165, 164, 162, 161, 160, 159,
    158, 156, 155, 154, 153, 151, 150, 149, 148, 146, 145, 144, 142, 141, 140, 139, 137, 136, 135, 133, 132,
    131, 129, 128, 127, 125, 124, 122, 121, 120, 118, 117, 116, 114, 113, 111, 110, 109, 107, 106, 104, 103,
    101, 100, 099, 097, 096, 094, 093, 091, 090, 088, 087, 085, 084, 082, 081, 079, 078, 077, 075, 074, 072,
    071, 069, 068, 066, 064, 063, 061, 060, 058, 057, 055, 054, 052, 051, 049, 048, 046, 045, 043, 042, 040,
    038, 037, 035, 034, 032, 031, 029, 028, 026, 024, 023, 021, 020, 018, 017, 015, 014, 012, 010, 009, 007,
    006, 004, 003, 001,
];

use std::f32::consts::PI;
enum Quadrant {
    FirstNormNorm,
    SecondReverseInvert,
    ThirdNormInvert,
    FourthReverseNorm,
}
/// LUT-based cosine computation. Has an `8-bit` resolution. Used to coarsely compute
/// a cosine without relying on `std` functions. The concerns driving this implementation are:
///
///   - code size
///   - xous-core issue #285 and https://github.com/rust-lang/rust/issues/105734
///
/// The issue above means any attempt to use `cos` (or any transcendental function) from
/// Rust math library leads to a link time error. Until Rust can fix this regression we have
/// to implement our own cosine function, or use `thin` LTO which adds about 7% to the size
/// of the kernel, making it by far the single largest contributing factor to kernel bloat.

pub fn cos(a: f32) -> f32 {
    let pos = a % (2.0 * PI);
    let q = if pos < (PI / 2.0) {
        Quadrant::FirstNormNorm
    } else if pos < (PI) {
        Quadrant::SecondReverseInvert
    } else if pos < (3.0 * PI / 2.0) {
        Quadrant::ThirdNormInvert
    } else {
        Quadrant::FourthReverseNorm
    };
    let idx = (256.0 * (a % (PI / 2.0)) / (PI / 2.0)) as usize;
    match q {
        Quadrant::FirstNormNorm => (COS_TABLE[idx] as f32) / 255.0,
        Quadrant::SecondReverseInvert => (COS_TABLE[255 - idx] as f32) / -255.0,
        Quadrant::ThirdNormInvert => (COS_TABLE[idx] as f32) / -255.0,
        Quadrant::FourthReverseNorm => (COS_TABLE[255 - idx] as f32) / 255.0,
    }
}
