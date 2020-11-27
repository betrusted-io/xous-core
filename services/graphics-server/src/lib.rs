#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{Point, Color, Rect};
pub use blitstr::fonts::GlyphSet;
pub use blitstr::Cursor;
use xous::String;
pub mod op;

use xous::{send_message, CID};

pub fn draw_line(cid: CID, start: Point, end: Point) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Line(start, end).into()).map(|_| ())
}

pub fn draw_circle(cid: CID, center: Point, radius: u16) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Circle(center, radius).into()).map(|_| ())
}

pub fn draw_rectangle(cid: CID, start: Point, end: Point) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Rectangle(start, end).into()).map(|_| ())
}

pub fn set_style(cid: CID, width: u16, stroke: Color, fill: Color) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Style(width, stroke, fill).into()).map(|_| ())
}

pub fn flush(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Flush.into()).map(|_| ())
}

pub fn clear_region(cid: CID, x0: usize, y0: usize, x1: usize, y1: usize) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::ClearRegion(blitstr::Rect::new(x0 as _, y0 as _, x1 as _, y1 as _)).into()).map(|_| ())
}

pub fn clear_rectangle(cid: CID, r: blitstr::Rect) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::ClearRegion(r).into()).map(|_| ())
}

pub fn set_string_clipping(cid: CID, r: blitstr::Rect) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetStringClipping(r).into()).map(|_| ())
}

pub fn set_cursor(cid: CID, c: Cursor) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetCursor(c).into()).map(|_| ())
}

pub fn get_cursor(cid: CID, c: Cursor) -> Result<Cursor, xous::Error> {
    let response = send_message(cid, api::Opcode::GetCursor.into())?;
    if let xous::Result::Scalar2(pt_as_usize, h) = response {
        let p: Point = pt_as_usize.into();
        Ok(Cursor::new(p.x, p.y, h as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn draw_string(cid: CID, s: &String) -> Result<(), xous::Error> {
    s.lend(cid, 1).map(|_| ())
}

pub fn set_glyph(cid: CID, glyph: GlyphSet) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetGlyph(glyph).into()).map( |_| ())
}

pub fn screen_size(cid: CID) -> Result<Point, xous::Error> {
    let response = send_message(cid, api::Opcode::ScreenSize.into())?;
    if let xous::Result::Scalar2(x, y) = response {
        Ok(Point::new(x as _, y as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn query_glyph(cid: CID) -> Result<(GlyphSet, usize), xous::Error> {
    let response = send_message(cid, api::Opcode::QueryGlyph.into())?;
    if let xous::Result::Scalar2(glyph, h) = response {
        Ok((blitstr::fonts::arg_to_glyph(glyph), h))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}