#![cfg_attr(not(target_os = "none"), allow(dead_code))]
// Copyright (c) 2020 Stu Small
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

// This murmur3 code was vendored in on April 28, 2021. We choose to vendor the code for this simple
// non-cryptographic hash in to reduce the number of crate dependencies in the build system. See
// https://github.com/betrusted-io/xous-core/issues/54 for more details on why.
// It also allowed us to adapt the code for our specific no_std needs.

const C1: u32 = 0x85eb_ca6b;
const C2: u32 = 0xc2b2_ae35;
const R1: u32 = 16;
const R2: u32 = 13;
const M: u32 = 5;
const N: u32 = 0xe654_6b64;

pub fn murmur3_32(source: &[u32], seed: u32) -> u32 {
    let mut processed = 0;
    let mut state = seed;

    for &k in source.iter() {
        processed += 4;
        state ^= calc_k(k);
        state = state.rotate_left(R2);
        state = (state.wrapping_mul(M)).wrapping_add(N);
    }
    finish(state, processed)
}

fn finish(state: u32, processed: u32) -> u32 {
    let mut hash = state;
    hash ^= processed as u32;
    hash ^= hash.wrapping_shr(R1);
    hash = hash.wrapping_mul(C1);
    hash ^= hash.wrapping_shr(R2);
    hash = hash.wrapping_mul(C2);
    hash ^= hash.wrapping_shr(R1);
    hash
}

fn calc_k(k: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    const R1: u32 = 15;
    k.wrapping_mul(C1).rotate_left(R1).wrapping_mul(C2)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Result {
        slice: &'static [u32],
        hash_32: u32,
    }

    #[test]
    fn test_static_slices() {
        let tests = [
            Result {
                slice: &[0x34333231], //"1234",
                hash_32: 0x721c5dc3,
            },
            Result {
                slice: &[0x34333231, 0x38373635], // "12345678",
                hash_32: 0x91b313ce,
            },
            Result { slice: &[], hash_32: 0 },
        ];

        for test in &tests {
            assert_eq!(murmur3_32(test.slice, 0), test.hash_32, "Failed on slice {:x?}", test.slice);
        }
    }
}
