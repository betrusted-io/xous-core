pub mod line;
pub use line::*;
pub mod point;
pub use point::*;
pub mod rect;
pub use rect::*;
pub mod style;
pub use style::*;
pub mod circle;
pub use circle::*;
pub mod clip;
pub use clip::*;
#[cfg(feature = "ditherpunk")]
pub mod tile;
#[cfg(feature = "ditherpunk")]
pub use tile::*;

#[cfg(feature = "std")]
pub mod textview;
#[cfg(feature = "std")]
pub use textview::*;
#[cfg(feature = "std")]
pub mod cursor;
#[cfg(feature = "std")]
pub use cursor::*;
#[cfg(feature = "std")]
pub mod handlers;
#[cfg(feature = "std")]
pub mod op;

#[cfg(feature = "std")]
use blitstr2::GlyphSprite;

use crate::platform;

/// Abstract trait for a FrameBuffer. Slower than native manipulation
/// of the [u8] contents of a frame buffer, but more portable.
pub trait FrameBuffer {
    /// Puts a pixel of ColorNative at x, y. (0, 0) is defined as the lower left corner.
    fn put_pixel(&mut self, p: Point, color: ColorNative);
    /// Retrieves a pixel value from the frame buffer; returns None if the point is out of bounds.
    fn get_pixel(&mut self, p: Point) -> Option<ColorNative>;
    /// XORs a pixel to what is in the existing frame buffer. The exact definition of "XOR" is somewhat
    /// ambiguous for full color systems but is generally meant to imply a light/dark swap of foreground
    /// and background colors for a color theme.
    fn xor_pixel(&mut self, p: Point);
    /// Swaps the drawable buffer to the screen and sends it to the hardware
    fn draw(&mut self);
    /// Clears the drawable buffer
    fn clear(&mut self);
    /// Returns the size of the frame buffer as a Point
    fn dimensions(&self) -> Point;
    /// Returns a raw pointer to the frame buffer
    unsafe fn raw_mut(&mut self) -> &mut platform::FbRaw;
}

/// A TypesetWord is a Word that has beet turned into sprites and placed at a specific location on the canvas,
/// defined by its `bb` record. The intention is that this abstract representation can be passed directly to
/// a rasterizer for rendering.
#[derive(Debug)]
#[cfg(feature = "std")]
pub struct TypesetWord {
    /// glyph data to directly render the word
    pub gs: Vec<GlyphSprite>,
    /// top left origin point for rendering of the glyphs
    pub origin: Point,
    /// width of the word
    pub width: isize,
    /// overall height for the word
    pub height: isize,
    /// set if this `word` is not drawable, e.g. a newline placeholder.
    /// *however* the `Vec<GlyphSprite>` should still be checked for an insertion point, so that
    /// successive newlines properly get their insertion point drawn
    pub non_drawable: bool,
    /// the position in the originating abstract string of the first character in the word
    pub strpos: usize,
}
