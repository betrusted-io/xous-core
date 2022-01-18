// Copyright (c) 2022 Sam Blenny
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
#![allow(dead_code)]
pub mod bold;
pub mod emoji;
pub mod ja;
pub mod kr;
pub mod mono;
pub mod regular;
pub mod small;
pub mod zh;

// Font data is stored as CODEPOINTS and GLYPHS arrays. CODEPOINTS holds sorted
// Unicode codepoints for characters included in the font, and GLYPHS holds
// 16*16px sprites (pixels packed in row-major order, LSB of first word is top
// left pixel of sprite). The order of codepoints and glyphs is the same, but,
// each codepoint is one u32 word long while each glyph is eight u32 words
// long. So, to find a glyph we do:
//  1. Binary search CODEPOINTS for the codepoint of interest
//  2. Multiply the codepoint index by 8, yielding an offset into GLYPHS
//  3. Slice 8 u32 words from GLYPHS starting at the offset

/// Struct to hold sprite pixel reference and associated metadata for glyphs
#[derive(Copy, Clone, Debug)]
pub struct GlyphSprite {
    pub glyph: &'static [u32],
    pub wide: u8,
    pub high: u8,
}

pub fn small_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match small::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= small::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &small::GLYPHS[offset..end],
                    wide: small::WIDTHS[n],
                    high: small::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn regular_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match regular::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= regular::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &regular::GLYPHS[offset..end],
                    wide: regular::WIDTHS[n],
                    high: regular::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn bold_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match bold::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= bold::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &bold::GLYPHS[offset..end],
                    wide: bold::WIDTHS[n],
                    high: bold::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn mono_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match mono::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= mono::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &mono::GLYPHS[offset..end],
                    wide: mono::WIDTHS[n],
                    high: mono::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn emoji_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match emoji::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= emoji::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &emoji::GLYPHS[offset..end],
                    wide: emoji::MAX_HEIGHT, // yes, use height for wide
                    high: emoji::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn zh_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match zh::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= zh::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &zh::GLYPHS[offset..end],
                    wide: zh::MAX_HEIGHT, // yes, use height for wide
                    high: zh::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn ja_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match ja::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= ja::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &ja::GLYPHS[offset..end],
                    wide: ja::MAX_HEIGHT, // yes, use height for wide
                    high: ja::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn kr_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match kr::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= kr::GLYPHS.len() {
                true => Ok(GlyphSprite {
                    glyph: &kr::GLYPHS[offset..end],
                    wide: kr::MAX_HEIGHT, // yes, use height for wide
                    high: kr::MAX_HEIGHT,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}
