#![cfg_attr(target_os = "none", no_std)]

// pub mod size;
pub mod api;
pub use api::{Point, Color, Rect};
use xous::String;
pub mod op;
pub mod fonts;

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
    send_message(cid, api::Opcode::ClearRegion(Rect::new(x0 as _, y0 as _, x1 as _, y1 as _)).into()).map(|_| ())
}

pub fn draw_string(cid: CID, s: &String) -> Result<(), xous::Error> {
    s.lend(cid, 1).map(|_| ())
}

pub fn screen_size(cid: CID) -> Result<Point, xous::Error> {
    let response = send_message(cid, api::Opcode::ScreenSize.into())?;
    if let xous::Result::Scalar2(x, y) = response {
        Ok(Point::new(x as _, y as _))
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}