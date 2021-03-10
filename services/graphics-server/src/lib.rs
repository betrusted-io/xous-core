#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{Circle, DrawStyle, Line, PixelColor, Point, Rectangle, TextView, TextBounds, Gid, TextOp, RoundedRectangle, ClipObject, ClipObjectType};
use blitstr_ref as blitstr;
pub use blitstr::{ClipRect, Cursor, GlyphStyle};
use xous::String;
pub mod op;

use xous::{send_message, CID};
use core::fmt::Write;
use rkyv::Write as RkyvWrite;
use rkyv::Unarchive;


pub fn draw_line(cid: CID, line: Line) -> Result<(), xous::Error> {
    let m: xous::Message = api::Opcode::Line(line).into();
    //log::info!("GFX|LIB: api encoded line as {:?}", m);
    send_message(cid, m).map(|_| ())
}

pub fn draw_circle(cid: CID, circ: Circle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Circle(circ).into()).map(|_| ())
}

pub fn draw_rectangle(cid: CID, rect: Rectangle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Rectangle(rect).into()).map(|_| ())
}

pub fn draw_rounded_rectangle(cid: CID, rr: RoundedRectangle) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::RoundedRectangle(rr).into()).map(|_| ())
}

pub fn flush(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Flush.into()).map(|_| ())
}

pub fn draw_sleepscreen(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::DrawSleepScreen.into()).map(|_| ())
}

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
pub fn set_string_clipping(cid: CID, r: ClipRect) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetStringClipping(r).into()).map(|_| ())
}

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
pub fn set_cursor(cid: CID, c: Cursor) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetCursor(c).into()).map(|_| ())
}

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
pub fn get_cursor(cid: CID) -> Result<Cursor, xous::Error> {
    let response = send_message(cid, api::Opcode::GetCursor.into())?;
    if let xous::Result::Scalar2(pt_as_usize, h) = response {
        let p: Point = pt_as_usize.into();
        Ok(Cursor::new(p.x as _, p.y as _, h as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
pub fn draw_string(cid: CID, s: &String<4096>) -> Result<(), xous::Error> {
    let mut clone_s: String<4096> = String::new();
    write!(clone_s, "{}", s.as_str().unwrap()).map_err(|_| xous::Error::AccessDenied)?;
    let request = api::Opcode::String(clone_s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    use rkyv::Write;
    let pos = writer.archive(&request).expect("GFX: couldn't archive String request");
    let xous_buffer = writer.into_inner();

    //log::info!("GFX: draw_string message being sent");
    xous_buffer.lend(cid, pos as u32).expect("GFX: String request failure");
    //log::info!("GFX: draw_string completed");
    Ok(())
}

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
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

#[deprecated(
    note = "Please use draw_textview for atomic text updates"
)]
pub fn query_glyph(cid: CID) -> Result<(GlyphStyle, usize), xous::Error> {
    let response = send_message(cid, api::Opcode::QueryGlyphStyle.into())?;
    if let xous::Result::Scalar2(glyph, h) = response {
        Ok((GlyphStyle::from(glyph), h))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn glyph_height_hint(cid: CID, glyph: GlyphStyle) -> Result<usize, xous::Error> {
    let response = send_message(cid, api::Opcode::QueryGlyphProps(glyph).into())?;
    if let xous::Result::Scalar2(_, h) = response {
        Ok(h)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn draw_textview(cid: CID, tv: &mut TextView) -> Result<(), xous::Error> {
    let rkyv_tv = api::Opcode::DrawTextView(*tv);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("couldn't archive textview");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(cid, pos as u32).expect("draw_textview operation failure");

    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::DrawTextView(result) = returned {
        let tvr: TextView = result.unarchive();
        //log::info!("draw_textview: got cursor of {:?}, bounds of {:?}", tvr.cursor, tvr.bounds_computed);
        tv.bounds_computed = tvr.bounds_computed;
        tv.cursor = tvr.cursor;
        Ok(())
    } else {
        let tvr = returned.unarchive();
        log::info!("draw_textview saw an unhandled return type of {:?}", tvr);
        Err(xous::Error::InternalError)
    }
}


pub fn draw_line_clipped(cid: xous::CID, line: Line, clip: Rectangle) -> Result<(), xous::Error> {
    let rkyv = api::Opcode::DrawClipObject(
        ClipObject {
            clip: clip,
            obj: ClipObjectType::Line(line),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GFX_API: couldn't archive ClipObject");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("GFX_API: ClipObject operation failure");

    Ok(())
}

pub fn draw_circle_clipped(cid: xous::CID, circ: Circle, clip: Rectangle) -> Result<(), xous::Error> {
    let rkyv = api::Opcode::DrawClipObject(
        ClipObject {
            clip: clip,
            obj: ClipObjectType::Circ(circ),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GFX_API: couldn't archive ClipObject");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("GFX_API: ClipObject operation failure");

    Ok(())
}

pub fn draw_rectangle_clipped(cid: xous::CID, rect: Rectangle, clip: Rectangle) -> Result<(), xous::Error> {
    let rkyv = api::Opcode::DrawClipObject(
        ClipObject {
            clip: clip,
            obj: ClipObjectType::Rect(rect),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GFX_API: couldn't archive ClipObject");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("GFX_API: ClipObject operation failure");

    Ok(())
}

pub fn draw_rounded_rectangle_clipped(cid: xous::CID, rr: RoundedRectangle, clip: Rectangle) -> Result<(), xous::Error> {
    let rkyv = api::Opcode::DrawClipObject(
        ClipObject {
            clip: clip,
            obj: ClipObjectType::RoundRect(rr),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GFX_API: couldn't archive ClipObject");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("GFX_API: ClipObject operation failure");

    Ok(())
}