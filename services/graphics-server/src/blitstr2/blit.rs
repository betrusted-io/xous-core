// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
use super::cliprect::ClipRect;
use crate::GlyphSprite;
#[allow(unused_imports)]
use super::{LINES, WORDS_PER_LINE, WIDTH, FrBuf};
use crate::api::Point;

/// Null glyph to use when everything else fails
pub const NULL_GLYPH: [u32; 8] = [0, 0x5500AA, 0x5500AA, 0x5500AA, 0x5500AA, 0x5500AA, 0, 0];
pub const NULL_GLYPH_SPRITE: GlyphSprite = GlyphSprite {
    glyph: &NULL_GLYPH,
    wide: 8u8,
    high: 12u8,
    kern: 1,
    ch: '\u{0}',
    invert: false,
    insert: false,
    double: false,
    large: false,
};

/// Unicode replacement character
pub const REPLACEMENT: char = '\u{FFFD}';

/// Blit a glyph with XOR at point; caller is responsible for word wrap.
///
/// Examples of word alignment for destination frame buffer:
/// 1. Fits in word: xr:1..7   => (data[0].bit_30)->(data[0].bit_26), mask:0x7c00_0000
/// 2. Spans words:  xr:30..36 => (data[0].bit_01)->(data[1].bit_29), mask:[0x0000_0003,0xe000_000]
///
pub fn xor_glyph(fb: &mut FrBuf, p: &Point, gs: GlyphSprite, xor: bool, cr: ClipRect) {
    const SPRITE_PX: i16 = 16;
    const SPRITE_WORDS: i16 = 8;
    if gs.glyph.len() < SPRITE_WORDS as usize {
        // Fail silently if the glyph slice was too small
        // TODO: Maybe return an error? Not sure which way is better.
        return;
    }
    let high = gs.high as i16;
    let wide = gs.wide as i16;
    if high > SPRITE_PX || wide > SPRITE_PX {
        // Fail silently if glyph height or width is out of spec
        // TODO: Maybe return an error?
        return;
    }
    // Calculate word alignment for destination buffer
    let x0 = p.x;
    if x0 >= cr.max.x as i16 { log::trace!("out the right"); return } // out the right hand side
    let x1 = p.x + wide - 1;
    if x1 < cr.min.x as i16 { log::trace!("out the left"); return } // out the left hand side
    let dest_low_word = x0 >> 5;
    let dest_high_word = x1 >> 5;
    let px_in_dest_low_word = 32 - (x0 & 0x1f);
    // Blit it (use glyph height to avoid blitting empty rows)
    let mut row_base = p.y * WORDS_PER_LINE as i16;
    let row_upper_limit = cr.max.y as i16 * WORDS_PER_LINE as i16;
    let row_lower_limit = cr.min.y as i16 * WORDS_PER_LINE as i16;
    let glyph = gs.glyph;
    for y in 0..high as usize {
        if row_base >= row_upper_limit {
            log::trace!("off the bottom");
            // Clip anything that would run off the end of the frame buffer
            break;
        }
        if row_base >= row_lower_limit {
            // Unpack pixels for this glyph row.
            // CAUTION: some math magic happening here...
            //  when y==0, this does (glyph[0] >>  0) & mask,
            //  when y==1, this does (glyph[0] >> 16) & mask,
            //  when y==2, this does (glyph[1] >>  0) & mask,
            //  ...
            let mask = 0x0000ffff as u32;
            let shift = (y as u32 & 1) << 4;
            let pattern = (glyph[y >> 1] >> shift) & mask;

            // compute partial masks to prevent glyphs from "spilling over" the clip rectangle
            let mut partial_mask_lo = 0xffff_ffff;
            let mut partial_mask_hi = 0xffff_ffff;
            if x0 < cr.min.x as i16 || x1 >= cr.max.x as i16 {
                let x0a = if x0 < cr.min.x as i16 { cr.min.x as i16 } else { x0 };
                let x1a = if x1 >= cr.max.x as i16 { cr.max.x as i16 } else { x1 };
                let mut ones = (1u64 << ((x1a - x0a) as u64 + 1)) - 1;
                ones <<= x0a as u64 & 0x1f;
                partial_mask_lo = ones as u32;
                partial_mask_hi = (ones >> 32) as u32;
            }
            // XOR glyph pixels onto destination buffer. Note that despite the masks above, we will not render
            // partial glyphs that cross the absolute bounds of the left and right edge of the screen.
            if x0 >= 0 && x1 < WIDTH as i16 {
                if xor {
                    fb[(row_base + dest_low_word) as usize] ^= (pattern << (32 - px_in_dest_low_word)) & partial_mask_lo;
                } else {
                    fb[(row_base + dest_low_word) as usize] &= 0xffff_ffff ^ ((pattern << (32 - px_in_dest_low_word)) & partial_mask_lo);
                }
                if wide > px_in_dest_low_word {
                    if xor {
                        fb[(row_base + dest_high_word) as usize] ^= (pattern >> px_in_dest_low_word) & partial_mask_hi;
                    } else {
                        fb[(row_base + dest_high_word) as usize] &= 0xffff_ffff ^ ((pattern >> px_in_dest_low_word) & partial_mask_hi);
                    }
                }
            } else {
                log::trace!("absolute x/y fail");
            }
            fb[(row_base as usize + WORDS_PER_LINE - 1) as usize] |= 0x1_0000; // set the dirty bit on the line
        } else {
            log::trace!("off the top");
        }

        // Advance destination offset using + instead of * to maybe save some CPU cycles
        row_base += WORDS_PER_LINE as i16;
    }
}

/// Blit a glyph that is based off of 32x sprites.
///
pub fn xor_glyph_large(fb: &mut FrBuf, p: &Point, gs: GlyphSprite, xor: bool, cr: ClipRect) {
    const SPRITE_PX: i16 = 32;
    const SPRITE_WORDS: i16 = 8;
    if gs.glyph.len() < SPRITE_WORDS as usize {
        // Fail silently if the glyph slice was too small
        // TODO: Maybe return an error? Not sure which way is better.
        log::info!("len err: {}", gs.glyph.len());
        return;
    }
    let high = gs.high as i16 / 2;
    let wide = (gs.wide as i16 / 2).max(1);
    if high > SPRITE_PX || wide > SPRITE_PX {
        // Fail silently if glyph height or width is out of spec
        // TODO: Maybe return an error?
        log::info!("high/wide err");
        return;
    }
    // Calculate word alignment for destination buffer
    let x0 = p.x;
    if x0 >= cr.max.x as i16 { log::trace!("out the right"); return } // out the right hand side
    let x1 = p.x + (wide << 1) - 1;
    if x1 < cr.min.x as i16 { log::trace!("out the left"); return } // out the left hand side
    let dest_low_word = x0 >> 5;
    let mut dest_high_word = x1 >> 5;
    // fixup case where a glyph is very narrow and and gs.wide / 2 is rounded down to 0.
    // this happens on things like '.' and '1'.
    if dest_high_word == dest_low_word {
        dest_high_word += 1;
    }
    let px_in_dest_low_word = 32 - (x0 & 0x1f);
    // Blit it (use glyph height to avoid blitting empty rows)
    let mut row_base = p.y * WORDS_PER_LINE as i16;
    let row_upper_limit = cr.max.y as i16 * WORDS_PER_LINE as i16;
    let row_lower_limit = cr.min.y as i16 * WORDS_PER_LINE as i16;
    let glyph = gs.glyph;
    // Blit the large glyph
    for src in glyph {
        if row_base >= row_upper_limit {
            // Clip anything that would run off the end of the frame buffer
            break;
        }
        if row_base >= row_lower_limit {
            // compute partial masks to prevent glyphs from "spilling over" the clip rectangle
            let mut partial_mask_lo = 0xffff_ffff;
            let mut partial_mask_hi = 0xffff_ffff;
            let x1_2x = p.x + ((gs.wide as i16) << 1) - 1;
            if x0 < cr.min.x as i16 || x1_2x >= cr.max.x as i16 {
                let x0a = if x0 < cr.min.x as i16 { cr.min.x as i16 } else { x0 };
                let x1a = if x1_2x >= cr.max.x as i16 { cr.max.x as i16 } else { x1_2x };
                let mut ones = (1u64 << ((x1a - x0a) as u64 + 1)) - 1;
                ones <<= x0a as u64 & 0x1f;
                partial_mask_lo = ones as u32;
                partial_mask_hi = (ones >> 32) as u32;
            }

            // XOR glyph pixels onto destination buffer
            if x0 >= 0 && x1_2x < WIDTH as i16 {
                if xor {
                    fb[(row_base + dest_low_word) as usize] ^= (src << (32 - px_in_dest_low_word)) & partial_mask_lo;
                } else {
                    fb[(row_base + dest_low_word) as usize] &= 0xffff_ffff ^ ((src << (32 - px_in_dest_low_word)) & partial_mask_lo);
                }
                if (wide << 1) >= px_in_dest_low_word {
                    if xor {
                        fb[(row_base + dest_high_word) as usize] ^= (src >> px_in_dest_low_word) & partial_mask_hi;
                    } else {
                        fb[(row_base + dest_high_word) as usize] &= 0xffff_ffff ^ ((src >> px_in_dest_low_word) & partial_mask_hi);
                    }
                }
            }
        }
        // Advance destination offset using + instead of * to maybe save some CPU cycles
        row_base += WORDS_PER_LINE as i16;
    }
}

/// Blit a 2x scaled glyph with XOR at point; caller is responsible for word wrap.
///
/// This is similar to xor_glyph(). But, instead of using 16px sprites for input
/// and output, this takes 16px sprites as input and blits 32px sprites as output.
///
pub fn xor_glyph_2x(fb: &mut FrBuf, p: &Point, gs: GlyphSprite, xor: bool, cr: ClipRect) {
    const SPRITE_PX: i16 = 16;
    const SPRITE_WORDS: i16 = 8;
    if gs.glyph.len() < SPRITE_WORDS as usize {
        // Fail silently if the glyph slice was too small
        // TODO: Maybe return an error? Not sure which way is better.
        return;
    }
    let high = gs.high as i16 / 2;
    let wide = (gs.wide as i16 / 2).max(1);
    if high > SPRITE_PX || wide > SPRITE_PX {
        // Fail silently if glyph height or width is out of spec
        // TODO: Maybe return an error?
        return;
    }
    // Calculate word alignment for destination buffer
    let x0 = p.x;
    if x0 >= cr.max.x as i16 { log::trace!("out the right"); return } // out the right hand side
    let x1 = p.x + (wide << 1) - 1;
    if x1 < cr.min.x as i16 { log::trace!("out the left"); return } // out the left hand side
    let dest_low_word = x0 >> 5;
    let mut dest_high_word = x1 >> 5;
    if dest_high_word == dest_low_word {
        dest_high_word += 1;
    }
    let px_in_dest_low_word = 32 - (x0 & 0x1f);
    // Blit it (use glyph height to avoid blitting empty rows)
    let mut row_base = p.y * WORDS_PER_LINE as i16;
    let row_upper_limit = cr.max.y as i16 * WORDS_PER_LINE as i16;
    let row_lower_limit = cr.min.y as i16 * WORDS_PER_LINE as i16;
    let glyph = gs.glyph;
    // Scale up 2x
    let mut glyph_2x = [0u32; 32];
    let mask = 0x0000ffff as u32;
    for y in 0..high as usize {
        let shift = (y as u32 & 1) << 4;
        let pattern = (glyph[y >> 1] >> shift) & mask;
        let low_2x = LUT_2X[(pattern & 0xff) as usize] as u32;
        let high_2x = LUT_2X[((pattern >> 8) & 0xff) as usize] as u32;
        let both_2x = low_2x | (high_2x << 16);
        let index_2x = y << 1;
        glyph_2x[index_2x] = both_2x;
        glyph_2x[index_2x + 1] = both_2x;
    }
    // Blit the scaled up glyph
    for src in glyph_2x {
        if row_base >= row_upper_limit {
            // Clip anything that would run off the end of the frame buffer
            break;
        }
        if row_base >= row_lower_limit {
            // compute partial masks to prevent glyphs from "spilling over" the clip rectangle
            let mut partial_mask_lo = 0xffff_ffff;
            let mut partial_mask_hi = 0xffff_ffff;
            let x1_2x = p.x + ((gs.wide as i16) << 1) - 1;
            if x0 < cr.min.x as i16 || x1_2x >= cr.max.x as i16 {
                let x0a = if x0 < cr.min.x as i16 { cr.min.x as i16 } else { x0 };
                let x1a = if x1_2x >= cr.max.x as i16 { cr.max.x as i16 } else { x1_2x };
                let mut ones = (1u64 << ((x1a - x0a) as u64 + 1)) - 1;
                ones <<= x0a as u64 & 0x1f;
                partial_mask_lo = ones as u32;
                partial_mask_hi = (ones >> 32) as u32;
            }

            // XOR glyph pixels onto destination buffer
            if x0 >= 0 && x1_2x < WIDTH as i16 {
                if xor {
                    fb[(row_base + dest_low_word) as usize] ^= (src << (32 - px_in_dest_low_word)) & partial_mask_lo;
                } else {
                    fb[(row_base + dest_low_word) as usize] &= 0xffff_ffff ^ ((src << (32 - px_in_dest_low_word)) & partial_mask_lo);
                }
                if (wide << 1) >= px_in_dest_low_word {
                    if xor {
                        fb[(row_base + dest_high_word) as usize] ^= (src >> px_in_dest_low_word) & partial_mask_hi;
                    } else {
                        fb[(row_base + dest_high_word) as usize] &= 0xffff_ffff ^ ((src >> px_in_dest_low_word) & partial_mask_hi);
                    }
                }
            }
        }
        // Advance destination offset using + instead of * to maybe save some CPU cycles
        row_base += WORDS_PER_LINE as i16;
    }
}

/// Lookup table to speed up 2x scaling by expanding u8 index to u16 value
pub const LUT_2X: [u16; 256] = [
    0b0000000000000000,
    0b0000000000000011,
    0b0000000000001100,
    0b0000000000001111,
    0b0000000000110000,
    0b0000000000110011,
    0b0000000000111100,
    0b0000000000111111,
    0b0000000011000000,
    0b0000000011000011,
    0b0000000011001100,
    0b0000000011001111,
    0b0000000011110000,
    0b0000000011110011,
    0b0000000011111100,
    0b0000000011111111,
    0b0000001100000000,
    0b0000001100000011,
    0b0000001100001100,
    0b0000001100001111,
    0b0000001100110000,
    0b0000001100110011,
    0b0000001100111100,
    0b0000001100111111,
    0b0000001111000000,
    0b0000001111000011,
    0b0000001111001100,
    0b0000001111001111,
    0b0000001111110000,
    0b0000001111110011,
    0b0000001111111100,
    0b0000001111111111,
    0b0000110000000000,
    0b0000110000000011,
    0b0000110000001100,
    0b0000110000001111,
    0b0000110000110000,
    0b0000110000110011,
    0b0000110000111100,
    0b0000110000111111,
    0b0000110011000000,
    0b0000110011000011,
    0b0000110011001100,
    0b0000110011001111,
    0b0000110011110000,
    0b0000110011110011,
    0b0000110011111100,
    0b0000110011111111,
    0b0000111100000000,
    0b0000111100000011,
    0b0000111100001100,
    0b0000111100001111,
    0b0000111100110000,
    0b0000111100110011,
    0b0000111100111100,
    0b0000111100111111,
    0b0000111111000000,
    0b0000111111000011,
    0b0000111111001100,
    0b0000111111001111,
    0b0000111111110000,
    0b0000111111110011,
    0b0000111111111100,
    0b0000111111111111,
    0b0011000000000000,
    0b0011000000000011,
    0b0011000000001100,
    0b0011000000001111,
    0b0011000000110000,
    0b0011000000110011,
    0b0011000000111100,
    0b0011000000111111,
    0b0011000011000000,
    0b0011000011000011,
    0b0011000011001100,
    0b0011000011001111,
    0b0011000011110000,
    0b0011000011110011,
    0b0011000011111100,
    0b0011000011111111,
    0b0011001100000000,
    0b0011001100000011,
    0b0011001100001100,
    0b0011001100001111,
    0b0011001100110000,
    0b0011001100110011,
    0b0011001100111100,
    0b0011001100111111,
    0b0011001111000000,
    0b0011001111000011,
    0b0011001111001100,
    0b0011001111001111,
    0b0011001111110000,
    0b0011001111110011,
    0b0011001111111100,
    0b0011001111111111,
    0b0011110000000000,
    0b0011110000000011,
    0b0011110000001100,
    0b0011110000001111,
    0b0011110000110000,
    0b0011110000110011,
    0b0011110000111100,
    0b0011110000111111,
    0b0011110011000000,
    0b0011110011000011,
    0b0011110011001100,
    0b0011110011001111,
    0b0011110011110000,
    0b0011110011110011,
    0b0011110011111100,
    0b0011110011111111,
    0b0011111100000000,
    0b0011111100000011,
    0b0011111100001100,
    0b0011111100001111,
    0b0011111100110000,
    0b0011111100110011,
    0b0011111100111100,
    0b0011111100111111,
    0b0011111111000000,
    0b0011111111000011,
    0b0011111111001100,
    0b0011111111001111,
    0b0011111111110000,
    0b0011111111110011,
    0b0011111111111100,
    0b0011111111111111,
    0b1100000000000000,
    0b1100000000000011,
    0b1100000000001100,
    0b1100000000001111,
    0b1100000000110000,
    0b1100000000110011,
    0b1100000000111100,
    0b1100000000111111,
    0b1100000011000000,
    0b1100000011000011,
    0b1100000011001100,
    0b1100000011001111,
    0b1100000011110000,
    0b1100000011110011,
    0b1100000011111100,
    0b1100000011111111,
    0b1100001100000000,
    0b1100001100000011,
    0b1100001100001100,
    0b1100001100001111,
    0b1100001100110000,
    0b1100001100110011,
    0b1100001100111100,
    0b1100001100111111,
    0b1100001111000000,
    0b1100001111000011,
    0b1100001111001100,
    0b1100001111001111,
    0b1100001111110000,
    0b1100001111110011,
    0b1100001111111100,
    0b1100001111111111,
    0b1100110000000000,
    0b1100110000000011,
    0b1100110000001100,
    0b1100110000001111,
    0b1100110000110000,
    0b1100110000110011,
    0b1100110000111100,
    0b1100110000111111,
    0b1100110011000000,
    0b1100110011000011,
    0b1100110011001100,
    0b1100110011001111,
    0b1100110011110000,
    0b1100110011110011,
    0b1100110011111100,
    0b1100110011111111,
    0b1100111100000000,
    0b1100111100000011,
    0b1100111100001100,
    0b1100111100001111,
    0b1100111100110000,
    0b1100111100110011,
    0b1100111100111100,
    0b1100111100111111,
    0b1100111111000000,
    0b1100111111000011,
    0b1100111111001100,
    0b1100111111001111,
    0b1100111111110000,
    0b1100111111110011,
    0b1100111111111100,
    0b1100111111111111,
    0b1111000000000000,
    0b1111000000000011,
    0b1111000000001100,
    0b1111000000001111,
    0b1111000000110000,
    0b1111000000110011,
    0b1111000000111100,
    0b1111000000111111,
    0b1111000011000000,
    0b1111000011000011,
    0b1111000011001100,
    0b1111000011001111,
    0b1111000011110000,
    0b1111000011110011,
    0b1111000011111100,
    0b1111000011111111,
    0b1111001100000000,
    0b1111001100000011,
    0b1111001100001100,
    0b1111001100001111,
    0b1111001100110000,
    0b1111001100110011,
    0b1111001100111100,
    0b1111001100111111,
    0b1111001111000000,
    0b1111001111000011,
    0b1111001111001100,
    0b1111001111001111,
    0b1111001111110000,
    0b1111001111110011,
    0b1111001111111100,
    0b1111001111111111,
    0b1111110000000000,
    0b1111110000000011,
    0b1111110000001100,
    0b1111110000001111,
    0b1111110000110000,
    0b1111110000110011,
    0b1111110000111100,
    0b1111110000111111,
    0b1111110011000000,
    0b1111110011000011,
    0b1111110011001100,
    0b1111110011001111,
    0b1111110011110000,
    0b1111110011110011,
    0b1111110011111100,
    0b1111110011111111,
    0b1111111100000000,
    0b1111111100000011,
    0b1111111100001100,
    0b1111111100001111,
    0b1111111100110000,
    0b1111111100110011,
    0b1111111100111100,
    0b1111111100111111,
    0b1111111111000000,
    0b1111111111000011,
    0b1111111111001100,
    0b1111111111001111,
    0b1111111111110000,
    0b1111111111110011,
    0b1111111111111100,
    0b1111111111111111,
];
