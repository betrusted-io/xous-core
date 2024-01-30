// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use crate::{
    Bit,
    Endian,
};

/// Defines big endian bit shifting required for packing and unpacking non aligned fields in byte slices
///
/// Not construcatable, used only at type level
pub enum BigEndian {}
impl Endian for BigEndian {
    const IS_LITTLE: bool = false;
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

        // Case 2: More than 1 byte but the LSB at the end of the input
        // is already in position 0 so no shifting is required
        } else if E::USIZE == 0 {
            // Since we aren't shrinking the data down by aligning the fields, the output
            // buffer must be at least as long as the input
            assert!(output_bytes.len() >= input_bytes.len());

            // We need to align the ends of the arrays for big endian
            let o_start = o_len - i_len;

            // Memcopy all the data
            output_bytes[o_start..].copy_from_slice(input_bytes);

            if S::USIZE != 7 {
                // Sort out the masked first byte
                output_bytes[o_start] &= S::HEAD_MASK;
            }

        // Note: Case 3 and 4 could be merged with some minor tweaks around the input start
        //       and an extra negative offset to output index for case 4 but I've left them
        //       split up for ease of debugging while testing edge cases for now.
        // Case 3: More than 1 byte and LSB at the end ISN'T at position 0 and we aren't
        // shrinking the data down by aligning the fields. We need to shift every byte
        } else if S::USIZE >= E::USIZE {
            // Since we aren't shrinking the data down by aligning the fields, the output
            // buffer must be at least as long as the input
            assert!(output_bytes.len() >= input_bytes.len());

            // We need to align the ends of the arrays for big endian
            let o_start = o_len - i_len;

            for i in 0..i_len {
                output_bytes[o_start + i] =
                    match i {
                        // No prior byte, just masked and shifted left by the number of bits
                        // we need to fill the space in the last byte
                        0 => (input_bytes[i] & S::HEAD_MASK) >> E::USIZE,
                        // Prior byte is 0 so needs to be masked
                        // Shift the prior byte right to get only the bits we need to fill the space
                        // in the last byte. `8` because E is an inclusive bound labelled from LSB0
                        1 => ((input_bytes[i-1] & S::HEAD_MASK) << (8-E::USIZE))
                            | (input_bytes[i] >> E::USIZE),
                        // Prior byte is whole so no masking required
                        _ => (input_bytes[i-1] << (8-E::USIZE))
                            | (input_bytes[i] >> E::USIZE),
                    };
            }

        // Case 4: More than 1 byte and LSB at the end ISN'T at position 0 and we ARE
        // shrinking the data down by 1 byte by aligning the fields. We need to shift every byte
        } else {
            // Since we are shrinking the data down by aligning the fields, the output
            // buffer can be 1 smaller than the input
            assert!(output_bytes.len() >= input_bytes.len() - 1);

            // We need to align the ends of the arrays for big endian
            let o_start = o_len - (i_len - 1);

            for i in 1..i_len {
                output_bytes[o_start + i - 1] =
                    match i {
                        // No prior byte, just masked and shifted left by the number of bits
                        // we need to fill the space in the last byte
                        // (unreachable in case 4) 0 => (input_bytes[i] & S::HEAD_MASK) >> E::USIZE,

                        // Prior byte is 0 so needs to be masked
                        // Shift the prior byte right to get only the bits we need to fill the space
                        // in the last byte. `8` because E is an inclusive bound labelled from LSB0
                        1 => ((input_bytes[i-1] & S::HEAD_MASK) << (8-E::USIZE))
                            | (input_bytes[i] >> E::USIZE),
                        // Prior byte is whole so no masking required
                        _ => (input_bytes[i-1] << (8-E::USIZE))
                            | (input_bytes[i] >> E::USIZE),
                    };
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

        // Case 1: LSB at the end of the data is already in position 0 so no shifting is required
        if E::USIZE == 0 {
            let n_bytes = o_len.min(i_len);

            // Mask in the first byte
            output_bytes[o_len - n_bytes] |=
                input_bytes[i_len - n_bytes] & S::HEAD_MASK;

            // Memcopy any remaining bytes
            if n_bytes > 1 {
                output_bytes[(o_len - n_bytes + 1)..]
                    .copy_from_slice(&input_bytes[(i_len - n_bytes + 1)..]);
            }

        // Case 2: LSB isn't aligned with the end of the last byte so we have to shift every byte.
        // Since in restore we have to use the length of the output as critical info (can't tell
        // if a single byte S: 5, E: 4 is a 2 bit field or a 10 bit field without it) we just keep
        // shifting until we run out of output bytes.
        // TODO: Should field width in bits/bytes be a separate parameter? probably...
        } else {
            // i[n] contributes to o[n] and o[n-1]
            // i[n] and o[n] might be offset from each other in either direction

            let n_bytes = i_len.min(o_len);
            // Start of input such that this + n bytes aligns with the end of the input
            let i_start = i_len - n_bytes;
            // Start of output such that this + n bytes aligns with the end of the output
            let o_start = o_len - n_bytes;

            for i in 0..n_bytes {
                let i_i = i_start + i;
                let o_i = o_start + i;

                // The shifted current byte will always fit in output since n_bytes == min length
                if o_i == 0 {
                    // It's the first byte in the field, potentially requires masking
                    output_bytes[o_i] |= (input_bytes[i_i] << E::USIZE) & S::HEAD_MASK;
                } else {
                    output_bytes[o_i] |= input_bytes[i_i] << E::USIZE;
                }

                // Overflow from current byte might not fit in previous output byte
                if o_i == 1 {
                    // It's the first byte in the field, potentially requires masking
                    output_bytes[o_i-1] |= (input_bytes[i_i] >> 8 - E::USIZE) & S::HEAD_MASK;
                } else if o_i > 1 {
                    output_bytes[o_i-1] |= input_bytes[i_i] >> 8 - E::USIZE;
                }
            }
        }
    }
}