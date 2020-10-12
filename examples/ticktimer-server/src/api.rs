use xous::{Message, ScalarMessage};

#[derive(Debug)]
pub enum Opcode {
    /// Reset the timer
    Reset,
    /// Get the elapsed time in milliseconds
    ElapsedMs,
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Reset),
                2 => Ok(Opcode::ElapsedMs),
                _ => Err("unrecognized opcode"),
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::Reset => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::ElapsedMs => Message::Scalar(ScalarMessage {
                id: 2,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
        }
    }
}
