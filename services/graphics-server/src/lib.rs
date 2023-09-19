#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{
    Circle, ClipObject, ClipObjectType, DrawStyle, Gid, Line, PixelColor, Point, Rectangle,
    RoundedRectangle, TextBounds, TextOp, TextView, TokenClaim, ClipRect, Cursor, GlyphStyle, ClipObjectList
};
#[cfg(feature="ditherpunk")]
pub use api::Tile;
pub mod op;

pub mod fontmap;
pub use fontmap::*;

use api::Opcode; // if you prefer to map the api into your local namespace
use num_traits::ToPrimitive;
use xous::{send_message, Message};
use xous_ipc::Buffer;

pub use api::ArchivedBulkRead;
pub use api::BulkRead;
#[derive(Debug)]
pub struct Gfx {
    conn: xous::CID,
}
impl Gfx {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(api::SERVER_NAME_GFX)
            .expect("Can't connect to GFX");
        Ok(Gfx { conn })
    }
    pub fn conn(&self) -> xous::CID {
        self.conn
    }

    pub fn draw_line(&self, line: Line) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Line.to_usize().unwrap(),
                line.start.into(),
                line.end.into(),
                line.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn draw_circle(&self, circ: Circle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Circle.to_usize().unwrap(),
                circ.center.into(),
                circ.radius as usize,
                circ.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn draw_rectangle(&self, rect: Rectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::Rectangle.to_usize().unwrap(),
                rect.tl.into(),
                rect.br.into(),
                rect.style.into(),
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn draw_rounded_rectangle(&self, rr: RoundedRectangle) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::RoundedRectangle.to_usize().unwrap(),
                rr.border.tl.into(),
                rr.border.br.into(),
                rr.border.style.into(),
                rr.radius as _,
            ),
        )
        .map(|_| ())
    }

    pub fn flush(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::Flush.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn draw_sleepscreen(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::DrawSleepScreen.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn draw_boot_logo(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::DrawBootLogo.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }

    pub fn screen_size(&self) -> Result<Point, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ScreenSize.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("ScreenSize message failed");
        if let xous::Result::Scalar2(x, y) = response {
            Ok(Point::new(x as _, y as _))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn glyph_height_hint(&self, glyph: GlyphStyle) -> Result<usize, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::QueryGlyphProps.to_usize().unwrap(),
                glyph as usize,
                0,
                0,
                0,
            ),
        )
        .expect("QueryGlyphProps failed");
        if let xous::Result::Scalar2(_, h) = response {
            Ok(h)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn draw_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        let mut buf = Buffer::into_buf(*tv).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::DrawTextView.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;

        let tvr = buf.to_original::<TextView, _>().unwrap();
        tv.bounds_computed = tvr.bounds_computed;
        tv.cursor = tvr.cursor;
        tv.overflow = tvr.overflow;
        Ok(())
    }

    pub fn draw_line_clipped(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::Line(line),
        };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    // for use in the deface operation
    pub fn draw_line_clipped_xor(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::XorLine(line),
        };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    pub fn draw_circle_clipped(&self, circ: Circle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::Circ(circ),
        };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    pub fn draw_rectangle_clipped(
        &self,
        rect: Rectangle,
        clip: Rectangle,
    ) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::Rect(rect),
        };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    pub fn draw_rounded_rectangle_clipped(
        &self,
        rr: RoundedRectangle,
        clip: Rectangle,
    ) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::RoundRect(rr),
        };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    #[cfg(feature="ditherpunk")]
    pub fn draw_tile_clipped(
        &self,
        tile: Tile,
        clip: Rectangle,
    ) -> Result<(), xous::Error> {
        let co = ClipObject {
            clip,
            obj: ClipObjectType::Tile(tile),
        };
        log::info!("ClipObject size: {}", core::mem::size_of::<ClipObject>());
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap())
            .map(|_| ())
    }

    pub fn draw_object_list_clipped(
        &self,
        list: ClipObjectList,
    ) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(list).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObjectList.to_u32().unwrap())
            .map(|_| ())
    }

    /// this is a one-way door, once you've set it, you can't unset it.
    pub fn set_devboot(&self, enable: bool) -> Result<(), xous::Error> {
        let ena = if enable { 1 } else { 0 };
        send_message(
            self.conn,
            Message::new_scalar(Opcode::Devboot.to_usize().unwrap(), ena, 0, 0, 0),
        )
        .map(|_| ())
    }

    /// instead of implementing the read in the library, we had the raw opcode to the caller
    /// this allows the caller to re-use the bulk read data structure across multiple reads
    /// instead of it being re-allocated and re-init'd every single call
    pub fn bulk_read_fontmap_op(&self) -> u32 {
        Opcode::BulkReadFonts.to_u32().unwrap()
    }
    /// the bulk read auto-increments a pointer on the gfx server, so this message is necessary
    /// to reset the pointer to 0.
    pub fn bulk_read_restart(&self) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::RestartBulkRead.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't reset bulk read");
    }

    pub fn selftest(&self, duration_ms: usize) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::TestPattern.to_usize().unwrap(), duration_ms, 0, 0, 0),
        )
        .expect("couldn't self test");
    }

    pub fn stash(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(Opcode::Stash.to_usize().unwrap(), 0, 0, 0, 0)
            )
            .expect("couldn't stash");
        } else {
            send_message(
                self.conn,
                Message::new_scalar(Opcode::Stash.to_usize().unwrap(), 0, 0, 0, 0)
            )
            .expect("couldn't stash");
        }
    }

    pub fn pop(&self, blocking: bool) {
        if blocking {
            send_message(
                self.conn,
                Message::new_blocking_scalar(Opcode::Pop.to_usize().unwrap(), 0, 0, 0, 0)
            )
            .expect("couldn't pop");
        } else {
            send_message(
                self.conn,
                Message::new_scalar(Opcode::Pop.to_usize().unwrap(), 0, 0, 0, 0)
            )
            .expect("couldn't pop");
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Gfx {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
