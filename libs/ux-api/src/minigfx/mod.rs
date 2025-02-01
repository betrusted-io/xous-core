pub mod line;
pub use line::*;
pub mod point;
pub use point::*;
pub mod rect;
pub use rect::*;
pub mod style;
pub use style::*;

/// Abstract trait for a FrameBuffer. Slower than native manipulation
/// of the [u8] contents of a frame buffer, but more portable.
pub trait FrameBuffer {
    /// Puts a pixel of ColorNative at x, y. (0, 0) is defined as the lower left corner.
    fn put_pixel(&mut self, p: Point, color: ColorNative);
    /// Retrieves a pixel value from the frame buffer; returns None if the point is out of bounds.
    fn get_pixel(&mut self, p: Point) -> Option<ColorNative>;
    /// Swaps the drawable buffer to the screen and sends it to the hardware
    fn draw(&mut self);
    /// Clears the drawable buffer
    fn clear(&mut self);
    /// Returns the size of the frame buffer as a Point
    fn dimensions(&self) -> Point;
}
