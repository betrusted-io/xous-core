use xous::{Message, ScalarMessage};

use graphics_server::Gid;

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum Opcode {
    /// informs me where my canvas is
    SetCanvas(Gid),

    /// set prediction. Must be a String of the name of a server that is loaded in the system.
    SetPrediction(xous::String<256>),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::SetCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
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
