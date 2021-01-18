#![allow(dead_code)]

/*
  Primitives needed:

  - clipping rectangles (for drawing)
  - rounded rectangles
  - inverse text
  - icons/sprites
  - width of a text string with a given font (to compute alignments)
  - untrusted backgrounds
  - bitmaps

*/
use crate::op::{HEIGHT, WIDTH};
use blitstr::{ClipRect, Cursor, GlyphStyle};
use core::cmp::{max, min};
use core::ops::{Add, AddAssign, Index, Neg, Sub, SubAssign};
use xous::{Message, ScalarMessage};
use hash32::{Hash, Hasher};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelColor {
    Dark,
    Light,
}

impl From<usize> for PixelColor {
    fn from(pc: usize) -> Self {
        if pc != 0 {
            PixelColor::Dark
        } else {
            PixelColor::Light
        }
    }
}

impl From<bool> for PixelColor {
    fn from(pc: bool) -> Self {
        if pc {
            PixelColor::Dark
        } else {
            PixelColor::Light
        }
    }
}

impl Into<usize> for PixelColor {
    fn into(self) -> usize {
        if self == PixelColor::Dark {
            1
        } else {
            0
        }
    }
}

impl Into<bool> for PixelColor {
    fn into(self) -> bool {
        if self == PixelColor::Dark {
            true
        } else {
            false
        }
    }
}

/// Style properties for an object
#[derive(Debug, Copy, Clone)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<PixelColor>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<PixelColor>,

    /// Stroke width
    pub stroke_width: i16,
}

impl DrawStyle {
    pub fn new(fill: PixelColor, stroke: PixelColor, width: i16) -> Self {
        Self {
            fill_color: Some(fill),
            stroke_color: Some(stroke),
            stroke_width: width,
        }
    }

    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: PixelColor) -> Self {
        Self {
            stroke_color: Some(stroke_color),
            ..DrawStyle::default()
        }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            fill_color: Some(PixelColor::Dark),
            stroke_color: Some(PixelColor::Dark),
            stroke_width: 1,
        }
    }
}

impl From<usize> for DrawStyle {
    fn from(s: usize) -> Self {
        // usize split into these words:
        //  31 ...  16  15 ... 4     3..2    1..0
        //    width       rsvd      stroke   fill
        // where the MSB of stroke/fill encodes Some/None
        let fc: PixelColor = (s & 0b0001).into();
        let sc: PixelColor = (s & 0b0100).into();
        DrawStyle {
            fill_color: if s & 0b0010 != 0 { Some(fc) } else { None },
            stroke_color: if s & 0b1000 != 0 { Some(sc) } else { None },
            stroke_width: (s >> 16) as i16,
        }
    }
}

impl Into<usize> for DrawStyle {
    fn into(self) -> usize {
        let sc: usize;
        if self.stroke_color.is_some() {
            sc = 0b10 | self.stroke_color.unwrap() as usize;
        } else {
            sc = 0;
        }
        let fc: usize;
        if self.fill_color.is_some() {
            fc = 0b10 | self.fill_color.unwrap() as usize;
        } else {
            fc = 0;
        }
        (self.stroke_width as usize) << 16 | sc << 2 | fc
    }
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

impl AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Point) {
        self.x -= other.x;
        self.y -= other.y;
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

impl From<usize> for Point {
    fn from(p: usize) -> Point {
        Point {
            x: (p >> 16 & 0xffff) as _,
            y: (p & 0xffff) as _,
        }
    }
}

/// A single pixel
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pixel(pub Point, pub PixelColor);

#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    /// Top left point of the rect
    pub tl: Point,

    /// Bottom right point of the rect
    pub br: Point,

    /// Drawing style
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
    pub fn new_coords(x0: i16, y0: i16, x1: i16, y1: i16) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: DrawStyle::default(),
        }
    }
    // stack a new rectangle on top of the current one (same width)
    // positive widths go *below*, negative go *above* in screen coordinate space. borders are non-overlapping.
    pub fn new_v_stack(reference: Rectangle, width: i16) -> Rectangle {
        if width >= 0 { // rectangle below
            Rectangle::new_coords(reference.tl.x, reference.br.y + 1,
            reference.br.x, reference.br.y + width + 1)
        } else { // rectangle above
            Rectangle::new_coords(reference.tl.x, reference.tl.y + width - 1,
            reference.br.x, reference.tl.y - 1)
        }
    }
    // make a new rectangle than spans between the above and below rectangles. the borders are non-overlapping.
    pub fn new_v_span(above: Rectangle, below: Rectangle) -> Rectangle {
        Rectangle::new_coords(above.tl.x, above.br.y + 1, below.br.x, below.tl.y - 1)
    }
    // "stack" a rectangle to the left or right of the current one (same height)
    // positive widths go to the right, negative to the left. borders are non-overlapping
    pub fn new_h_stack(reference: Rectangle, width: i16) -> Rectangle {
        if width >= 0 { // stack to the right
            Rectangle::new_coords(reference.br.x + 1, reference.tl.y,
            reference.br.x + width + 1, reference.br.y)
        } else { // stack to the left
            Rectangle::new_coords(reference.tl.x + width - 1, reference.tl.y,
            reference.tl.x - 1, reference.br.y)
        }
    }
    // make a new rectangle than spans between the left and right rectangles. borders are non-overlapping
    pub fn new_h_span(left: Rectangle, right: Rectangle) -> Rectangle {
        Rectangle::new_coords(left.br.x + 1, left.tl.y, right.tl.x - 1, right.br.y)
    }
    pub fn intersects(&self, other: Rectangle) -> bool {
        ((other.tl.x >= self.tl.x) && (other.tl.x <= self.br.x)) &&
        ((other.tl.y >= self.tl.y) && (other.tl.y <= self.br.y))
        ||
        ((other.br.x >= self.tl.x) && (other.br.x <= self.br.x)) &&
        ((other.br.y >= self.tl.y) && (other.br.y <= self.br.y))
    }
    pub fn new_coords_with_style(
        x0: i16,
        y0: i16,
        x1: i16,
        y1: i16,
        style: DrawStyle,
    ) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: style,
        }
    }
    pub fn new_with_style(p1: Point, p2: Point, style: DrawStyle) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style: style,
        }
    }
    pub fn x0(&self) -> usize {
        self.tl.x as usize
    }
    pub fn x1(&self) -> usize {
        self.br.x as usize
    }
    pub fn y0(&self) -> usize {
        self.tl.y as usize
    }
    pub fn y1(&self) -> usize {
        self.br.y as usize
    }

    /// Make a rectangle of the full screen size
    pub fn full_screen() -> Rectangle {
        Rectangle {
            tl: Point::new(0, 0),
            br: Point::new(WIDTH, HEIGHT),
            style: DrawStyle::default(),
        }
    }
    /// Make a rectangle of the screen size minus padding
    pub fn padded_screen() -> Rectangle {
        let pad = 6;
        Rectangle {
            tl: Point::new(pad, pad),
            br: Point::new(WIDTH - pad, HEIGHT - pad),
            style: DrawStyle::default(),
        }
    }
}

impl Into<ClipRect> for Rectangle {
    fn into(self) -> ClipRect {
        ClipRect::new(self.x0(), self.y0(), self.x1(), self.y1())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub start: Point,
    pub end: Point,

    /// Drawing style
    pub style: DrawStyle,
}

impl Line {
    pub fn new(start: Point, end: Point) -> Line {
        Line {
            start: start,
            end: end,
            style: DrawStyle::default(),
        }
    }
    pub fn new_with_style(start: Point, end: Point, style: DrawStyle) -> Line {
        Line {
            start: start,
            end: end,
            style: style,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Point,
    pub radius: i16,

    /// Drawing style
    pub style: DrawStyle,
}

impl Circle {
    pub fn new(c: Point, r: i16) -> Circle {
        Circle {
            center: c,
            radius: r,
            style: DrawStyle::default(),
        }
    }
    pub fn new_with_style(c: Point, r: i16, style: DrawStyle) -> Circle {
        Circle {
            center: c,
            radius: r,
            style,
        }
    }
}

#[derive(Debug)]
pub enum Opcode<'a> {
    /// Flush the buffer to the screen
    Flush,

    /// Clear the buffer to "light" colored pixels
    Clear,

    /// Draw a line at the specified area
    Line(Line),

    /// Draw a rectangle or square at the specified coordinates
    Rectangle(Rectangle),

    /// Draw a circle with a specified radius
    Circle(Circle),

    /// Set the current string glyph set for strings
    SetGlyphStyle(GlyphStyle),

    /// Set the cursor point for the current string clipping region
    SetCursor(Cursor),

    /// Retrieve the current cursor porint for the current string clipping region
    GetCursor,

    /// Set the clipping region for the string.
    SetStringClipping(ClipRect),

    /// Overwrite the string inside the clipping region.
    String(&'a str),

    /// Xor the string inside the clipping region.
    StringXor(&'a str),

    /// Simulate the string on the clipping region (for computing text widths)
    SimulateString(&'a str),

    /// Retrieve the X and Y dimensions of the screen
    ScreenSize,

    /// Retrieve the current Glyph style
    QueryGlyphStyle,

    /// gets info about the current glyph to assist with layout
    QueryGlyphProps(GlyphStyle),

    /// draws a textview
    TextView(&'a TextView<'a>),
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Flush),
                2 => Ok(Opcode::Clear),
                3 => Ok(Opcode::Line(Line::new_with_style(
                    Point::from(m.arg1),
                    Point::from(m.arg2),
                    DrawStyle::from(m.arg3),
                ))),
                4 => Ok(Opcode::Rectangle(Rectangle::new_with_style(
                    Point::from(m.arg1),
                    Point::from(m.arg2),
                    DrawStyle::from(m.arg3),
                ))),
                5 => Ok(Opcode::Circle(Circle::new_with_style(
                    Point::from(m.arg1),
                    m.arg2 as _,
                    DrawStyle::from(m.arg3),
                ))),
                9 => Ok(Opcode::SetGlyphStyle(GlyphStyle::from(m.arg1))),
                11 => Ok(Opcode::SetStringClipping(ClipRect::new(
                    m.arg1 as _,
                    m.arg2 as _,
                    m.arg3 as _,
                    m.arg4 as _,
                ))),
                12 => Ok(Opcode::SetCursor(Cursor::new(
                    m.arg1 as _,
                    m.arg2 as _,
                    m.arg3 as _,
                ))),
                14 => Ok(Opcode::QueryGlyphProps(GlyphStyle::from(m.arg1))),
                _ => Err("unrecognized opcode"),
            },
            Message::BlockingScalar(m) => match m.id {
                8 => Ok(Opcode::ScreenSize),
                10 => Ok(Opcode::QueryGlyphStyle),
                13 => Ok(Opcode::GetCursor),
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
                },
                2 => {
                    let s = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::StringXor(core::str::from_utf8(s).unwrap()))
                },
                3 => {
                    let s = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::SimulateString(core::str::from_utf8(s).unwrap()))
                },
                _ => Err("unrecognized opcode"),
            },
            Message::MutableBorrow(m) => match m.id {
                0x100 => {
                    let tv: &mut TextView = unsafe {
                        &mut *(m.buf.as_mut_ptr() as *mut TextView)
                    };
                    Ok(Opcode::TextView(tv))
                },
                _ => Err("unrecognized opcode"),
            }
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
            Opcode::Clear => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::Line(line) => Message::Scalar(ScalarMessage {
                id: 3,
                arg1: line.start.into(),
                arg2: line.end.into(),
                arg3: line.style.into(),
                arg4: 0,
            }),
            Opcode::Rectangle(r) => Message::Scalar(ScalarMessage {
                id: 4,
                arg1: r.tl.into(),
                arg2: r.br.into(),
                arg3: r.style.into(),
                arg4: 0,
            }),
            Opcode::Circle(c) => Message::Scalar(ScalarMessage {
                id: 5,
                arg1: c.center.into(),
                arg2: c.radius as usize,
                arg3: c.style.into(),
                arg4: 0,
            }),
            Opcode::ScreenSize => Message::BlockingScalar(ScalarMessage {
                id: 8,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::QueryGlyphStyle => Message::BlockingScalar(ScalarMessage {
                id: 10,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::SetGlyphStyle(glyph) => Message::Scalar(ScalarMessage {
                id: 9,
                arg1: glyph as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::SetStringClipping(r) => Message::Scalar(ScalarMessage {
                id: 11,
                arg1: r.min.x as _,
                arg2: r.min.y as _,
                arg3: r.max.x as _,
                arg4: r.max.y as _,
            }),
            Opcode::String(string) => {
                let region = xous::carton::Carton::from_bytes(string.as_bytes());
                Message::Borrow(region.into_message(1))
            }
            Opcode::StringXor(string) => {
                let region = xous::carton::Carton::from_bytes(string.as_bytes());
                Message::Borrow(region.into_message(2))
            }
            Opcode::SimulateString(string) => {
                let region = xous::carton::Carton::from_bytes(string.as_bytes());
                Message::Borrow(region.into_message(3))
            }
            Opcode::SetCursor(c) => Message::Scalar(ScalarMessage {
                id: 12,
                arg1: c.pt.x,
                arg2: c.pt.y,
                arg3: c.line_height,
                arg4: 0,
            }),
            Opcode::GetCursor => Message::BlockingScalar(ScalarMessage {
                id: 13,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::QueryGlyphProps(glyph) => Message::Scalar(ScalarMessage {
                id: 14,
                arg1: glyph as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            _ => panic!("GFX api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}


#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Gid {
    gid: [u32; 4],
}
impl Gid {
    pub fn new(id: [u32; 4]) -> Self { Gid{gid: id} }
    pub fn gid(&self) -> [u32; 4] { self.gid }
}
impl hash32::Hash for Gid {
    fn hash<H>(&self, state: &mut H)
    where
    H: Hasher,
    {
        Hash::hash(&self.gid[..], state);
    }
}

#[derive(Debug, Copy, Clone)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Copy, Clone)]
pub enum TextBounds {
    // fixed width and height in a rectangle
    BoundingBox(Rectangle),
    // fixed width, grows up from bottom right
    GrowableFromBr(Point, u16),
    // fixed width, grows down from top left
    GrowableFromTl(Point, u16),
    // fixed width, grows up from bottom left
    GrowableFromBl(Point, u16),
}

#[derive(Debug, Copy, Clone)]
// operations that may be requested of a TextView when sent to GAM
pub enum TextOp {
    Nop,
    Render,
    ComputeBounds,
}
impl Into<usize> for TextOp {
    fn into(self) -> usize {
        match self {
            TextOp::Nop => 0,
            TextOp::Render => 1,
            TextOp::ComputeBounds => 2,
        }
    }
}
impl From<usize> for TextOp {
    fn from(code: usize) -> Self {
        match code {
            1 => TextOp::Render,
            2 => TextOp::ComputeBounds,
            _ => TextOp::Nop,
        }
    }
}

#[repr(C)]
pub struct TextView<'a> {
    operation: TextOp,

    pub untrusted: bool,  // render content with random stipples to indicate the strings within are untrusted
    pub token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    pub invert: bool, // only trusted, token-validated TextViews will have the invert bit respected

    // lower numbers are drawn last
    pub draw_order: usize,

    // offsets for text drawing -- exactly one of the following options should be specified
    pub bounds_hint: TextBounds,
    pub bounds_computed: Option<Rectangle>, // is Some(Rectangle) if bounds have been computed and text has not been modified

    pub style: GlyphStyle,
    pub text: xous::String<'a>,
    pub alignment: TextAlignment,
    pub cursor: blitstr::Cursor,

    pub draw_border: bool,
    pub clear_area: bool,
    pub border_width: u16,
    pub rounded_border: bool,
    pub x_margin: u16,
    pub y_margin: u16,

    // this field specifies the beginning and end of a "selected" region of text
    pub selected: Option<[usize; 2]>,

    canvas: Gid, // GID of the canvas to draw on
}
impl<'a> TextView<'a> {
    pub fn new(canvas: Gid, maxlen: usize, draw_order: usize, bounds_hint: TextBounds) -> Self {
        TextView {
            operation: TextOp::Nop,
            untrusted: true,
            token: None,
            invert: false,
            draw_order,
            bounds_hint,
            bounds_computed: None,
            style: GlyphStyle::Regular,
            text: xous::String::new(maxlen),
            alignment: TextAlignment::Left,
            cursor: blitstr::Cursor::new(0,0,0),
            draw_border: true,
            border_width: 1,
            rounded_border: false,
            x_margin: 4,
            y_margin: 4,
            selected: None,
            canvas,
            clear_area: true,
        }
    }
    pub fn set_op(&mut self, op: TextOp) { self.operation = op; }
    pub fn get_op(&self) -> TextOp { self.operation }
    pub fn get_canvas_gid(&self) -> Gid { self.canvas }
}

impl<'a> core::fmt::Debug for TextView<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // this should definitely be extended to print more relevant data, but for now just render the string itself
        write!(f, "{}", self.text)
    }
}
