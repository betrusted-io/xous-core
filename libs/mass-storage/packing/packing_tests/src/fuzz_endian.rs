// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

#[cfg(test)]
use quickcheck::{ Arbitrary, Gen };

#[cfg(test)]
use quickcheck_macros::quickcheck;

// Moved the fuzzing tests to a separate file because of the large mess the generic paramters make...
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pretty_error;
    use packing::*;

    #[derive(Clone, Debug)]
    struct BitParams {
        start: u8,
        end: u8,
        data_bytes: usize,
        n: u128,
        total_bytes: usize,
        offset_bytes: usize,
        little_endian: bool,
    }

    impl Arbitrary for BitParams {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let data_bytes = 1 + (usize::arbitrary(g) % 15);
            let n = u128::arbitrary(g) % (16 * (data_bytes as u128));
            let offset_bytes = usize::arbitrary(g) % 10;
            let data_encoded_bytes = match data_bytes {
                1 => 1,
                2 => 2,
                3..=4 => 4,
                5..=8 => 8,
                _ => 16,
            };
            let total_bytes = data_encoded_bytes + offset_bytes + usize::arbitrary(g) % 10;

            let (start, end) = if data_bytes > 1 {
                ( u8::arbitrary(g) % 8, u8::arbitrary(g) % 8)
            } else {
                let start = u8::arbitrary(g) % 8;
                let end = if start == 0 {
                    0
                } else {
                    start - u8::arbitrary(g) % (start+1)
                };
                (start, end)
            };

            let little_endian = bool::arbitrary(g);

            Self {
                data_bytes,
                n,
                offset_bytes,
                total_bytes,
                start,
                end,
                little_endian,
            }
        }
    }


    #[quickcheck]
    fn qc_roundtrip(p: BitParams) {
        let bytes = match p.data_bytes {
            1 => {
                match p.little_endian {
                    true => <u8 as PackedBytes<[u8; 1]>>::to_bytes::<LittleEndian>(&(p.n as u8)),
                    false => <u8 as PackedBytes<[u8; 1]>>::to_bytes::<BigEndian>(&(p.n as u8)),
                }.unwrap().to_vec()
            },
            2 => {
                match p.little_endian {
                    true => <u16 as PackedBytes<[u8; 2]>>::to_bytes::<LittleEndian>(&(p.n as u16)),
                    false => <u16 as PackedBytes<[u8; 2]>>::to_bytes::<BigEndian>(&(p.n as u16)),
                }.unwrap().to_vec()
            },
            3..=4 => {
                match p.little_endian {
                    true => <u32 as PackedBytes<[u8; 4]>>::to_bytes::<LittleEndian>(&(p.n as u32)),
                    false => <u32 as PackedBytes<[u8; 4]>>::to_bytes::<BigEndian>(&(p.n as u32)),
                }.unwrap().to_vec()
            },
            5..=8 => {
                match p.little_endian {
                    true => <u64 as PackedBytes<[u8; 8]>>::to_bytes::<LittleEndian>(&(p.n as u64)),
                    false => <u64 as PackedBytes<[u8; 8]>>::to_bytes::<BigEndian>(&(p.n as u64)),
                }.unwrap().to_vec()
            },
            _ => {
                match p.little_endian {
                    true => <u128 as PackedBytes<[u8; 16]>>::to_bytes::<LittleEndian>(&p.n),
                    false => <u128 as PackedBytes<[u8; 16]>>::to_bytes::<BigEndian>(&p.n),
                }.unwrap().to_vec()
            },
        };

        let mut offset_bytes = Vec::new();
        offset_bytes.resize(p.total_bytes, 0);
        offset_bytes[p.offset_bytes..(p.offset_bytes + bytes.len())].copy_from_slice(&bytes);

        let bytes = offset_bytes;

        let mut restored_field_bytes = Vec::new();
        restored_field_bytes.resize(p.total_bytes, 0);

        match (p.start, p.end, p.little_endian) {
            (7, 7, true) => LittleEndian::restore_field_bits::<U7, U7>(&bytes, &mut restored_field_bytes),
            (7, 7, false) => BigEndian::restore_field_bits::<U7, U7>(&bytes, &mut restored_field_bytes),
            (7, 6, true) => LittleEndian::restore_field_bits::<U7, U6>(&bytes, &mut restored_field_bytes),
            (7, 6, false) => BigEndian::restore_field_bits::<U7, U6>(&bytes, &mut restored_field_bytes),
            (7, 5, true) => LittleEndian::restore_field_bits::<U7, U5>(&bytes, &mut restored_field_bytes),
            (7, 5, false) => BigEndian::restore_field_bits::<U7, U5>(&bytes, &mut restored_field_bytes),
            (7, 4, true) => LittleEndian::restore_field_bits::<U7, U4>(&bytes, &mut restored_field_bytes),
            (7, 4, false) => BigEndian::restore_field_bits::<U7, U4>(&bytes, &mut restored_field_bytes),
            (7, 3, true) => LittleEndian::restore_field_bits::<U7, U3>(&bytes, &mut restored_field_bytes),
            (7, 3, false) => BigEndian::restore_field_bits::<U7, U3>(&bytes, &mut restored_field_bytes),
            (7, 2, true) => LittleEndian::restore_field_bits::<U7, U2>(&bytes, &mut restored_field_bytes),
            (7, 2, false) => BigEndian::restore_field_bits::<U7, U2>(&bytes, &mut restored_field_bytes),
            (7, 1, true) => LittleEndian::restore_field_bits::<U7, U1>(&bytes, &mut restored_field_bytes),
            (7, 1, false) => BigEndian::restore_field_bits::<U7, U1>(&bytes, &mut restored_field_bytes),
            (7, 0, true) => LittleEndian::restore_field_bits::<U7, U0>(&bytes, &mut restored_field_bytes),
            (7, 0, false) => BigEndian::restore_field_bits::<U7, U0>(&bytes, &mut restored_field_bytes),

            (6, 7, true) => LittleEndian::restore_field_bits::<U6, U7>(&bytes, &mut restored_field_bytes),
            (6, 7, false) => BigEndian::restore_field_bits::<U6, U7>(&bytes, &mut restored_field_bytes),
            (6, 6, true) => LittleEndian::restore_field_bits::<U6, U6>(&bytes, &mut restored_field_bytes),
            (6, 6, false) => BigEndian::restore_field_bits::<U6, U6>(&bytes, &mut restored_field_bytes),
            (6, 5, true) => LittleEndian::restore_field_bits::<U6, U5>(&bytes, &mut restored_field_bytes),
            (6, 5, false) => BigEndian::restore_field_bits::<U6, U5>(&bytes, &mut restored_field_bytes),
            (6, 4, true) => LittleEndian::restore_field_bits::<U6, U4>(&bytes, &mut restored_field_bytes),
            (6, 4, false) => BigEndian::restore_field_bits::<U6, U4>(&bytes, &mut restored_field_bytes),
            (6, 3, true) => LittleEndian::restore_field_bits::<U6, U3>(&bytes, &mut restored_field_bytes),
            (6, 3, false) => BigEndian::restore_field_bits::<U6, U3>(&bytes, &mut restored_field_bytes),
            (6, 2, true) => LittleEndian::restore_field_bits::<U6, U2>(&bytes, &mut restored_field_bytes),
            (6, 2, false) => BigEndian::restore_field_bits::<U6, U2>(&bytes, &mut restored_field_bytes),
            (6, 1, true) => LittleEndian::restore_field_bits::<U6, U1>(&bytes, &mut restored_field_bytes),
            (6, 1, false) => BigEndian::restore_field_bits::<U6, U1>(&bytes, &mut restored_field_bytes),
            (6, 0, true) => LittleEndian::restore_field_bits::<U6, U0>(&bytes, &mut restored_field_bytes),
            (6, 0, false) => BigEndian::restore_field_bits::<U6, U0>(&bytes, &mut restored_field_bytes),

            (5, 7, true) => LittleEndian::restore_field_bits::<U5, U7>(&bytes, &mut restored_field_bytes),
            (5, 7, false) => BigEndian::restore_field_bits::<U5, U7>(&bytes, &mut restored_field_bytes),
            (5, 6, true) => LittleEndian::restore_field_bits::<U5, U6>(&bytes, &mut restored_field_bytes),
            (5, 6, false) => BigEndian::restore_field_bits::<U5, U6>(&bytes, &mut restored_field_bytes),
            (5, 5, true) => LittleEndian::restore_field_bits::<U5, U5>(&bytes, &mut restored_field_bytes),
            (5, 5, false) => BigEndian::restore_field_bits::<U5, U5>(&bytes, &mut restored_field_bytes),
            (5, 4, true) => LittleEndian::restore_field_bits::<U5, U4>(&bytes, &mut restored_field_bytes),
            (5, 4, false) => BigEndian::restore_field_bits::<U5, U4>(&bytes, &mut restored_field_bytes),
            (5, 3, true) => LittleEndian::restore_field_bits::<U5, U3>(&bytes, &mut restored_field_bytes),
            (5, 3, false) => BigEndian::restore_field_bits::<U5, U3>(&bytes, &mut restored_field_bytes),
            (5, 2, true) => LittleEndian::restore_field_bits::<U5, U2>(&bytes, &mut restored_field_bytes),
            (5, 2, false) => BigEndian::restore_field_bits::<U5, U2>(&bytes, &mut restored_field_bytes),
            (5, 1, true) => LittleEndian::restore_field_bits::<U5, U1>(&bytes, &mut restored_field_bytes),
            (5, 1, false) => BigEndian::restore_field_bits::<U5, U1>(&bytes, &mut restored_field_bytes),
            (5, 0, true) => LittleEndian::restore_field_bits::<U5, U0>(&bytes, &mut restored_field_bytes),
            (5, 0, false) => BigEndian::restore_field_bits::<U5, U0>(&bytes, &mut restored_field_bytes),

            (4, 7, true) => LittleEndian::restore_field_bits::<U4, U7>(&bytes, &mut restored_field_bytes),
            (4, 7, false) => BigEndian::restore_field_bits::<U4, U7>(&bytes, &mut restored_field_bytes),
            (4, 6, true) => LittleEndian::restore_field_bits::<U4, U6>(&bytes, &mut restored_field_bytes),
            (4, 6, false) => BigEndian::restore_field_bits::<U4, U6>(&bytes, &mut restored_field_bytes),
            (4, 5, true) => LittleEndian::restore_field_bits::<U4, U5>(&bytes, &mut restored_field_bytes),
            (4, 5, false) => BigEndian::restore_field_bits::<U4, U5>(&bytes, &mut restored_field_bytes),
            (4, 4, true) => LittleEndian::restore_field_bits::<U4, U4>(&bytes, &mut restored_field_bytes),
            (4, 4, false) => BigEndian::restore_field_bits::<U4, U4>(&bytes, &mut restored_field_bytes),
            (4, 3, true) => LittleEndian::restore_field_bits::<U4, U3>(&bytes, &mut restored_field_bytes),
            (4, 3, false) => BigEndian::restore_field_bits::<U4, U3>(&bytes, &mut restored_field_bytes),
            (4, 2, true) => LittleEndian::restore_field_bits::<U4, U2>(&bytes, &mut restored_field_bytes),
            (4, 2, false) => BigEndian::restore_field_bits::<U4, U2>(&bytes, &mut restored_field_bytes),
            (4, 1, true) => LittleEndian::restore_field_bits::<U4, U1>(&bytes, &mut restored_field_bytes),
            (4, 1, false) => BigEndian::restore_field_bits::<U4, U1>(&bytes, &mut restored_field_bytes),
            (4, 0, true) => LittleEndian::restore_field_bits::<U4, U0>(&bytes, &mut restored_field_bytes),
            (4, 0, false) => BigEndian::restore_field_bits::<U4, U0>(&bytes, &mut restored_field_bytes),

            (3, 7, true) => LittleEndian::restore_field_bits::<U3, U7>(&bytes, &mut restored_field_bytes),
            (3, 7, false) => BigEndian::restore_field_bits::<U3, U7>(&bytes, &mut restored_field_bytes),
            (3, 6, true) => LittleEndian::restore_field_bits::<U3, U6>(&bytes, &mut restored_field_bytes),
            (3, 6, false) => BigEndian::restore_field_bits::<U3, U6>(&bytes, &mut restored_field_bytes),
            (3, 5, true) => LittleEndian::restore_field_bits::<U3, U5>(&bytes, &mut restored_field_bytes),
            (3, 5, false) => BigEndian::restore_field_bits::<U3, U5>(&bytes, &mut restored_field_bytes),
            (3, 4, true) => LittleEndian::restore_field_bits::<U3, U4>(&bytes, &mut restored_field_bytes),
            (3, 4, false) => BigEndian::restore_field_bits::<U3, U4>(&bytes, &mut restored_field_bytes),
            (3, 3, true) => LittleEndian::restore_field_bits::<U3, U3>(&bytes, &mut restored_field_bytes),
            (3, 3, false) => BigEndian::restore_field_bits::<U3, U3>(&bytes, &mut restored_field_bytes),
            (3, 2, true) => LittleEndian::restore_field_bits::<U3, U2>(&bytes, &mut restored_field_bytes),
            (3, 2, false) => BigEndian::restore_field_bits::<U3, U2>(&bytes, &mut restored_field_bytes),
            (3, 1, true) => LittleEndian::restore_field_bits::<U3, U1>(&bytes, &mut restored_field_bytes),
            (3, 1, false) => BigEndian::restore_field_bits::<U3, U1>(&bytes, &mut restored_field_bytes),
            (3, 0, true) => LittleEndian::restore_field_bits::<U3, U0>(&bytes, &mut restored_field_bytes),
            (3, 0, false) => BigEndian::restore_field_bits::<U3, U0>(&bytes, &mut restored_field_bytes),

            (2, 7, true) => LittleEndian::restore_field_bits::<U2, U7>(&bytes, &mut restored_field_bytes),
            (2, 7, false) => BigEndian::restore_field_bits::<U2, U7>(&bytes, &mut restored_field_bytes),
            (2, 6, true) => LittleEndian::restore_field_bits::<U2, U6>(&bytes, &mut restored_field_bytes),
            (2, 6, false) => BigEndian::restore_field_bits::<U2, U6>(&bytes, &mut restored_field_bytes),
            (2, 5, true) => LittleEndian::restore_field_bits::<U2, U5>(&bytes, &mut restored_field_bytes),
            (2, 5, false) => BigEndian::restore_field_bits::<U2, U5>(&bytes, &mut restored_field_bytes),
            (2, 4, true) => LittleEndian::restore_field_bits::<U2, U4>(&bytes, &mut restored_field_bytes),
            (2, 4, false) => BigEndian::restore_field_bits::<U2, U4>(&bytes, &mut restored_field_bytes),
            (2, 3, true) => LittleEndian::restore_field_bits::<U2, U3>(&bytes, &mut restored_field_bytes),
            (2, 3, false) => BigEndian::restore_field_bits::<U2, U3>(&bytes, &mut restored_field_bytes),
            (2, 2, true) => LittleEndian::restore_field_bits::<U2, U2>(&bytes, &mut restored_field_bytes),
            (2, 2, false) => BigEndian::restore_field_bits::<U2, U2>(&bytes, &mut restored_field_bytes),
            (2, 1, true) => LittleEndian::restore_field_bits::<U2, U1>(&bytes, &mut restored_field_bytes),
            (2, 1, false) => BigEndian::restore_field_bits::<U2, U1>(&bytes, &mut restored_field_bytes),
            (2, 0, true) => LittleEndian::restore_field_bits::<U2, U0>(&bytes, &mut restored_field_bytes),
            (2, 0, false) => BigEndian::restore_field_bits::<U2, U0>(&bytes, &mut restored_field_bytes),

            (1, 7, true) => LittleEndian::restore_field_bits::<U1, U7>(&bytes, &mut restored_field_bytes),
            (1, 7, false) => BigEndian::restore_field_bits::<U1, U7>(&bytes, &mut restored_field_bytes),
            (1, 6, true) => LittleEndian::restore_field_bits::<U1, U6>(&bytes, &mut restored_field_bytes),
            (1, 6, false) => BigEndian::restore_field_bits::<U1, U6>(&bytes, &mut restored_field_bytes),
            (1, 5, true) => LittleEndian::restore_field_bits::<U1, U5>(&bytes, &mut restored_field_bytes),
            (1, 5, false) => BigEndian::restore_field_bits::<U1, U5>(&bytes, &mut restored_field_bytes),
            (1, 4, true) => LittleEndian::restore_field_bits::<U1, U4>(&bytes, &mut restored_field_bytes),
            (1, 4, false) => BigEndian::restore_field_bits::<U1, U4>(&bytes, &mut restored_field_bytes),
            (1, 3, true) => LittleEndian::restore_field_bits::<U1, U3>(&bytes, &mut restored_field_bytes),
            (1, 3, false) => BigEndian::restore_field_bits::<U1, U3>(&bytes, &mut restored_field_bytes),
            (1, 2, true) => LittleEndian::restore_field_bits::<U1, U2>(&bytes, &mut restored_field_bytes),
            (1, 2, false) => BigEndian::restore_field_bits::<U1, U2>(&bytes, &mut restored_field_bytes),
            (1, 1, true) => LittleEndian::restore_field_bits::<U1, U1>(&bytes, &mut restored_field_bytes),
            (1, 1, false) => BigEndian::restore_field_bits::<U1, U1>(&bytes, &mut restored_field_bytes),
            (1, 0, true) => LittleEndian::restore_field_bits::<U1, U0>(&bytes, &mut restored_field_bytes),
            (1, 0, false) => BigEndian::restore_field_bits::<U1, U0>(&bytes, &mut restored_field_bytes),

            (0, 7, true) => LittleEndian::restore_field_bits::<U0, U7>(&bytes, &mut restored_field_bytes),
            (0, 7, false) => BigEndian::restore_field_bits::<U0, U7>(&bytes, &mut restored_field_bytes),
            (0, 6, true) => LittleEndian::restore_field_bits::<U0, U6>(&bytes, &mut restored_field_bytes),
            (0, 6, false) => BigEndian::restore_field_bits::<U0, U6>(&bytes, &mut restored_field_bytes),
            (0, 5, true) => LittleEndian::restore_field_bits::<U0, U5>(&bytes, &mut restored_field_bytes),
            (0, 5, false) => BigEndian::restore_field_bits::<U0, U5>(&bytes, &mut restored_field_bytes),
            (0, 4, true) => LittleEndian::restore_field_bits::<U0, U4>(&bytes, &mut restored_field_bytes),
            (0, 4, false) => BigEndian::restore_field_bits::<U0, U4>(&bytes, &mut restored_field_bytes),
            (0, 3, true) => LittleEndian::restore_field_bits::<U0, U3>(&bytes, &mut restored_field_bytes),
            (0, 3, false) => BigEndian::restore_field_bits::<U0, U3>(&bytes, &mut restored_field_bytes),
            (0, 2, true) => LittleEndian::restore_field_bits::<U0, U2>(&bytes, &mut restored_field_bytes),
            (0, 2, false) => BigEndian::restore_field_bits::<U0, U2>(&bytes, &mut restored_field_bytes),
            (0, 1, true) => LittleEndian::restore_field_bits::<U0, U1>(&bytes, &mut restored_field_bytes),
            (0, 1, false) => BigEndian::restore_field_bits::<U0, U1>(&bytes, &mut restored_field_bytes),
            (0, 0, true) => LittleEndian::restore_field_bits::<U0, U0>(&bytes, &mut restored_field_bytes),
            (0, 0, false) => BigEndian::restore_field_bits::<U0, U0>(&bytes, &mut restored_field_bytes),
            _ => unimplemented!(),
        }

        let mut aligned_field_bytes = Vec::new();
        aligned_field_bytes.resize(p.total_bytes, 0);

        match (p.start, p.end, p.little_endian) {
            (7, 7, true) => LittleEndian::align_field_bits::<U7, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 7, false) => BigEndian::align_field_bits::<U7, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 6, true) => LittleEndian::align_field_bits::<U7, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 6, false) => BigEndian::align_field_bits::<U7, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 5, true) => LittleEndian::align_field_bits::<U7, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 5, false) => BigEndian::align_field_bits::<U7, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 4, true) => LittleEndian::align_field_bits::<U7, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 4, false) => BigEndian::align_field_bits::<U7, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 3, true) => LittleEndian::align_field_bits::<U7, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 3, false) => BigEndian::align_field_bits::<U7, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 2, true) => LittleEndian::align_field_bits::<U7, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 2, false) => BigEndian::align_field_bits::<U7, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 1, true) => LittleEndian::align_field_bits::<U7, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 1, false) => BigEndian::align_field_bits::<U7, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 0, true) => LittleEndian::align_field_bits::<U7, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (7, 0, false) => BigEndian::align_field_bits::<U7, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (6, 7, true) => LittleEndian::align_field_bits::<U6, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 7, false) => BigEndian::align_field_bits::<U6, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 6, true) => LittleEndian::align_field_bits::<U6, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 6, false) => BigEndian::align_field_bits::<U6, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 5, true) => LittleEndian::align_field_bits::<U6, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 5, false) => BigEndian::align_field_bits::<U6, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 4, true) => LittleEndian::align_field_bits::<U6, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 4, false) => BigEndian::align_field_bits::<U6, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 3, true) => LittleEndian::align_field_bits::<U6, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 3, false) => BigEndian::align_field_bits::<U6, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 2, true) => LittleEndian::align_field_bits::<U6, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 2, false) => BigEndian::align_field_bits::<U6, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 1, true) => LittleEndian::align_field_bits::<U6, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 1, false) => BigEndian::align_field_bits::<U6, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 0, true) => LittleEndian::align_field_bits::<U6, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (6, 0, false) => BigEndian::align_field_bits::<U6, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (5, 7, true) => LittleEndian::align_field_bits::<U5, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 7, false) => BigEndian::align_field_bits::<U5, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 6, true) => LittleEndian::align_field_bits::<U5, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 6, false) => BigEndian::align_field_bits::<U5, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 5, true) => LittleEndian::align_field_bits::<U5, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 5, false) => BigEndian::align_field_bits::<U5, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 4, true) => LittleEndian::align_field_bits::<U5, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 4, false) => BigEndian::align_field_bits::<U5, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 3, true) => LittleEndian::align_field_bits::<U5, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 3, false) => BigEndian::align_field_bits::<U5, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 2, true) => LittleEndian::align_field_bits::<U5, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 2, false) => BigEndian::align_field_bits::<U5, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 1, true) => LittleEndian::align_field_bits::<U5, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 1, false) => BigEndian::align_field_bits::<U5, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 0, true) => LittleEndian::align_field_bits::<U5, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (5, 0, false) => BigEndian::align_field_bits::<U5, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (4, 7, true) => LittleEndian::align_field_bits::<U4, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 7, false) => BigEndian::align_field_bits::<U4, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 6, true) => LittleEndian::align_field_bits::<U4, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 6, false) => BigEndian::align_field_bits::<U4, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 5, true) => LittleEndian::align_field_bits::<U4, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 5, false) => BigEndian::align_field_bits::<U4, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 4, true) => LittleEndian::align_field_bits::<U4, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 4, false) => BigEndian::align_field_bits::<U4, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 3, true) => LittleEndian::align_field_bits::<U4, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 3, false) => BigEndian::align_field_bits::<U4, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 2, true) => LittleEndian::align_field_bits::<U4, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 2, false) => BigEndian::align_field_bits::<U4, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 1, true) => LittleEndian::align_field_bits::<U4, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 1, false) => BigEndian::align_field_bits::<U4, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 0, true) => LittleEndian::align_field_bits::<U4, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (4, 0, false) => BigEndian::align_field_bits::<U4, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (3, 7, true) => LittleEndian::align_field_bits::<U3, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 7, false) => BigEndian::align_field_bits::<U3, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 6, true) => LittleEndian::align_field_bits::<U3, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 6, false) => BigEndian::align_field_bits::<U3, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 5, true) => LittleEndian::align_field_bits::<U3, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 5, false) => BigEndian::align_field_bits::<U3, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 4, true) => LittleEndian::align_field_bits::<U3, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 4, false) => BigEndian::align_field_bits::<U3, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 3, true) => LittleEndian::align_field_bits::<U3, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 3, false) => BigEndian::align_field_bits::<U3, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 2, true) => LittleEndian::align_field_bits::<U3, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 2, false) => BigEndian::align_field_bits::<U3, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 1, true) => LittleEndian::align_field_bits::<U3, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 1, false) => BigEndian::align_field_bits::<U3, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 0, true) => LittleEndian::align_field_bits::<U3, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (3, 0, false) => BigEndian::align_field_bits::<U3, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (2, 7, true) => LittleEndian::align_field_bits::<U2, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 7, false) => BigEndian::align_field_bits::<U2, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 6, true) => LittleEndian::align_field_bits::<U2, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 6, false) => BigEndian::align_field_bits::<U2, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 5, true) => LittleEndian::align_field_bits::<U2, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 5, false) => BigEndian::align_field_bits::<U2, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 4, true) => LittleEndian::align_field_bits::<U2, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 4, false) => BigEndian::align_field_bits::<U2, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 3, true) => LittleEndian::align_field_bits::<U2, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 3, false) => BigEndian::align_field_bits::<U2, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 2, true) => LittleEndian::align_field_bits::<U2, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 2, false) => BigEndian::align_field_bits::<U2, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 1, true) => LittleEndian::align_field_bits::<U2, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 1, false) => BigEndian::align_field_bits::<U2, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 0, true) => LittleEndian::align_field_bits::<U2, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (2, 0, false) => BigEndian::align_field_bits::<U2, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (1, 7, true) => LittleEndian::align_field_bits::<U1, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 7, false) => BigEndian::align_field_bits::<U1, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 6, true) => LittleEndian::align_field_bits::<U1, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 6, false) => BigEndian::align_field_bits::<U1, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 5, true) => LittleEndian::align_field_bits::<U1, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 5, false) => BigEndian::align_field_bits::<U1, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 4, true) => LittleEndian::align_field_bits::<U1, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 4, false) => BigEndian::align_field_bits::<U1, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 3, true) => LittleEndian::align_field_bits::<U1, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 3, false) => BigEndian::align_field_bits::<U1, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 2, true) => LittleEndian::align_field_bits::<U1, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 2, false) => BigEndian::align_field_bits::<U1, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 1, true) => LittleEndian::align_field_bits::<U1, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 1, false) => BigEndian::align_field_bits::<U1, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 0, true) => LittleEndian::align_field_bits::<U1, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (1, 0, false) => BigEndian::align_field_bits::<U1, U0>(&restored_field_bytes, &mut aligned_field_bytes),

            (0, 7, true) => LittleEndian::align_field_bits::<U0, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 7, false) => BigEndian::align_field_bits::<U0, U7>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 6, true) => LittleEndian::align_field_bits::<U0, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 6, false) => BigEndian::align_field_bits::<U0, U6>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 5, true) => LittleEndian::align_field_bits::<U0, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 5, false) => BigEndian::align_field_bits::<U0, U5>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 4, true) => LittleEndian::align_field_bits::<U0, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 4, false) => BigEndian::align_field_bits::<U0, U4>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 3, true) => LittleEndian::align_field_bits::<U0, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 3, false) => BigEndian::align_field_bits::<U0, U3>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 2, true) => LittleEndian::align_field_bits::<U0, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 2, false) => BigEndian::align_field_bits::<U0, U2>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 1, true) => LittleEndian::align_field_bits::<U0, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 1, false) => BigEndian::align_field_bits::<U0, U1>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 0, true) => LittleEndian::align_field_bits::<U0, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            (0, 0, false) => BigEndian::align_field_bits::<U0, U0>(&restored_field_bytes, &mut aligned_field_bytes),
            _ => unimplemented!(),
        }


        let mut restored_field_bytes2 = Vec::new();
        restored_field_bytes2.resize(p.total_bytes, 0);

        match (p.start, p.end, p.little_endian) {
            (7, 7, true) => LittleEndian::restore_field_bits::<U7, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 7, false) => BigEndian::restore_field_bits::<U7, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 6, true) => LittleEndian::restore_field_bits::<U7, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 6, false) => BigEndian::restore_field_bits::<U7, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 5, true) => LittleEndian::restore_field_bits::<U7, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 5, false) => BigEndian::restore_field_bits::<U7, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 4, true) => LittleEndian::restore_field_bits::<U7, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 4, false) => BigEndian::restore_field_bits::<U7, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 3, true) => LittleEndian::restore_field_bits::<U7, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 3, false) => BigEndian::restore_field_bits::<U7, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 2, true) => LittleEndian::restore_field_bits::<U7, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 2, false) => BigEndian::restore_field_bits::<U7, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 1, true) => LittleEndian::restore_field_bits::<U7, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 1, false) => BigEndian::restore_field_bits::<U7, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 0, true) => LittleEndian::restore_field_bits::<U7, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (7, 0, false) => BigEndian::restore_field_bits::<U7, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (6, 7, true) => LittleEndian::restore_field_bits::<U6, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 7, false) => BigEndian::restore_field_bits::<U6, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 6, true) => LittleEndian::restore_field_bits::<U6, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 6, false) => BigEndian::restore_field_bits::<U6, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 5, true) => LittleEndian::restore_field_bits::<U6, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 5, false) => BigEndian::restore_field_bits::<U6, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 4, true) => LittleEndian::restore_field_bits::<U6, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 4, false) => BigEndian::restore_field_bits::<U6, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 3, true) => LittleEndian::restore_field_bits::<U6, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 3, false) => BigEndian::restore_field_bits::<U6, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 2, true) => LittleEndian::restore_field_bits::<U6, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 2, false) => BigEndian::restore_field_bits::<U6, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 1, true) => LittleEndian::restore_field_bits::<U6, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 1, false) => BigEndian::restore_field_bits::<U6, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 0, true) => LittleEndian::restore_field_bits::<U6, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (6, 0, false) => BigEndian::restore_field_bits::<U6, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (5, 7, true) => LittleEndian::restore_field_bits::<U5, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 7, false) => BigEndian::restore_field_bits::<U5, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 6, true) => LittleEndian::restore_field_bits::<U5, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 6, false) => BigEndian::restore_field_bits::<U5, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 5, true) => LittleEndian::restore_field_bits::<U5, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 5, false) => BigEndian::restore_field_bits::<U5, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 4, true) => LittleEndian::restore_field_bits::<U5, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 4, false) => BigEndian::restore_field_bits::<U5, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 3, true) => LittleEndian::restore_field_bits::<U5, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 3, false) => BigEndian::restore_field_bits::<U5, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 2, true) => LittleEndian::restore_field_bits::<U5, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 2, false) => BigEndian::restore_field_bits::<U5, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 1, true) => LittleEndian::restore_field_bits::<U5, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 1, false) => BigEndian::restore_field_bits::<U5, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 0, true) => LittleEndian::restore_field_bits::<U5, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (5, 0, false) => BigEndian::restore_field_bits::<U5, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (4, 7, true) => LittleEndian::restore_field_bits::<U4, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 7, false) => BigEndian::restore_field_bits::<U4, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 6, true) => LittleEndian::restore_field_bits::<U4, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 6, false) => BigEndian::restore_field_bits::<U4, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 5, true) => LittleEndian::restore_field_bits::<U4, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 5, false) => BigEndian::restore_field_bits::<U4, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 4, true) => LittleEndian::restore_field_bits::<U4, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 4, false) => BigEndian::restore_field_bits::<U4, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 3, true) => LittleEndian::restore_field_bits::<U4, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 3, false) => BigEndian::restore_field_bits::<U4, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 2, true) => LittleEndian::restore_field_bits::<U4, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 2, false) => BigEndian::restore_field_bits::<U4, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 1, true) => LittleEndian::restore_field_bits::<U4, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 1, false) => BigEndian::restore_field_bits::<U4, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 0, true) => LittleEndian::restore_field_bits::<U4, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (4, 0, false) => BigEndian::restore_field_bits::<U4, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (3, 7, true) => LittleEndian::restore_field_bits::<U3, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 7, false) => BigEndian::restore_field_bits::<U3, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 6, true) => LittleEndian::restore_field_bits::<U3, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 6, false) => BigEndian::restore_field_bits::<U3, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 5, true) => LittleEndian::restore_field_bits::<U3, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 5, false) => BigEndian::restore_field_bits::<U3, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 4, true) => LittleEndian::restore_field_bits::<U3, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 4, false) => BigEndian::restore_field_bits::<U3, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 3, true) => LittleEndian::restore_field_bits::<U3, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 3, false) => BigEndian::restore_field_bits::<U3, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 2, true) => LittleEndian::restore_field_bits::<U3, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 2, false) => BigEndian::restore_field_bits::<U3, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 1, true) => LittleEndian::restore_field_bits::<U3, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 1, false) => BigEndian::restore_field_bits::<U3, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 0, true) => LittleEndian::restore_field_bits::<U3, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (3, 0, false) => BigEndian::restore_field_bits::<U3, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (2, 7, true) => LittleEndian::restore_field_bits::<U2, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 7, false) => BigEndian::restore_field_bits::<U2, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 6, true) => LittleEndian::restore_field_bits::<U2, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 6, false) => BigEndian::restore_field_bits::<U2, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 5, true) => LittleEndian::restore_field_bits::<U2, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 5, false) => BigEndian::restore_field_bits::<U2, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 4, true) => LittleEndian::restore_field_bits::<U2, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 4, false) => BigEndian::restore_field_bits::<U2, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 3, true) => LittleEndian::restore_field_bits::<U2, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 3, false) => BigEndian::restore_field_bits::<U2, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 2, true) => LittleEndian::restore_field_bits::<U2, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 2, false) => BigEndian::restore_field_bits::<U2, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 1, true) => LittleEndian::restore_field_bits::<U2, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 1, false) => BigEndian::restore_field_bits::<U2, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 0, true) => LittleEndian::restore_field_bits::<U2, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (2, 0, false) => BigEndian::restore_field_bits::<U2, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (1, 7, true) => LittleEndian::restore_field_bits::<U1, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 7, false) => BigEndian::restore_field_bits::<U1, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 6, true) => LittleEndian::restore_field_bits::<U1, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 6, false) => BigEndian::restore_field_bits::<U1, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 5, true) => LittleEndian::restore_field_bits::<U1, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 5, false) => BigEndian::restore_field_bits::<U1, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 4, true) => LittleEndian::restore_field_bits::<U1, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 4, false) => BigEndian::restore_field_bits::<U1, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 3, true) => LittleEndian::restore_field_bits::<U1, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 3, false) => BigEndian::restore_field_bits::<U1, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 2, true) => LittleEndian::restore_field_bits::<U1, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 2, false) => BigEndian::restore_field_bits::<U1, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 1, true) => LittleEndian::restore_field_bits::<U1, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 1, false) => BigEndian::restore_field_bits::<U1, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 0, true) => LittleEndian::restore_field_bits::<U1, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (1, 0, false) => BigEndian::restore_field_bits::<U1, U0>(&aligned_field_bytes, &mut restored_field_bytes2),

            (0, 7, true) => LittleEndian::restore_field_bits::<U0, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 7, false) => BigEndian::restore_field_bits::<U0, U7>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 6, true) => LittleEndian::restore_field_bits::<U0, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 6, false) => BigEndian::restore_field_bits::<U0, U6>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 5, true) => LittleEndian::restore_field_bits::<U0, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 5, false) => BigEndian::restore_field_bits::<U0, U5>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 4, true) => LittleEndian::restore_field_bits::<U0, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 4, false) => BigEndian::restore_field_bits::<U0, U4>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 3, true) => LittleEndian::restore_field_bits::<U0, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 3, false) => BigEndian::restore_field_bits::<U0, U3>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 2, true) => LittleEndian::restore_field_bits::<U0, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 2, false) => BigEndian::restore_field_bits::<U0, U2>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 1, true) => LittleEndian::restore_field_bits::<U0, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 1, false) => BigEndian::restore_field_bits::<U0, U1>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 0, true) => LittleEndian::restore_field_bits::<U0, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            (0, 0, false) => BigEndian::restore_field_bits::<U0, U0>(&aligned_field_bytes, &mut restored_field_bytes2),
            _ => unimplemented!(),
        }

        if let Err(e) = pretty_error(
            &aligned_field_bytes,
            &restored_field_bytes2,
            &restored_field_bytes,
            p.start as usize,
            p.end as usize,
        ) {
            panic!("restored->aligned->restored round trip failed: {}", e);
        }
    }
}