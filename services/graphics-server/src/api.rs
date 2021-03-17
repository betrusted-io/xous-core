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

//////////////// primitives
pub mod points;
pub use points::Point;
pub mod styles;
pub use styles::*;
pub mod shapes;
pub use shapes::*;
pub mod text;
pub use text::*;

use blitstr_ref as blitstr;
use blitstr::{ClipRect, Cursor, GlyphStyle};

use xous::{Message, ScalarMessage};
use hash32::{Hash, Hasher};

//////////////// OS APIs

#[derive(Debug, Copy, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Gid {
    /// a 128-bit random identifier for graphical objects
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

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Opcode {
    /// Flush the buffer to the screen
    Flush,

    /// Clear the buffer to "light" colored pixels
    Clear,

    /// Draw a line at the specified area
    Line(Line),

    /// Draw a rectangle or square at the specified coordinates
    Rectangle(Rectangle),

    /// Draw a rounded rectangle
    RoundedRectangle(RoundedRectangle),

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
    String(xous::String<4096>),

    /// Xor the string inside the clipping region.
    StringXor(xous::String<4096>),

    /// Simulate the string on the clipping region (for computing text widths)
    SimulateString(xous::String<4096>),

    /// Retrieve the X and Y dimensions of the screen
    ScreenSize,

    /// Retrieve the current Glyph style
    QueryGlyphStyle,

    /// gets info about the current glyph to assist with layout
    QueryGlyphProps(GlyphStyle),

    /// draws a textview
    DrawTextView(TextView),

    /// draws an object that requires clipping
    DrawClipObject(ClipObject),

    /// draws the sleep screen; assumes requests are vetted by GAM/xous-names
    DrawSleepScreen
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
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
                15 => Ok(Opcode::RoundedRectangle(RoundedRectangle::new(Rectangle::new_with_style(
                    Point::from(m.arg1),
                    Point::from(m.arg2),
                    DrawStyle::from(m.arg3)),
                    m.arg4 as _
                ))),
                16 => Ok(Opcode::DrawSleepScreen),
                _ => Err("unrecognized opcode"),
            },
            Message::BlockingScalar(m) => match m.id {
                8 => Ok(Opcode::ScreenSize),
                10 => Ok(Opcode::QueryGlyphStyle),
                13 => Ok(Opcode::GetCursor),
                14 => Ok(Opcode::QueryGlyphProps(GlyphStyle::from(m.arg1))),
                _ => Err("unrecognized opcode"),
            },
            /*Message::MutableBorrow(m) => match m.id {
                0x100 => {
                    let tv: &mut TextView = unsafe {
                        &mut *(m.buf.as_mut_ptr() as *mut TextView)
                    };
                    Ok(Opcode::TextView(tv))
                },
                _ => Err("unrecognized opcode"),
            }*/
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
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
            Opcode::SetCursor(c) => Message::Scalar(ScalarMessage {
                id: 12,
                arg1: c.pt.x as usize,
                arg2: c.pt.y as usize,
                arg3: c.line_height as usize,
                arg4: 0,
            }),
            Opcode::GetCursor => Message::BlockingScalar(ScalarMessage {
                id: 13,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::QueryGlyphProps(glyph) => Message::BlockingScalar(ScalarMessage {
                id: 14,
                arg1: glyph as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::RoundedRectangle(rr) => Message::Scalar(ScalarMessage {
                id: 15,
                arg1: rr.border.tl.into(),
                arg2: rr.border.br.into(),
                arg3: rr.border.style.into(),
                arg4: rr.radius as _,
            }),
            Opcode::DrawSleepScreen => Message::Scalar(ScalarMessage {
                id: 16, arg1: 0, arg2: 0, arg3: 0, arg4: 0,
            }),
            _ => panic!("GFX api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}


#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum ClipObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ClipObject {
    pub clip: Rectangle,
    pub obj: ClipObjectType,
}
