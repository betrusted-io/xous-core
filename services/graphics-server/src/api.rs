use crate::op::PixelColor;
use xous::{Message, ScalarMessage};
use core::ops::{Add, AddAssign, Index, Neg, Sub, SubAssign};

/// 2D size.
///
/// `Size` is used to define the width and height of an object.
///
/// [Nalgebra] support can be enabled with the `nalgebra_support` feature. This implements
/// `From<Vector2<N>>` and `From<&Vector2<N>>` where `N` is `Scalar + Into<u16>`. This allows use
/// of Nalgebra's [`Vector2`] with embedded-graphics where `u16`, `u16` or `u8` is used for value
/// storage.
///
/// # Examples
///
/// ## Create a `Size` from two integers
///
///
/// ```rust
/// use embedded_graphics::geometry::Size;
///
/// // Create a size using the `new` constructor method
/// let s = Size::new(10, 20);
/// ```
///
/// ## Create a `Size` from a Nalgebra `Vector2`
///
/// _Be sure to enable the `nalgebra_support` feature to get [Nalgebra] integration._
///
/// Any `Vector2<N>` can be used where `N: Into<u16> + nalgebra::Scalar`. This includes the primitive types `u16`, `u16` and `u8`.
///
/// ```rust
/// # #[cfg(feature = "nalgebra_support")] {
/// use nalgebra::Vector2;
/// use embedded_graphics::geometry::Size;
///
/// assert_eq!(Size::from(Vector2::new(10u16, 20)), Size::new(10u16, 20));
/// assert_eq!(Size::from(Vector2::new(10u16, 20)), Size::new(10u16, 20));
/// assert_eq!(Size::from(Vector2::new(10u8, 20)), Size::new(10u16, 20));
/// # }
/// ```
///
/// `.into()` can also be used, but may require more type annotations:
///
/// ```rust
/// # #[cfg(feature = "nalgebra_support")] {
/// use nalgebra::Vector2;
/// use embedded_graphics::geometry::Size;
///
/// let c: Size = Vector2::new(10u16, 20).into();
///
/// assert_eq!(c, Size::new(10u16, 20));
/// # }
/// ```
///
/// [`Drawable`]: ../drawable/trait.Drawable.html
/// [`Point`]: struct.Point.html
/// [`Vector2<N>`]: https://docs.rs/nalgebra/0.18.0/nalgebra/base/type.Vector2.html
/// [`Vector2`]: https://docs.rs/nalgebra/0.18.0/nalgebra/base/type.Vector2.html
/// [Nalgebra]: https://docs.rs/nalgebra
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct Size {
    /// The width.
    pub width: u16,

    /// The height.
    pub height: u16,
}

impl Size {
    /// Creates a size from a width and a height.
    pub const fn new(width: u16, height: u16) -> Self {
        Size { width, height }
    }

    /// Creates a size with width and height equal to zero.
    pub const fn zero() -> Self {
        Size {
            width: 0,
            height: 0,
        }
    }

    /// Creates a size from two corner points of a bounding box.
    pub(crate) fn from_bounding_box(corner_1: Point, corner_2: Point) -> Self {
        let width = (corner_1.x - corner_2.x).abs() as u16;
        let height = (corner_1.y - corner_2.y).abs() as u16;

        Self { width, height }
    }
}

impl Add for Size {
    type Output = Size;

    fn add(self, other: Size) -> Size {
        Size::new(self.width + other.width, self.height + other.height)
    }
}

impl AddAssign for Size {
    fn add_assign(&mut self, other: Size) {
        self.width += other.width;
        self.height += other.height;
    }
}

impl Sub for Size {
    type Output = Size;

    fn sub(self, other: Size) -> Size {
        Size::new(self.width - other.width, self.height - other.height)
    }
}

impl SubAssign for Size {
    fn sub_assign(&mut self, other: Size) {
        self.width -= other.width;
        self.height -= other.height;
    }
}

impl Index<usize> for Size {
    type Output = u16;

    fn index(&self, idx: usize) -> &u16 {
        match idx {
            0 => &self.width,
            1 => &self.height,
            _ => panic!("index out of bounds: the len is 2 but the index is {}", idx),
        }
    }
}

impl From<(u16, u16)> for Size {
    fn from(other: (u16, u16)) -> Self {
        Size::new(other.0, other.1)
    }
}

impl From<[u16; 2]> for Size {
    fn from(other: [u16; 2]) -> Self {
        Size::new(other[0], other[1])
    }
}

impl From<&[u16; 2]> for Size {
    fn from(other: &[u16; 2]) -> Self {
        Size::new(other[0], other[1])
    }
}

impl From<Size> for (u16, u16) {
    fn from(other: Size) -> (u16, u16) {
        (other.width, other.height)
    }
}

impl From<&Size> for (u16, u16) {
    fn from(other: &Size) -> (u16, u16) {
        (other.width, other.height)
    }
}

#[cfg(feature = "nalgebra_support")]
use nalgebra::{base::Scalar, Vector2};

#[cfg(feature = "nalgebra_support")]
impl<N> From<Vector2<N>> for Size
where
    N: Into<u16> + Scalar,
{
    fn from(other: Vector2<N>) -> Self {
        Self::new(other[0].into(), other[1].into())
    }
}

#[cfg(feature = "nalgebra_support")]
impl<N> From<&Vector2<N>> for Size
where
    N: Into<u16> + Scalar,
{
    fn from(other: &Vector2<N>) -> Self {
        Self::new(other[0].into(), other[1].into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_can_be_added() {
        let left = Size::new(10, 20);
        let right = Size::new(30, 40);

        assert_eq!(left + right, Size::new(40, 60));
    }

    #[test]
    fn sizes_can_be_subtracted() {
        let left = Size::new(30, 40);
        let right = Size::new(10, 20);

        assert_eq!(left - right, Size::new(20, 20));
    }

    #[test]
    fn from_tuple() {
        assert_eq!(Size::from((20, 30)), Size::new(20, 30));
    }

    #[test]
    fn from_array() {
        assert_eq!(Size::from([20, 30]), Size::new(20, 30));
    }

    #[test]
    fn from_array_ref() {
        assert_eq!(Size::from(&[20, 30]), Size::new(20, 30));
    }

    #[test]
    fn index() {
        let size = Size::new(1, 2);

        assert_eq!(size.width, size[0]);
        assert_eq!(size.height, size[1]);
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds() {
        let size = Size::new(1, 2);
        let _ = size[2];
    }

    #[test]
    #[cfg(feature = "nalgebra_support")]
    fn nalgebra_support() {
        let left = nalgebra::Vector2::new(30u16, 40);
        let right = nalgebra::Vector2::new(10, 20);

        assert_eq!(Size::from(left - right), Size::new(20, 20));
    }
}



/// Style properties for an object
#[derive(Debug, Copy, Clone)]
pub struct Style {
    /// Fill colour of the object
    ///
    /// For fonts, this is the background colour of the text
    pub fill_color: Option<PixelColor>,

    /// Stroke (border/line) color of the object
    ///
    /// For fonts, this is the foreground colour of the text
    pub stroke_color: Option<PixelColor>,

    /// Stroke width
    ///
    /// Set the stroke width for an object. Has no effect on fonts.
    pub stroke_width: i16,
}

impl Style
{
    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: PixelColor) -> Self {
        Self {
            stroke_color: Some(stroke_color),
            ..Style::default()
        }
    }

    /// Returns the stroke width as an `i16`.
    ///
    /// If the stroke width is too large to fit into an `i16` the maximum value
    /// for an `i16` is returned instead.
    pub(crate) fn stroke_width_i16(&self) -> i16 {
        self.stroke_width
    }
}

impl Default for Style
{
    fn default() -> Self {
        Self {
            fill_color: None,
            stroke_color: None,
            stroke_width: 1,
        }
    }
}

/// Add a style to an object
pub trait WithStyle
{
    /// Add a complete style to the object
    fn style(self, style: Style) -> Self;

    /// Set the stroke colour for the object
    ///
    /// This can be a noop
    fn stroke_color(self, color: Option<PixelColor>) -> Self;

    /// Set the stroke width for the object
    ///
    /// A stroke with a width of zero will not be rendered
    fn stroke_width(self, width: u16) -> Self;

    /// Set the fill property of the object's style
    ///
    /// This can be a noop
    fn fill_color(self, color: Option<PixelColor>) -> Self;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub fn new(x: i16, y: i16) -> Point {
        Point { x, y }
    }

    /// Creates a point with X and Y equal to zero.
    pub const fn zero() -> Self {
        Point { x: 0, y: 0 }
    }
}


impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}

impl Add<Size> for Point {
    type Output = Point;

    /// Offsets a point by adding a size.
    ///
    /// # Panics
    ///
    /// This function will panic if `width` or `height` are too large to be represented as an `i16`
    /// and debug assertions are enabled.
    fn add(self, other: Size) -> Point {
        let width = other.width as i16;
        let height = other.height as i16;

        debug_assert!(width >= 0, "width is too large");
        debug_assert!(height >= 0, "height is too large");

        Point::new(self.x + width, self.y + height)
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl AddAssign<Size> for Point {
    /// Offsets a point by adding a size.
    ///
    /// # Panics
    ///
    /// This function will panic if `width` or `height` are too large to be represented as an `i16`
    /// and debug assertions are enabled.
    fn add_assign(&mut self, other: Size) {
        let width = other.width as i16;
        let height = other.height as i16;

        debug_assert!(width >= 0, "width is too large");
        debug_assert!(height >= 0, "height is too large");

        self.x += width;
        self.y += height;
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }
}

impl Sub<Size> for Point {
    type Output = Point;

    /// Offsets a point by subtracting a size.
    ///
    /// # Panics
    ///
    /// This function will panic if `width` or `height` are too large to be represented as an `i16`
    /// and debug assertions are enabled.
    fn sub(self, other: Size) -> Point {
        let width = other.width as i16;
        let height = other.height as i16;

        debug_assert!(width >= 0, "width is too large");
        debug_assert!(height >= 0, "height is too large");

        Point::new(self.x - width, self.y - height)
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Point) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl SubAssign<Size> for Point {
    /// Offsets a point by subtracting a size.
    ///
    /// # Panics
    ///
    /// This function will panic if `width` or `height` are too large to be represented as an `i16`
    /// and debug assertions are enabled.
    fn sub_assign(&mut self, other: Size) {
        let width = other.width as i16;
        let height = other.height as i16;

        debug_assert!(width >= 0, "width is too large");
        debug_assert!(height >= 0, "height is too large");

        self.x -= width;
        self.y -= height;
    }
}

impl Index<usize> for Point {
    type Output = i16;

    fn index(&self, idx: usize) -> &i16 {
        match idx {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("index out of bounds: the len is 2 but the index is {}", idx),
        }
    }
}

impl Neg for Point {
    type Output = Point;

    fn neg(self) -> Self::Output {
        Point::new(-self.x, -self.y)
    }
}

impl From<(i16, i16)> for Point {
    fn from(other: (i16, i16)) -> Self {
        Point::new(other.0, other.1)
    }
}

impl From<[i16; 2]> for Point {
    fn from(other: [i16; 2]) -> Self {
        Point::new(other[0], other[1])
    }
}

impl From<&[i16; 2]> for Point {
    fn from(other: &[i16; 2]) -> Self {
        Point::new(other[0], other[1])
    }
}

impl From<Point> for (i16, i16) {
    fn from(other: Point) -> (i16, i16) {
        (other.x, other.y)
    }
}

impl From<&Point> for (i16, i16) {
    fn from(other: &Point) -> (i16, i16) {
        (other.x, other.y)
    }
}

impl Into<usize> for Point {
    fn into(self) -> usize {
        (self.x as usize) << 16 | (self.y as usize)
    }
}

/// A single pixel
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pixel(pub Point, pub PixelColor);

#[derive(Copy, Clone, Debug)]
pub struct Rect {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
}

impl Rect {
    pub fn new(x0: i16, y0: i16, x1: i16, y1: i16) -> Self {
        Self { x0, y0, x1, y1 }
    }
}

// impl Into<embedded_graphics::geometry::Point> for Point {
//     fn into(self) -> embedded_graphics::geometry::Point {
//         embedded_graphics::geometry::Point::new(self.x as _, self.y as _)
//     }
// }

impl From<usize> for Point {
    fn from(p: usize) -> Point {
        Point {
            x: (p >> 16 & 0xffff) as _,
            y: (p & 0xffff) as _,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Color {
    pub color: u16,
}

impl From<usize> for Color {
    fn from(c: usize) -> Color {
        Color { color: c as _ }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    /// Top left point of the rect
    pub top_left: Point,

    /// Bottom right point of the rect
    pub bottom_right: Point,

    /// Object style
    pub style: PixelColor,
}

#[derive(Debug)]
pub enum Opcode<'a> {
    /// Flush the buffer to the screen
    Flush,

    /// Clear the buffer to the specified color
    Clear(Color),

    /// Draw a line at the specified area
    Line(Point /* start */, Point /* end */),

    /// Draw a rectangle or square at the specified coordinates
    Rectangle(Point /* upper-left */, Point /* lower-right */),

    /// Draw a circle with a specified radius
    Circle(Point, u16 /* radius */),

    /// Change the style of the current pen
    Style(
        u16,   /* stroke width */
        Color, /* stroke color */
        Color, /* fill color */
    ),

    /// Clear the specified region
    ClearRegion(Rect),

    /// Render the string at the (x,y) coordinates
    String(&'a str),
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Flush),
                2 => Ok(Opcode::Clear(Color::from(m.arg1))),
                3 => Ok(Opcode::Line(Point::from(m.arg1), Point::from(m.arg2))),
                4 => Ok(Opcode::Rectangle(Point::from(m.arg1), Point::from(m.arg2))),
                5 => Ok(Opcode::Circle(Point::from(m.arg1), m.arg2 as _)),
                6 => Ok(Opcode::Style(
                    m.arg1 as _,
                    Color::from(m.arg2),
                    Color::from(m.arg3),
                )),
                7 => Ok(Opcode::ClearRegion(Rect::new(
                    m.arg1 as _,
                    m.arg2 as _,
                    m.arg3 as _,
                    m.arg4 as _,
                ))),
                _ => Err("unrecognized opcode"),
            },
            Message::Borrow(m) => match m.id {
                1 => {
                    let s = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::String(core::str::from_utf8(s).unwrap()))
                }
                _ => Err("unrecognized opcode"),
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl<'a> Into<Message> for Opcode<'a> {
    fn into(self) -> Message {
        match self {
            Opcode::Flush => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Clear(color) => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: color.color as _,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Line(start, end) => Message::Scalar(ScalarMessage {
                id: 3,
                arg1: start.into(),
                arg2: end.into(),
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Rectangle(start, end) => Message::Scalar(ScalarMessage {
                id: 4,
                arg1: start.into(),
                arg2: end.into(),
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Circle(center, radius) => Message::Scalar(ScalarMessage {
                id: 5,
                arg1: center.into(),
                arg2: radius as usize,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Style(stroke_width, stroke_color, fill_color) => {
                Message::Scalar(ScalarMessage {
                    id: 6,
                    arg1: stroke_width as _,
                    arg2: stroke_color.color as _,
                    arg3: fill_color.color as _,
                    arg4: 0,
                })
            }
            Opcode::ClearRegion(rect) => Message::Scalar(ScalarMessage {
                id: 7,
                arg1: rect.x0 as _,
                arg2: rect.y0 as _,
                arg3: rect.x1 as _,
                arg4: rect.y1 as _,
            }),
            Opcode::String(string) => {
                let region = xous::carton::Carton::from_bytes(string.as_bytes());
                Message::Borrow(region.into_message(1))
            }
        }
    }
}
