#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{Circle, DrawStyle, Line, PixelColor, Point, Rectangle};
pub use blitstr::{ClipRect, Cursor, GlyphStyle};
use xous::String;
pub mod op;

use xous::{send_message, CID};

pub fn draw_line(cid: CID, line: Line) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Line(line).into()).map(|_| ())
}

pub fn draw_circle(cid: CID, circ: Circle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Circle(circ).into()).map(|_| ())
}

pub fn draw_rectangle(cid: CID, rect: Rectangle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Rectangle(rect).into()).map(|_| ())
}

pub fn flush(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Flush.into()).map(|_| ())
}

pub fn set_string_clipping(cid: CID, r: ClipRect) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetStringClipping(r).into()).map(|_| ())
}

pub fn set_cursor(cid: CID, c: Cursor) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetCursor(c).into()).map(|_| ())
}

pub fn get_cursor(cid: CID) -> Result<Cursor, xous::Error> {
    let response = send_message(cid, api::Opcode::GetCursor.into())?;
    if let xous::Result::Scalar2(pt_as_usize, h) = response {
        let p: Point = pt_as_usize.into();
        Ok(Cursor::new(p.x as usize, p.y as usize, h as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn draw_string(cid: CID, s: &String) -> Result<(), xous::Error> {
    s.lend(cid, 1).map(|_| ())
}

pub fn set_glyph_style(cid: CID, glyph: GlyphStyle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetGlyphStyle(glyph).into()).map(|_| ())
}

pub fn screen_size(cid: CID) -> Result<Point, xous::Error> {
    let response = send_message(cid, api::Opcode::ScreenSize.into())?;
    if let xous::Result::Scalar2(x, y) = response {
        Ok(Point::new(x as _, y as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn query_glyph(cid: CID) -> Result<(GlyphStyle, usize), xous::Error> {
    let response = send_message(cid, api::Opcode::QueryGlyphStyle.into())?;
    if let xous::Result::Scalar2(glyph, h) = response {
        Ok((GlyphStyle::from(glyph), h))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}
