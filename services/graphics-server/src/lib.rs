#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{Circle, DrawStyle, Line, PixelColor, Point, Rectangle, TextView, TextBounds, Gid, TextOp, RoundedRectangle, ClipObject, ClipObjectType};
use blitstr_ref as blitstr;
pub use blitstr::{ClipRect, Cursor, GlyphStyle};
pub mod op;

use api::Opcode; // if you prefer to map the api into your local namespace
use xous::{send_message, Message};
use xous_ipc::Buffer;
use num_traits::ToPrimitive;

pub struct Gfx {
    conn: xous::CID,
}
impl Gfx {
    pub fn new(xns: xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_GFX).expect("Can't connect to GFX");
        Ok(Gfx {
            conn,
        })
    }

    pub fn draw_line(&self, line: Line) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Line.to_usize().unwrap(),
                line.start.into(),
                line.end.into(),
                line.style.into(),
                0
        )).map(|_|())
    }

    pub fn draw_circle(&self, circ: Circle) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Circle.to_usize().unwrap(),
            circ.center.into(),
            circ.radius as usize,
            circ.style.into(),
            0
        )).map(|_|())
    }

    pub fn draw_rectangle(&self, rect: Rectangle) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Rectangle.to_usize().unwrap(),
            rect.tl.into(),
            rect.br.into(),
            rect.style.into(),
            0,
        )).map(|_|())
    }

    pub fn draw_rounded_rectangle(&self, rr: RoundedRectangle) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RoundedRectangle.to_usize().unwrap(),
            rr.border.tl.into(),
            rr.border.br.into(),
            rr.border.style.into(),
            rr.radius as _,
        )).map(|_|())
    }

    pub fn flush(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Flush.to_usize().unwrap(), 0, 0, 0, 0,
        )).map(|_|())
    }

    pub fn draw_sleepscreen(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::DrawSleepScreen.to_usize().unwrap(), 0, 0, 0, 0,
        )).map(|_|())
    }

    pub fn screen_size(&self) -> Result<Point, xous::Error> {
        let response = send_message(self.conn,
            Message::new_scalar(Opcode::ScreenSize.to_usize().unwrap(), 0, 0, 0, 0,
        )).expect("ScreenSize message failed");
        if let xous::Result::Scalar2(x, y) = response {
            Ok(Point::new(x as _, y as _))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn glyph_height_hint(&self, glyph: GlyphStyle) -> Result<usize, xous::Error> {
        let response = send_message(self.conn,
            Message::new_scalar(Opcode::QueryGlyphProps.to_usize().unwrap(), glyph as usize, 0, 0, 0,
        )).expect("QueryGlyphProps failed");
        if let xous::Result::Scalar2(_, h) = response {
            Ok(h)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn draw_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        let mut buf = Buffer::into_buf(*tv).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::DrawTextView.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        let tvr = buf.to_original::<TextView, _>().unwrap();
        tv.bounds_computed = tvr.bounds_computed;
        tv.cursor = tvr.cursor;
        Ok(())
    }


    pub fn draw_line_clipped(&self, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Line(line) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    pub fn draw_circle_clipped(&self, circ: Circle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Circ(circ) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    pub fn draw_rectangle_clipped(&self, rect: Rectangle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::Rect(rect) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }

    pub fn draw_rounded_rectangle_clipped(&self, rr: RoundedRectangle, clip: Rectangle) -> Result<(), xous::Error> {
        let co = ClipObject { clip, obj: ClipObjectType::RoundRect(rr) };
        let buf = Buffer::into_buf(co).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DrawClipObject.to_u32().unwrap()).map(|_| ())
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}