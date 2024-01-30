mod cliprect;
pub use cliprect::*;
mod cursor;
pub use cursor::*;
mod pt;
pub use pt::*;

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

/// A TypesetWord is a Word that has beet turned into sprites and placed at a specific location on the canvas,
/// defined by its `bb` record. The intention is that this abstract representation can be passed directly to
/// a rasterizer for rendering.
#[derive(Debug)]
pub(crate) struct TypesetWord {
    /// glyph data to directly render the word
    pub gs: Vec<GlyphSprite>,
    /// top left origin point for rendering of the glyphs
    pub origin: Pt,
    /// width of the word
    pub width: i16,
    /// overall height for the word
    pub height: i16,
    /// set if this `word` is not drawable, e.g. a newline placeholder.
    /// *however* the Vec<GlyphSprite> should still be checked for an insertion point, so that
    /// successive newlines properly get their insertion point drawn
    pub non_drawable: bool,
    /// the position in the originating abstract string of the first character in the word
    pub strpos: usize,
}
