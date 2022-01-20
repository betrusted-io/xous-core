// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
use super::cliprect::ClipRect;
use super::fonts;
#[allow(unused_imports)]
use super::{LINES, WORDS_PER_LINE, WIDTH, FrBuf};
use crate::api::Point;

/// Null glyph to use when everything else fails
pub const NULL_GLYPH: [u32; 8] = [0, 0x5500AA, 0x5500AA, 0x5500AA, 0x5500AA, 0x5500AA, 0, 0];
pub const NULL_GLYPH_SPRITE: fonts::GlyphSprite = fonts::GlyphSprite {
    glyph: &NULL_GLYPH,
    wide: 8u8,
    high: 12u8,
    kern: 1,
    ch: '\u{0}',
    invert: false,
    insert: false,
};

/// Unicode replacement character
pub const REPLACEMENT: char = '\u{FFFD}';

/// Find glyph for char using only the latin small font data
pub fn find_glyph_latin_small(ch: char) -> fonts::GlyphSprite {
    match fonts::small_glyph(ch) {
        Ok(g) => g,
        _ => match fonts::small_glyph(REPLACEMENT) {
            Ok(g) => g,
            _ => NULL_GLYPH_SPRITE,
        },
    }
}

/// Blit a glyph with XOR at point; caller is responsible for word wrap.
///
/// Examples of word alignment for destination frame buffer:
/// 1. Fits in word: xr:1..7   => (data[0].bit_30)->(data[0].bit_26), mask:0x7c00_0000
/// 2. Spans words:  xr:30..36 => (data[0].bit_01)->(data[1].bit_29), mask:[0x0000_0003,0xe000_000]
///
pub fn xor_glyph(fb: &mut FrBuf, p: &Point, gs: fonts::GlyphSprite, xor: bool, cr: ClipRect) {
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
