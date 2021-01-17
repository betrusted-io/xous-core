use xous::{Message, ScalarMessage};
use graphics_server::{Point, Rectangle};
use blitstr::{GlyphStyle, Cursor};
use hash32::{Hash, Hasher};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Gid {
    gid: [u32; 4],
}
impl Gid {
    pub fn new(id: [u32; 4]) -> Self { Gid{gid: id} }
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

    untrusted: bool,  // render content with random stipples to indicate the strings within are untrusted
    token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    invert: bool, // only trusted, token-validated TextViews will have the invert bit respected

    // lower numbers are drawn last
    draw_order: usize,

    // offsets for text drawing -- exactly one of the following options should be specified
    bounds_hint: TextBounds,
    bounds_computed: Option<Rectangle>, // is Some(Rectangle) if bounds have been computed and text has not been modified

    style: GlyphStyle,
    text: xous::String<'a>,
    alignment: TextAlignment,

    draw_border: bool,
    border_width: u16,
    rounded_border: bool,
    x_margin: u16,
    y_margin: u16,

    // this field specifies the beginning and end of a "selected" region of text
    selected: Option<[usize; 2]>,

    canvas: Gid, // GID of the canvas to draw on
}
impl<'a> TextView<'a> {
    pub fn new(maxlen: usize, draw_order: usize, bounds_hint: TextBounds, canvas: Gid) -> Self {
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
            draw_border: true,
            border_width: 1,
            rounded_border: false,
            x_margin: 4,
            y_margin: 4,
            selected: None,
            canvas,
        }
    }
    pub fn set_op(&mut self, op: TextOp) { self.operation = op; }
    pub fn get_op(&self) -> TextOp { self.operation }
}

impl<'a> core::fmt::Debug for TextView<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // this should definitely be extended to print more relevant data, but for now just render the string itself
        write!(f, "{}", self.text)
    }
}


#[derive(Debug)]
pub enum Opcode<'a> {
    // clears a canvas with a given GID
    ClearCanvas(Gid),

    // renders a TextView
    RenderTextView(TextView<'a>),

    // returns a GID to the "content" Canvas; requires an authentication token
    RequestContentCanvas(Gid),

    // hides a canvas with a given GID
    HideCanvas(Gid),

    // requests the GID to the "input" Canvas; call only works once (for the IME server), then maps out
    RequestInputCanvas(),

    // indicates if the current UI layout requires an input field
    HasInput(bool),
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::ClearCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                _ => Err("GAM api: unknown Scalar ID"),
            },
            _ => Err("GAM api: unhandled message type"),
        }
    }
}

impl<'a> Into<Message> for Opcode<'a> {
    fn into(self) -> Message {
        match self {
            Opcode::ClearCanvas(gid) => Message::Scalar(ScalarMessage {
                id: 0, arg1: gid.gid[0] as _, arg2: gid.gid[1] as _, arg3: gid.gid[2] as _, arg4: gid.gid[3] as _
            }),
            _ => panic!("GAM api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}
