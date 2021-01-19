use crate::api::{Point, Rectangle, Gid};
use blitstr::{GlyphStyle, Cursor};

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
    // this is the operation as specified for the GAM. Note this is different from the "op" when sent to graphics-server
    // only the GAM should be sending TextViews to the graphics-server, and a different coding scheme is used for that link.
    operation: TextOp,

    pub untrusted: bool,  // render content with random stipples to indicate the strings within are untrusted
    pub token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    pub invert: bool, // only trusted, token-validated TextViews will have the invert bit respected

    // lower numbers are drawn last
    pub draw_order: usize,

    // offsets for text drawing -- exactly one of the following options should be specified
    pub bounds_hint: TextBounds,
    bounds_computed: Option<Rectangle>, // is Some(Rectangle) if bounds have been computed and text has not been modified

    pub style: GlyphStyle,
    text: xous::String<'a>,
    pub alignment: TextAlignment,
    pub cursor: Cursor,

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
            cursor: Cursor::new(0,0,0),
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
    pub fn get_bounds_computed(&self) -> Option<Rectangle> { self.bounds_computed }
    pub fn compute_bounds(&mut self) -> Result<(), xous::Error> {
        match self.bounds_hint {
            TextBounds::BoundingBox(r) => self.bounds_computed = Some(r),
            _=> todo!("other dynamic bounds computations not yet implemented"),
        }
        Ok(())
    }
    pub fn clear_str(&mut self) { self.text.clear() }
    pub fn to_str(&self) -> &str { self.text.to_str() }
}

impl<'a> core::fmt::Debug for TextView<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // this should definitely be extended to print more relevant data, but for now just render the string itself
        write!(f, "{}", self.text)
    }
}


impl<'a> core::fmt::Display for TextView<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.text)
    }
}

impl<'a> core::fmt::Write for TextView<'a> {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        self.bounds_computed = None;
        self.text.write_str(s)
    }
}