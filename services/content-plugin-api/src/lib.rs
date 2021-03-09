#![cfg_attr(target_os = "none", no_std)]

use xous::{CID, send_message, Message, ScalarMessage};

#[derive(Debug)]
pub enum Opcode {
    /// request a redraw of our canvas
    Redraw,
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::Redraw),
                _ => Err("SHCH api: unknown Scalar ID"),
            },
            _ => Err("SHCH api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::Redraw => Message::Scalar(ScalarMessage {
                id: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0,
            }),
            // _ => panic!("SHCH api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}

pub trait ContentCanvasApi {
    fn redraw_canvas(&self) -> Result<(), xous::Error>;
}

pub struct ContentCanvasConnection {
    pub connection: Option<CID>,
}

impl ContentCanvasApi for ContentCanvasConnection {
    fn redraw_canvas(&self) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => send_message(cid, Opcode::Redraw.into()).map(|_| ()),
            _ => Err(xous::Error::UseBeforeInit),
        }
    }
}

