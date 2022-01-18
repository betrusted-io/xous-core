// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// This code includes an adaptation of the the murmur3 hash algorithm.
// The murmur3 public domain notice, as retrieved on August 3, 2020 from
// https://github.com/aappleby/smhasher/blob/master/src/MurmurHash3.cpp,
// states:
// > MurmurHash3 was written by Austin Appleby, and is placed in the public
// > domain. The author hereby disclaims copyright to this source code.
//

/// Compute Murmur3 hash function of a u32 frame buffer.
/// This is intended for testing that the contents of a frame buffer match a
/// previous frame buffer state that was visually checked for correctness.
#[allow(dead_code)]
pub fn frame_buffer(fb: &[u32], seed: u32) -> u32 {
    let mut h = seed;
    let mut k;
    for word in fb.iter() {
        k = *word;
        k = k.wrapping_mul(0xcc9e2d51);
        k = k.rotate_left(15);
        k = k.wrapping_mul(0x1b873593);
        h ^= k;
        h = h.rotate_left(13);
        h = h.wrapping_mul(5);
        h = h.wrapping_add(0xe6546b64);
    }
    h ^= fb.len() as u32;
    // Finalize with avalanche
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_buffer_seed0_0x00000000_len1() {
        let fb: &[u32] = &[0x0];
        let seed = 0;
        // Note how the hash is not the same as for seed=1 in the test below
        assert_eq!(frame_buffer(fb, seed), 0x9B9CB39A);
    }

    #[test]
    fn test_frame_buffer_seed1_0x00000000_len1() {
        let fb: &[u32] = &[0x0];
        let seed = 1;
        // Note how the hash is not the same as for seed=0 in the test above
        assert_eq!(frame_buffer(fb, seed), 0xC8C1D2C1);
    }

    #[test]
    fn test_frame_buffer_seed0_0x00000100_len1() {
        let fb: &[u32] = &[0x00000500];
        let seed = 0;
        assert_eq!(frame_buffer(fb, seed), 0x7DEFDA4F);
    }

    #[test]
    fn test_frame_buffer_seed0_0x00000100_len2000() {
        let fb: &[u32] = &[0x00000500; 2000];
        let seed = 0;
        assert_eq!(frame_buffer(fb, seed), 0x4E61577A);
    }

    #[test]
    fn test_frame_buffer_seed0_0xffffffff_len2000() {
        let fb: &[u32] = &[0xFFFFFFFF; 2000];
        let seed = 0;
        assert_eq!(frame_buffer(fb, seed), 0x59F987C6);
    }
}
