use num_traits::*;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PixelColor {
    Dark,
    Light,
}

impl From<bool> for PixelColor {
    fn from(pc: bool) -> Self { if pc { PixelColor::Dark } else { PixelColor::Light } }
}

impl From<PixelColor> for bool {
    fn from(pc: PixelColor) -> bool { if pc == PixelColor::Dark { true } else { false } }
}

impl From<usize> for PixelColor {
    fn from(pc: usize) -> Self { if pc == 0 { PixelColor::Light } else { PixelColor::Dark } }
}

impl From<PixelColor> for usize {
    fn from(pc: PixelColor) -> usize { if pc == PixelColor::Light { 0 } else { 1 } }
}

/// Style properties for an object
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<PixelColor>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<PixelColor>,

    /// Stroke width
    pub stroke_width: i16,
}

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Rectangle {
    /// Top left point of the rect
    pub tl: Point,

    /// Bottom right point of the rect
    pub br: Point,

    /// Drawing style
    pub style: DrawStyle,
}

/// coordinates are local to the canvas, not absolute to the screen
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum TextBounds {
    // fixed width and height in a rectangle
    BoundingBox(Rectangle),
    // fixed width, grows up from bottom right
    GrowableFromBr(Point, u16),
    // fixed width, grows down from top left
    GrowableFromTl(Point, u16),
    // fixed width, grows up from bottom left
    GrowableFromBl(Point, u16),
    // fixed width, grows down from top right
    GrowableFromTr(Point, u16),
    // fixed width, centered aligned top
    CenteredTop(Rectangle),
    // fixed width, centered aligned bottom
    CenteredBot(Rectangle),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Gid {
    /// a 128-bit random identifier for graphical objects
    pub gid: [u32; 4],
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, PartialEq)]
// operations that may be requested of a TextView when sent to GAM
pub enum TextOp {
    Nop,
    Render,
    ComputeBounds, // maybe we don't need this
}

/// Style options for Latin script fonts
#[derive(Copy, Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum GlyphStyle {
    Small = 0,
    Regular = 1,
    Bold = 2,
    Monospace = 3,
    Cjk = 4,
    Large = 5,
    ExtraLarge = 6,
    Tall = 7,
}

/// Point specifies a pixel coordinate
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Pt {
    pub x: i16,
    pub y: i16,
}

#[derive(Copy, Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Cursor {
    pub pt: Pt,
    pub line_height: usize,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub struct TextView {
    // this is the operation as specified for the GAM. Note this is different from the "op" when sent to
    // graphics-server only the GAM should be sending TextViews to the graphics-server, and a different
    // coding scheme is used for that link.
    operation: TextOp,
    canvas: Gid, // GID of the canvas to draw on
    pub clip_rect: Option<Rectangle>, /* this is set by the GAM to the canvas' clip_rect; needed by gfx
                  * for drawing. Note this is in screen coordinates. */

    pub untrusted: bool, // render content with random stipples to indicate the strings within are untrusted
    pub token: Option<[u32; 4]>, // optional 128-bit token which is presented to prove a field's trustability
    pub invert: bool,    // only trusted, token-validated TextViews will have the invert bit respected

    // offsets for text drawing -- exactly one of the following options should be specified
    // note that the TextBounds coordinate system is local to the canvas, not the screen
    pub bounds_hint: TextBounds,
    pub bounds_computed: Option<Rectangle>, /* is Some(Rectangle) if bounds have been computed and text
                                             * has not been modified. This is local to the canvas. */
    pub overflow: Option<bool>, /* indicates if the text has overflowed the canvas, set by the drawing
                                 * routine */
    dry_run: bool, /* callers should not set; use TexOp to select. gam-side bookkeepping, set to true if
                    * no drawing is desired and we just want to compute the bounds */

    pub style: GlyphStyle,
    pub cursor: Cursor,
    pub insertion: Option<i32>, // this is the insertion point offset, if it's to be drawn, on the string
    pub ellipsis: bool,

    pub draw_border: bool,
    pub clear_area: bool, // you almost always want this to be true
    pub border_width: u16,
    pub rounded_border: Option<u16>, // radius of the rounded border, if applicable
    pub margin: Point,

    // this field specifies the beginning and end of a "selected" region of text
    pub selected: Option<[u32; 2]>,

    // this field tracks the state of a busy animation, if `Some`
    pub busy_animation_state: Option<u32>,

    pub text: String,
}

pub const TEXTVIEW_DEFAULT_STYLE: GlyphStyle = GlyphStyle::Regular;

#[allow(dead_code)]
impl Cursor {
    // Make a new Cursor. When in doubt, set line_height = 0.
    pub fn new(x: i16, y: i16, line_height: usize) -> Cursor { Cursor { pt: Pt { x, y }, line_height } }
}

impl TextView {
    pub fn new(canvas: Gid, bounds_hint: TextBounds) -> Self {
        TextView {
            canvas,
            operation: TextOp::Nop,
            untrusted: true,
            token: None,
            invert: false,
            clip_rect: None,
            bounds_hint,
            bounds_computed: None,
            style: TEXTVIEW_DEFAULT_STYLE,
            text: String::new(),
            cursor: Cursor::new(0, 0, 0),
            insertion: None,
            ellipsis: false,
            draw_border: true,
            border_width: 1,
            rounded_border: None,
            margin: Point { x: 4, y: 4 },
            selected: None,
            clear_area: true,
            overflow: None,
            dry_run: false,
            busy_animation_state: None,
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    /// draws a textview
    DrawTextView, //(TextView),

    Quit,
}

pub fn draw_textview(conn: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    let mut buf = crate::Buffer::into_buf(tv);
    buf.lend_mut(conn, Opcode::DrawTextView.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

    let tvr = buf.to_original::<TextView, _>().unwrap();
    tv.bounds_computed = tvr.bounds_computed;
    tv.cursor = tvr.cursor;
    tv.overflow = tvr.overflow;
    tv.busy_animation_state = tvr.busy_animation_state;
    Ok(())
}
