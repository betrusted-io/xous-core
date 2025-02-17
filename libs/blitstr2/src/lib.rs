mod blit;
pub use blit::*;
mod cliprect;
pub use cliprect::*;
pub mod fonts;
pub use fonts::*;
pub mod fontmap;
pub mod platform;
pub use platform::*;
pub mod glyphstyle;
pub use glyphstyle::*;
mod pt;
pub mod style_macros;

// add more fonts (an example):
// https://github.com/samblenny/blitstr2/commit/bb7d4ab6a2d8913dcb520895a3c242c933413aae

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
    pub kern: u8,
    // the original character
    pub ch: char,
    // invert rendering for the character - for copy/paste selection regions
    pub invert: bool,
    // drawn an insertion point after this character
    pub insert: bool,
    // 2x flag for the back-end rendering (wide/high should be pre-computed to match this)
    pub double: bool,
    // flag for 32-bit wide glyph sets
    pub large: bool,
}

/// Estimate line-height for Latin script text in the given style
/// These are hard-coded in because we want to keep the rest of the font data
/// structures private to this crate. Moving the font files out of their
/// current location would also require modifying a bunch of codegen infrastruture,
/// so, this is one spot where we have to manually maintain a link.
pub fn glyph_height_hint(g: GlyphStyle) -> usize {
    match g {
        GlyphStyle::Small => 12,      // crate::blitstr2::fonts::small::MAX_HEIGHT as usize,
        GlyphStyle::Regular => 15,    // crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Bold => 15,       // crate::blitstr2::fonts::regular::MAX_HEIGHT as usize,
        GlyphStyle::Monospace => 15,  // crate::blitstr2::fonts::mono::MAX_HEIGHT as usize,
        GlyphStyle::Cjk => 16,        // crate::blistr2::fonts::emoji::MAX_HEIGHT as usize,
        GlyphStyle::Large => 24,      // 2x of small
        GlyphStyle::ExtraLarge => 30, // 2x of regular
        GlyphStyle::Tall => 19,
    }
}
