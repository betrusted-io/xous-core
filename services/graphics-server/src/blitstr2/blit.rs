// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
use super::cliprect::ClipRect;
use super::cursor::Cursor;
use super::fonts;
use super::pt::Pt;
use super::{LINES, WORDS_PER_LINE, WIDTH, FrBuf};

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

/// Clear a screen region bounded by (clip.min.x,clip.min.y)..(clip.min.x,clip.max.y)
pub fn clear_region(fb: &mut FrBuf, clip: ClipRect) {
    if clip.max.y > LINES
        || clip.min.y >= clip.max.y
        || clip.max.x > WIDTH
        || clip.min.x >= clip.max.x
    {
        return;
    }
    // Calculate word alignment for destination buffer
    let dest_low_word = clip.min.x >> 5;
    let dest_high_word = clip.max.x >> 5;
    let px_in_dest_low_word = 32 - (clip.min.x & 0x1f);
    let px_in_dest_high_word = clip.max.x & 0x1f;
    // Blit it
    for y in clip.min.y..clip.max.y {
        let base = y * WORDS_PER_LINE;
        fb[base + dest_low_word] |= 0xffffffff << (32 - px_in_dest_low_word);
        for w in dest_low_word + 1..dest_high_word {
            fb[base + w] = 0xffffffff;
        }
        if dest_low_word < dest_high_word {
            fb[base + dest_high_word] |= 0xffffffff >> (32 - px_in_dest_high_word);
        }
    }
}

/// Find glyph for char using latin regular, emoji, ja, zh, and kr font data
pub fn find_glyph(ch: char) -> fonts::GlyphSprite {
    match fonts::regular_glyph(ch) {
        Ok(g) => g,
        _ => match fonts::emoji_glyph(ch) {
            Ok(g) => g,
            _ => match fonts::ja_glyph(ch) {
                Ok(g) => g,
                _ => match fonts::zh_glyph(ch) {
                    Ok(g) => g,
                    _ => match fonts::kr_glyph(ch) {
                        Ok(g) => g,
                        _ => match fonts::regular_glyph(REPLACEMENT) {
                            Ok(g) => g,
                            _ => NULL_GLYPH_SPRITE,
                        },
                    },
                },
            },
        },
    }
}

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

/// Find glyph for char using only the latin bold font data
pub fn find_glyph_latin_bold(ch: char) -> fonts::GlyphSprite {
    match fonts::bold_glyph(ch) {
        Ok(g) => g,
        _ => match fonts::bold_glyph(REPLACEMENT) {
            Ok(g) => g,
            _ => NULL_GLYPH_SPRITE,
        },
    }
}

/// Find glyph for char using only the latin mono font data
pub fn find_glyph_latin_mono(ch: char) -> fonts::GlyphSprite {
    match fonts::mono_glyph(ch) {
        Ok(g) => g,
        _ => match fonts::mono_glyph(REPLACEMENT) {
            Ok(g) => g,
            _ => NULL_GLYPH_SPRITE,
        },
    }
}

/// XOR blit a string using multi-lingual glyphs with specified clip rect, starting at cursor
pub fn paint_str(fb: &mut FrBuf, clip: ClipRect, c: &mut Cursor, s: &str) {
    const KERN: usize = 2;
    for ch in s.chars() {
        if ch == '\n' {
            newline(clip, c);
        } else {
            // Look up the glyph for this char
            let glyph = find_glyph(ch);
            let wide = glyph.wide as usize;
            let high = glyph.high as usize;
            // Adjust for word wrapping
            if c.pt.x + wide + KERN >= clip.max.x {
                newline(clip, c);
            }
            // Blit the glyph and advance the cursor
            xor_glyph(fb, &c.pt, glyph, false);
            c.pt.x += wide + KERN;
            if high > c.line_height {
                c.line_height = high;
            }
        }
    }
}

/// XOR blit a string using latin small glyphs with specified clip rect, starting at cursor
pub fn paint_str_latin_small(fb: &mut FrBuf, clip: ClipRect, c: &mut Cursor, s: &str) {
    const KERN: usize = 1;
    for ch in s.chars() {
        if ch == '\n' {
            newline(clip, c);
        } else {
            // Look up the glyph for this char
            let glyph = find_glyph_latin_small(ch);
            let wide = glyph.wide as usize;
            let high = glyph.high as usize;
            // Adjust for word wrapping
            if c.pt.x + wide + KERN >= clip.max.x {
                newline(clip, c);
            }
            // Blit the glyph and advance the cursor
            xor_glyph(fb, &c.pt, glyph, false);
            c.pt.x += wide + KERN;
            if high > c.line_height {
                c.line_height = high;
            }
        }
    }
}

/// XOR blit a string using latin bold glyphs with specified clip rect, starting at cursor
pub fn paint_str_latin_bold(fb: &mut FrBuf, clip: ClipRect, c: &mut Cursor, s: &str) {
    const KERN: usize = 2;
    for ch in s.chars() {
        if ch == '\n' {
            newline(clip, c);
        } else {
            // Look up the glyph for this char
            let glyph = find_glyph_latin_bold(ch);
            let wide = glyph.wide as usize;
            let high = glyph.high as usize;
            // Adjust for word wrapping
            if c.pt.x + wide + KERN >= clip.max.x {
                newline(clip, c);
            }
            // Blit the glyph and advance the cursor
            xor_glyph(fb, &c.pt, glyph, false);
            c.pt.x += wide + KERN;
            if high > c.line_height {
                c.line_height = high;
            }
        }
    }
}

/// XOR blit a string using latin mono glyphs with specified clip rect, starting at cursor
pub fn paint_str_latin_mono(fb: &mut FrBuf, clip: ClipRect, c: &mut Cursor, s: &str) {
    const KERN: usize = 1;
    for ch in s.chars() {
        if ch == '\n' {
            newline(clip, c);
        } else {
            // Look up the glyph for this char
            let glyph = find_glyph_latin_mono(ch);
            let wide = glyph.wide as usize;
            let high = glyph.high as usize;
            // Adjust for word wrapping
            if c.pt.x + wide + KERN >= clip.max.x {
                newline(clip, c);
            }
            // Blit the glyph and advance the cursor
            xor_glyph(fb, &c.pt, glyph, false);
            c.pt.x += wide + KERN;
            if high > c.line_height {
                c.line_height = high;
            }
        }
    }
}

/// Advance the cursor to the start of a new line within the clip rect
pub fn newline(clip: ClipRect, c: &mut Cursor) {
    c.pt.x = clip.min.x;
    if c.line_height < fonts::small::MAX_HEIGHT as usize {
        c.line_height = fonts::small::MAX_HEIGHT as usize;
    }
    c.pt.y += c.line_height + 2;
    c.line_height = 0;
}

/// Blit a glyph with XOR at point; caller is responsible for word wrap.
///
/// Examples of word alignment for destination frame buffer:
/// 1. Fits in word: xr:1..7   => (data[0].bit_30)->(data[0].bit_26), mask:0x7c00_0000
/// 2. Spans words:  xr:30..36 => (data[0].bit_01)->(data[1].bit_29), mask:[0x0000_0003,0xe000_000]
///
pub fn xor_glyph(fb: &mut FrBuf, p: &Pt, gs: fonts::GlyphSprite, xor: bool) {
    const SPRITE_PX: usize = 16;
    const SPRITE_WORDS: usize = 8;
    if gs.glyph.len() < SPRITE_WORDS {
        // Fail silently if the glyph slice was too small
        // TODO: Maybe return an error? Not sure which way is better.
        return;
    }
    let high = gs.high as usize;
    let wide = gs.wide as usize;
    if high > SPRITE_PX || wide > SPRITE_PX {
        // Fail silently if glyph height or width is out of spec
        // TODO: Maybe return an error?
        return;
    }
    // Calculate word alignment for destination buffer
    let x0 = p.x;
    let x1 = p.x + wide - 1;
    let dest_low_word = x0 >> 5;
    let dest_high_word = x1 >> 5;
    let px_in_dest_low_word = 32 - (x0 & 0x1f);
    // Blit it (use glyph height to avoid blitting empty rows)
    let mut row_base = p.y * WORDS_PER_LINE;
    const ROW_LIMIT: usize = LINES * WORDS_PER_LINE;
    let glyph = gs.glyph;
    for y in 0..high {
        if row_base >= ROW_LIMIT {
            // Clip anything that would run off the end of the frame buffer
            break;
        }
        // Unpack pixels for this glyph row.
        // CAUTION: some math magic happening here...
        //  when y==0, this does (glyph[0] >>  0) & mask,
        //  when y==1, this does (glyph[0] >> 16) & mask,
        //  when y==2, this does (glyph[1] >>  0) & mask,
        //  ...
        let mask = 0x0000ffff as u32;
        let shift = (y & 1) << 4;
        let pattern = (glyph[y >> 1] >> shift) & mask;
        // XOR glyph pixels onto destination buffer
        if xor {
            fb[row_base + dest_low_word] ^= pattern << (32 - px_in_dest_low_word);
        } else {
            fb[(row_base + dest_low_word) as usize] &= 0xffff_ffff ^ (pattern << (32 - px_in_dest_low_word));
        }
        if wide > px_in_dest_low_word {
            if xor {
                fb[row_base + dest_high_word] ^= pattern >> px_in_dest_low_word;
            } else {
                fb[(row_base + dest_high_word) as usize] &= 0xffff_ffff ^ (pattern >> px_in_dest_low_word);
            }
        }
        fb[(row_base + WORDS_PER_LINE - 1) as usize] |= 0x1_0000; // set the dirty bit on the line

        // Advance destination offset using + instead of * to maybe save some CPU cycles
        row_base += WORDS_PER_LINE;
    }
}
