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
pub mod tall;
pub mod zh;

use crate::minigfx::*;

const DEFAULT_KERN: u8 = 1;

pub fn small_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match small::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= small::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &small::glyphs()[offset..end],
                    wide: small::WIDTHS[n],
                    high: small::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
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
            match end <= regular::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &regular::glyphs()[offset..end],
                    wide: regular::WIDTHS[n],
                    high: regular::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn large_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match small::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= small::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &small::glyphs()[offset..end],
                    wide: small::WIDTHS[n] * 2,
                    high: small::MAX_HEIGHT * 2,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: true,
                    large: false,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn extra_large_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match regular::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= regular::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &regular::glyphs()[offset..end],
                    wide: regular::WIDTHS[n] * 2,
                    high: regular::MAX_HEIGHT * 2,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: true,
                    large: false,
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
            match end <= bold::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &bold::glyphs()[offset..end],
                    wide: bold::WIDTHS[n],
                    high: bold::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
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
            match end <= mono::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &mono::glyphs()[offset..end],
                    wide: mono::WIDTHS[n],
                    high: mono::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn tall_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match tall::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 5;
            let end = offset + 32;
            match end <= tall::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &tall::glyphs()[offset..end],
                    wide: tall::WIDTHS[n],
                    high: tall::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: true,
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
            match end <= emoji::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &emoji::glyphs()[offset..end],
                    wide: emoji::MAX_HEIGHT, // yes, use height for wide
                    high: emoji::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}

pub fn emoji_large_glyph(ch: char) -> Result<GlyphSprite, usize> {
    match emoji::CODEPOINTS.binary_search(&(ch as u32)) {
        Ok(n) => {
            let offset = n << 3;
            let end = offset + 8;
            match end <= emoji::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &emoji::glyphs()[offset..end],
                    wide: emoji::MAX_HEIGHT * 2, // yes, use height for wide
                    high: emoji::MAX_HEIGHT * 2,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: true,
                    large: false,
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
            match end <= zh::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &zh::glyphs()[offset..end],
                    wide: zh::MAX_HEIGHT, // yes, use height for wide
                    high: zh::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
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
            match end <= ja::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &ja::glyphs()[offset..end],
                    wide: ja::MAX_HEIGHT, // yes, use height for wide
                    high: ja::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
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
            match end <= kr::glyphs().len() {
                true => Ok(GlyphSprite {
                    glyph: &kr::glyphs()[offset..end],
                    wide: kr::MAX_HEIGHT, // yes, use height for wide
                    high: kr::MAX_HEIGHT,
                    kern: DEFAULT_KERN,
                    ch,
                    invert: false,
                    insert: false,
                    double: false,
                    large: false,
                }),
                false => Err(0),
            }
        }
        _ => Err(1),
    }
}
