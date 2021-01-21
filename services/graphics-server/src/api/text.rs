use crate::api::{Point, Rectangle, Gid};
use blitstr_ref as blitstr;
use blitstr::{GlyphStyle, Cursor};

use log::{error, info};

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

// roughly 168 bytes to represent the rest of the struct, and we want to fill out the 4096 byte page with text
const TEXTVIEW_LEN: usize = 3072;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TextView {
    // this is the operation as specified for the GAM. Note this is different from the "op" when sent to graphics-server
    // only the GAM should be sending TextViews to the graphics-server, and a different coding scheme is used for that link.
    operation: TextOp,

    pub untrusted: bool,  // render content with random stipples to indicate the strings within are untrusted
    pub token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    pub invert: bool, // only trusted, token-validated TextViews will have the invert bit respected

    // lower numbers are drawn last
    pub draw_order: u32,

    // offsets for text drawing -- exactly one of the following options should be specified
    pub bounds_hint: TextBounds,
    bounds_computed: Option<Rectangle>, // is Some(Rectangle) if bounds have been computed and text has not been modified

    pub style: GlyphStyle,
    text: [u8; TEXTVIEW_LEN],
    length: u32,
    pub alignment: TextAlignment,
    pub cursor: Cursor,

    pub draw_border: bool,
    pub clear_area: bool,
    pub border_width: u16,
    pub rounded_border: bool,
    pub x_margin: u16,
    pub y_margin: u16,

    // this field specifies the beginning and end of a "selected" region of text
    pub selected: Option<[u32; 2]>,

    canvas: Gid, // GID of the canvas to draw on
}
impl TextView {
    pub fn new(canvas: Gid, draw_order: u32, bounds_hint: TextBounds) -> Self {
        TextView {
            operation: TextOp::Nop,
            untrusted: true,
            token: None,
            invert: false,
            draw_order,
            bounds_hint,
            bounds_computed: None,
            style: GlyphStyle::Regular,
            text: [0; TEXTVIEW_LEN],
            length: 0,
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
            TextBounds::BoundingBox(r) => {
                info!("GFX/text - r {:?}", r);
                self.bounds_computed = Some(r);
                info!("GFX/text - bounds_computed {:?}", self.bounds_computed);
            },
            _=> todo!("other dynamic bounds computations not yet implemented"),
        }
        Ok(())
    }

    pub fn to_str(&self) -> &str {
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.text.as_ptr(), self.length as usize)
        })
        .unwrap()
    }

    pub fn clear_str(&mut self) { self.text = [0; TEXTVIEW_LEN] }

    pub fn populate_from(&mut self, t: &TextView) {
        self.operation = t.operation;
        self.untrusted = t.untrusted;
        self.token = t.token;
        self.invert = t.invert;
        self.draw_order = t.draw_order;
        self.bounds_hint = t.bounds_hint;
        self.bounds_computed = t.bounds_computed;
        self.style = t.style;
        self.text = t.text;
        self.length = t.length;
        self.alignment = t.alignment;
        self.cursor = t.cursor;
        self.draw_border = t.draw_border;
        self.clear_area = t.clear_area;
        self.border_width = t.border_width;
        self.rounded_border = t.rounded_border;
        self.x_margin = t.x_margin;
        self.y_margin = t.y_margin;
        self.selected = t.selected;
        self.canvas = t.canvas;
    }
}

// Allow a `&TextView` to be used anywhere that expects a `&str`
impl AsRef<str> for TextView {
    fn as_ref(&self) -> &str {
        self.to_str()
    }
}

impl core::fmt::Debug for TextView {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // this should definitely be extended to print more relevant data, but for now just render the string itself
        write!(f, "{:?}, {:?}, {:?}, {:?}, {}",
            self.get_op(), self.bounds_hint, self.cursor, self.get_canvas_gid(), self.to_str())
    }
}

// allow printing of the text
impl core::fmt::Display for TextView {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

// allow `write!()` macro on a` &TextView`
impl core::fmt::Write for TextView {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        self.bounds_computed = None;

        let b = s.bytes();

        // Ensure the length is acceptable
        if b.len() + self.length as usize > self.text.len() as usize {
            Err(core::fmt::Error)?;
        }
        // append the write to the array
        for c in s.bytes() {
            if self.length < self.text.len() as u32 {
                self.text[self.length as usize] = c;
                self.length += 1;
            }
        }
        // Attempt to convert the string to UTF-8 to validate it's correct UTF-8.
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.text.as_ptr(), self.length as usize)
        })
        .map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}