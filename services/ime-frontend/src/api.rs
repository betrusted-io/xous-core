use xous::{Message, ScalarMessage};

use graphics_server::Gid;

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum Opcode {
    /// informs me where my canvas is
    SetCanvas(Gid),

    /// set prediction. Must be a String of the name of a server that is loaded in the system.
    SetPrediction(xous::String),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::GetTrng(m.arg1)),
                _ => Err("IMEF api: unknown Scalar ID"),
            },
            _ => Err("IMEF api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::SetCanvas(gid) => Message::Scalar(ScalarMessage {
                id: 0, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            _ => panic!("IMEF api: Opcode type not handled by into()"),
        }
    }
}
