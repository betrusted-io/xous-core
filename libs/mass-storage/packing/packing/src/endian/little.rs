// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use crate::{
    Bit,
    Endian,
};

/// Defines little endian bit shifting required for packing and unpacking non aligned fields in byte slices
///
/// Not construcatable, used only at type level
pub enum LittleEndian {}
impl Endian for LittleEndian {
    const IS_LITTLE: bool = true;
    fn align_field_bits<S: Bit, E: Bit>(input_bytes: &[u8], output_bytes: &mut [u8]) {
        // Not valid to call this with no data
        assert!(input_bytes.len() > 0);
        // or no 0 wide output
        assert!(output_bytes.len() > 0);
        // Not valid to call with 1 byte and S E bits overlapping
        assert!(input_bytes.len() > 1 || S::USIZE >= E::USIZE);

        let i_len = input_bytes.len();
        let o_len = output_bytes.len();

        // Case 1: Only a single byte, can't shrink
        if i_len == 1 {
            output_bytes[o_len - 1] =
                // Mask away anything before the start bit
                (input_bytes[0] & S::HEAD_MASK)
                // Shift the field to the LSB
                >> E::USIZE;

        // Case 2: More than 1 byte but the MSB at the beginning of the input
        // is already in position 7 so no shifting is required
        } else if S::USIZE == 7 {
            // Since we aren't shrinking the data down by aligning the fields, the output
            // buffer must be at least as long as the input
            assert!(output_bytes.len() >= input_bytes.len());

            // Memcopy all the data
            output_bytes[..i_len]
                .copy_from_slice(input_bytes);

            if E::USIZE != 0 {
                // Align the LSB of the last byte if necessary
                output_bytes[i_len-1] >>= E::USIZE;
            }

        // Note: Case 3 and 4 could be merged with some minor tweaks around the input start
        //       and an extra negative offset to output index for case 4 but I've left them
        //       split up for ease of debugging while testing edge cases for now.
        // Case 3: More than 1 byte, MSB at the beginning ISN'T at position 7 and we aren't
        // shrinking the data down by aligning the fields. We need to shift every byte
        } else {
            let n_bytes = if S::USIZE >= E::USIZE {
                // Since we aren't shrinking the data down by aligning the fields, the output
                // buffer must be at least as long as the input
                assert!(output_bytes.len() >= input_bytes.len());
                i_len
            } else {
                // Since we are shrinking the data down by aligning the fields, the output
                // buffer can be 1 smaller than the input
                assert!(output_bytes.len() >= input_bytes.len() - 1);
                i_len - 1
            };

            // Little endian so align beginning of slices
            let last_byte = i_len - 1;
            for i in 0..n_bytes {
                output_bytes[i] = match i {
                    // First byte is aligned in LE as long as we're dealing with more than 1 byte
                    0 => input_bytes[i] & S::HEAD_MASK,
                    // Last byte might also need shifting for E
                    i if i == last_byte => (input_bytes[i] >> E::USIZE) >> (7-S::USIZE),
                    // Other bytes just need to be shifted to remove the bytes we shifted to the previous byte last iter
                    _ => input_bytes[i] >> (7-S::USIZE),
                };

                // If there's a next byte, take as many bits as will fit into the current byte
                let next_byte = i+1;
                if next_byte < i_len {
                    let val = if next_byte == last_byte {
                        // Take E into account
                        input_bytes[next_byte] >> E::USIZE
                    } else {
                        input_bytes[next_byte]
                    };

                    output_bytes[i] |= val << S::USIZE+1;
                }
            }
        }
    }

    fn restore_field_bits<S: Bit, E: Bit>(input_bytes: &[u8], output_bytes: &mut [u8]) {
        // Not valid to call this with no data
        assert!(input_bytes.len() > 0);
        // or no 0 wide output
        assert!(output_bytes.len() > 0);
        // Not valid to call with 1 byte and S E bits overlapping
        assert!(output_bytes.len() > 1 || S::USIZE >= E::USIZE);

        let i_len = input_bytes.len();
        let o_len = output_bytes.len();

        // Case 1: MSB at the beginning of the data is already in position 7 so no shifting is required
        if S::USIZE == 7 {
            let n_bytes = o_len.min(i_len);

            // Shift in the last byte
            output_bytes[o_len-1] |=
                input_bytes[i_len-1] << E::USIZE;

            // Memcopy any remaining bytes
            if n_bytes > 1 {
                let o_start = o_len - n_bytes;
                let o_end = o_start + n_bytes - 1;
                let i_start = i_len - n_bytes;
                let i_end = i_start + n_bytes - 1;

                output_bytes[o_start..o_end]
                    .copy_from_slice(&input_bytes[i_start..i_end]);
            }

        // Case 2: MSB of the beginning byte isn't aligned with the start so we have to shift every byte.
        // Since in restore we have to use the length of the output as critical info (can't tell
        // if a single byte S: 5, E: 4 is a 2 bit field or a 10 bit field without it) we just keep
        // shifting until we run out of output bytes.
        // TODO: Should field width in bits/bytes be a separate parameter? probably...
        } else {
            // i[n] contributes to o[n] and o[n-1]
            // i[n] and o[n] might be offset from each other in either direction

            let last_byte = o_len - 1;
            for i in 0..o_len {
                let current = if i == 0 {
                    // It's the first byte in the field, potentially requires masking
                    input_bytes[i] & S::HEAD_MASK
                } else if i < i_len {
                    // Shift to make room for the bits chopped off the previous byte
                    input_bytes[i] << 7 - S::USIZE
                } else {
                    0
                };

                let previous = if i > 0 && i - 1 < i_len {
                    // Add in what overflowed from prev byte
                    input_bytes[i-1] >> S::USIZE + 1
                } else {
                    0
                };

                let bits = previous | current;

                if i == 0 && i == last_byte {
                    // Single byte field
                    output_bytes[i] |= (bits << E::USIZE) & S::HEAD_MASK
                } else if i == 0 {
                    // First but not a single byte field
                    output_bytes[i] |= bits & S::HEAD_MASK
                } else if i == last_byte {
                    // Last but not a single byte field
                    output_bytes[i] |= bits << E::USIZE;
                } else {
                    // Not first, not last, not single
                    output_bytes[i] |= bits;
                }
            }
        }
    }
}