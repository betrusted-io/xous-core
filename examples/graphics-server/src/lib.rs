#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::{Point, Color};

use xous::{send_message, CID};

pub fn draw_line(cid: CID, start: Point, end: Point) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Line(start, end).into())
}

pub fn draw_circle(cid: CID, center: Point, radius: u32) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Circle(center, radius).into())
}

pub fn draw_rectangle(cid: CID, start: Point, end: Point) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Rectangle(start, end).into())
}

pub fn set_style(cid: CID, width: u32, stroke: Color, fill: Color) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Style(width, stroke, fill).into())
}

pub fn flush(cid: CID) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::Flush.into())
}
