use xous::{Message, ScalarMessage};
use graphics_server::{Point, Rectangle};
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

#[repr(C)]
pub struct TextView<'a> {
    untrusted: bool,  // render content with random stipples to indicate the strings within are untrusted
    token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    invert: bool, // only trusted, token-validated TextViews will have the invert bit respected

    // lower numbers are drawn last
    draw_order: usize,

    // offsets for text drawing -- exactly one of the following options should be specified
    bounds: TextBounds,
    computed_bounds: Option<Rectangle>, // is Some(Rectangle) if bounds have been computed and text has not been modified

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

    canvas: [u32; 4], // GID of the canvas to draw on
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
    ClearCanvas([u32; 4]),

    // renders a TextView
    RenderTextView(TextView<'a>),

    // returns a GID to the "content" Canvas; requires an authentication token
    RequestContentCanvas([u32; 4]),

    // hides a canvas with a given GID
    HideCanvas([u32; 4]),

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
                0 => Ok(Opcode::ClearCanvas([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _])),
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
                id: 0, arg1: gid[0] as _, arg2: gid[1] as _, arg3: gid[2] as _, arg4: gid[3] as _
            }),
            _ => panic!("GAM api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}
