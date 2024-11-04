use core::cmp::{max, min};
use core::ops::{Add, AddAssign, Div, DivAssign, Index, Mul, MulAssign, Neg, Sub, SubAssign};

/// Type wrapper for native colors
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorNative(pub usize);
impl From<usize> for ColorNative {
    fn from(value: usize) -> Self { Self { 0: value } }
}
impl Into<usize> for ColorNative {
    fn into(self) -> usize { self.0 }
}

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

/// Style properties for an object
#[derive(Debug, Copy, Clone)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<ColorNative>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<ColorNative>,

    /// Stroke width
    pub stroke_width: isize,
}

impl DrawStyle {
    pub fn new(fill: ColorNative, stroke: ColorNative, width: isize) -> Self {
        Self { fill_color: Some(fill), stroke_color: Some(stroke), stroke_width: width }
    }

    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: ColorNative) -> Self {
        Self { stroke_color: Some(stroke_color), ..DrawStyle::default() }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self { fill_color: Some(ColorNative(0)), stroke_color: Some(ColorNative(0)), stroke_width: 1 }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: isize,
    pub y: isize,
}
impl Point {
    pub fn new(x: isize, y: isize) -> Self { Point { x, y } }
}

impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point { Point::new(self.x + other.x, self.y + other.y) }
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point { Point::new(self.x - other.x, self.y - other.y) }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Point) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl Mul<isize> for Point {
    type Output = Point;

    fn mul(self, rhs: isize) -> Point { Point::new(self.x * rhs, self.y * rhs) }
}

impl MulAssign<isize> for Point {
    fn mul_assign(&mut self, rhs: isize) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<isize> for Point {
    type Output = Point;

    fn div(self, rhs: isize) -> Point { Point::new(self.x / rhs, self.y / rhs) }
}

impl DivAssign<isize> for Point {
    fn div_assign(&mut self, rhs: isize) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Index<usize> for Point {
    type Output = isize;

    fn index(&self, idx: usize) -> &isize {
        match idx {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("index out of bounds: the len is 2 but the index is {}", idx),
        }
    }
}

impl Neg for Point {
    type Output = Point;

    fn neg(self) -> Self::Output { Point::new(-self.x, -self.y) }
}

//////////////////////////// RECTANGLE
#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    pub tl: Point,
    pub br: Point,
    pub style: DrawStyle,
}

impl Rectangle {
    pub fn new(p1: Point, p2: Point) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style: DrawStyle::default(),
        }
    }

    pub fn new_coords(x0: isize, y0: isize, x1: isize, y1: isize) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: DrawStyle::default(),
        }
    }

    // stack a new rectangle on top of the current one (same width)
    // positive heights go *below*, negative go *above* in screen coordinate space. borders are
    // non-overlapping.
    pub fn new_v_stack(reference: Rectangle, height: isize) -> Rectangle {
        if height >= 0 {
            // rectangle below
            Rectangle::new_coords(reference.tl.x, reference.br.y + 1, reference.br.x, reference.br.y + height)
        } else {
            // rectangle above
            Rectangle::new_coords(reference.tl.x, reference.tl.y + height, reference.br.x, reference.tl.y - 1)
        }
    }

    // make a new rectangle than spans between the above and below rectangles. the borders are
    // non-overlapping.
    pub fn new_v_span(above: Rectangle, below: Rectangle) -> Rectangle {
        Rectangle::new_coords(above.tl.x, above.br.y + 1, below.br.x, below.tl.y - 1)
    }

    // "stack" a rectangle to the left or right of the current one (same height)
    // positive widths go to the right, negative to the left. borders are non-overlapping
    pub fn new_h_stack(reference: Rectangle, width: isize) -> Rectangle {
        if width >= 0 {
            // stack to the right
            Rectangle::new_coords(
                reference.br.x + 1,
                reference.tl.y,
                reference.br.x + width + 1,
                reference.br.y,
            )
        } else {
            // stack to the left
            Rectangle::new_coords(
                reference.tl.x + width - 1,
                reference.tl.y,
                reference.tl.x - 1,
                reference.br.y,
            )
        }
    }

    // make a new rectangle than spans between the left and right rectangles. borders are non-overlapping
    pub fn new_h_span(left: Rectangle, right: Rectangle) -> Rectangle {
        Rectangle::new_coords(left.br.x + 1, left.tl.y, right.tl.x - 1, right.br.y)
    }

    pub fn tl(&self) -> Point { self.tl }

    pub fn br(&self) -> Point { self.br }

    pub fn bl(&self) -> Point { Point { x: self.tl.x, y: self.br.y } }

    pub fn tr(&self) -> Point { Point { x: self.br.x, y: self.tl.y } }

    pub fn intersects(&self, other: Rectangle) -> bool {
        !(self.br.x < other.tl.x
            || self.tl.y > other.br.y
            || self.br.y < other.tl.y
            || self.tl.x > other.br.x)
    }

    pub fn intersects_point(&self, point: Point) -> bool {
        ((point.x >= self.tl.x) && (point.x <= self.br.x))
            && ((point.y >= self.tl.y) && (point.y <= self.br.y))
    }

    /// takes the current Rectangle, and clips it with a clipping Rectangle; returns a new rectangle as the
    /// result
    pub fn clip_with(&self, clip: Rectangle) -> Option<Rectangle> {
        // check to see if we even overlap; if not, don't do any computation
        if !self.intersects(clip) {
            return None;
        }
        let tl: Point = Point::new(
            if self.tl.x < clip.tl.x { clip.tl.x } else { self.tl.x },
            if self.tl.y < clip.tl.y { clip.tl.y } else { self.tl.y },
        );
        let br: Point = Point::new(
            if self.br.x > clip.br.x { clip.br.x } else { self.br.x },
            if self.br.y > clip.br.y { clip.br.y } else { self.br.y },
        );
        Some(Rectangle::new(tl, br))
    }

    pub fn new_coords_with_style(x0: isize, y0: isize, x1: isize, y1: isize, style: DrawStyle) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style,
        }
    }

    pub fn new_with_style(p1: Point, p2: Point, style: DrawStyle) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style,
        }
    }

    pub fn x0(&self) -> u32 { self.tl.x as u32 }

    pub fn x1(&self) -> u32 { self.br.x as u32 }

    pub fn y0(&self) -> u32 { self.tl.y as u32 }

    pub fn y1(&self) -> u32 { self.br.y as u32 }

    pub fn width(&self) -> u32 { (self.br.x - self.tl.x) as u32 }

    pub fn height(&self) -> u32 { (self.br.y - self.tl.y) as u32 }

    pub fn translate(&mut self, offset: Point) {
        self.tl.x += offset.x;
        self.br.x += offset.x;
        self.tl.y += offset.y;
        self.br.y += offset.y;
    }

    pub fn translate_chain(self, offset: Point) -> Rectangle {
        Rectangle {
            tl: Point::new(self.tl.x + offset.x, self.tl.y + offset.y),
            br: Point::new(self.br.x + offset.x, self.br.y + offset.y),
            style: self.style,
        }
    }

    pub fn normalize(&mut self) {
        self.br.x -= self.tl.x;
        self.br.y -= self.tl.y;
        self.tl.x = 0;
        self.tl.y = 0;
    }

    // this "margins in" a rectangle on all sides; if the margin is more than the twice any
    // dimension it just reduces the rectangle to a line at the midpoint of the axis dimension
    pub fn margin(&mut self, margin: Point) {
        if margin.x * 2 <= (self.br.x - self.tl.x) {
            self.tl.x += margin.x;
            self.br.x -= margin.x;
        } else {
            let midpoint = (self.br.x + self.tl.x) / 2;
            self.tl.x = midpoint;
            self.br.x = midpoint;
        }
        if margin.y * 2 <= (self.br.y - self.tl.y) {
            self.tl.y += margin.y;
            self.br.y -= margin.y;
        } else {
            let midpoint = (self.br.y + self.tl.y) / 2;
            self.tl.y = midpoint;
            self.br.y = midpoint;
        }
    }

    // this "margins out" on all sides
    pub fn margin_out(&mut self, margin: Point) {
        self.tl.x -= margin.x;
        self.tl.y -= margin.y;
        self.br.x += margin.x;
        self.br.y += margin.y;
    }
}

//////////////////////////// LINE

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub start: Point,
    pub end: Point,

    /// Drawing style
    pub style: DrawStyle,
}

impl Line {
    pub fn new(start: Point, end: Point) -> Line { Line { start, end, style: DrawStyle::default() } }

    pub fn new_with_style(start: Point, end: Point, style: DrawStyle) -> Line { Line { start, end, style } }

    pub fn translate(&mut self, offset: Point) {
        self.start = self.start + offset;
        self.end = self.end + offset;
    }
}
