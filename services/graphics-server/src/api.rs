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
pub mod cursor;
pub use cursor::*;

#[cfg(feature = "ditherpunk")]
pub mod tile;
use std::hash::{Hash, Hasher};

#[cfg(feature = "ditherpunk")]
pub use tile::*;

pub const LINES: i16 = 536;
pub const WIDTH: i16 = 336;

//////////////// IPC APIs
#[derive(Debug, Copy, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Gid {
    /// a 128-bit random identifier for graphical objects
    gid: [u32; 4],
}
impl Gid {
    pub fn new(id: [u32; 4]) -> Self { Gid { gid: id } }

    pub fn gid(&self) -> [u32; 4] { self.gid }
}
impl Hash for Gid {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Hash::hash(&self.gid[..], state);
    }
}

pub const SERVER_NAME_GFX: &str = "_Graphics_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// Flush the buffer to the screen
    Flush,

    /// Clear the buffer to "light" colored pixels
    Clear,

    /// Draw a line at the specified area
    Line, //(Line),

    /// Draw a rectangle or square at the specified coordinates
    Rectangle, //(Rectangle),

    /// Draw a rounded rectangle
    RoundedRectangle, //(RoundedRectangle),

    /// Paint a Bitmap Tile
    #[cfg(feature = "ditherpunk")]
    Tile,

    /// Draw a circle with a specified radius
    Circle, //(Circle),

    /// Retrieve the X and Y dimensions of the screen
    ScreenSize,

    /// gets info about the current glyph to assist with layout
    QueryGlyphProps, //(GlyphStyle),

    /// draws a textview
    DrawTextView, //(TextView),

    /// draws an object that requires clipping
    DrawClipObject, //(ClipObject),
    DrawClipObjectList,

    /// draws the sleep screen; assumes requests are vetted by GAM/xous-names
    DrawSleepScreen,

    /// permanently turns on the Devboot mark
    Devboot,

    /// bulk read for signature verifications
    BulkReadFonts,
    RestartBulkRead,

    /// sling the framebuffer into and out of the suspend/resume area, abusing this
    /// to help accelerate redraws between modal swaps.
    Stash,
    Pop,

    /// generates a test pattern
    TestPattern,

    /// SuspendResume callback
    #[cfg(not(feature = "cramium-soc"))]
    SuspendResume,

    /// draw the boot logo (for continuity as apps initialize)
    DrawBootLogo,

    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum ClipObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
    XorLine(Line),
    #[cfg(feature = "ditherpunk")]
    Tile(Tile),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ClipObject {
    pub clip: Rectangle,
    pub obj: ClipObjectType,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ClipObjectList {
    // ClipObject is 28 bytes, so 32 of these takes 896 bytes, which is less than a 4k page (the minimum
    // amount that gets remapped) we limit the length to 32 so we can use the Default initializer to set
    // the None's on the array, otherwise it gets a bit painful.
    pub list: [Option<ClipObject>; 32],
    free: usize,
}
impl ClipObjectList {
    pub fn default() -> ClipObjectList { ClipObjectList { list: Default::default(), free: 0 } }

    pub fn push(&mut self, item: ClipObjectType, clip: Rectangle) -> Result<(), ClipObjectType> {
        if self.free < self.list.len() {
            self.list[self.free] = Some(ClipObject { clip, obj: item });
            self.free += 1;
            Ok(())
        } else {
            Err(item)
        }
    }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct TokenClaim {
    pub token: Option<[u32; 4]>,
    pub name: String,
}

/// the buffer length of this equal to the internal length passed by the
/// engine-sha512 implementation times 2 (a small amount of overhead is required
/// out of an even 4096 page for bookkeeping). We could make this a neat power of 2,
/// but then you'd end up doing an extra memory message for the overhead bits that
/// are left over.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct BulkRead {
    pub buf: [u8; 7936],
    pub from_offset: u32,
    pub len: u32, // used to return the length read out of the font map
}
impl BulkRead {
    pub fn default() -> BulkRead { BulkRead { buf: [0; 7936], from_offset: 0, len: 7936 } }
}
