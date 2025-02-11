mod blit;
pub use blit::*;
pub(crate) mod fonts;
pub(crate) use fonts::*;

pub type FrBuf =
    [u32; (crate::platform::LINES * crate::platform::WIDTH) as usize / core::mem::size_of::<u32>()];

// add more fonts (an example):
// https://github.com/samblenny/blitstr2/commit/bb7d4ab6a2d8913dcb520895a3c242c933413aae

use crate::minigfx::*;
pub fn glyph_height_hint(glyph: GlyphStyle) -> usize { glyph_to_height_hint(glyph) }
