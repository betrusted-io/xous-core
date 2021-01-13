use xous::{Message, ScalarMessage};

#[allow(dead_code)]
#[derive(Debug)]
pub enum Opcode {
    /// Get one or two 32-bit words of TRNG data
    GetTrng(usize),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::BlockingScalar(m) => match m.id {
                0 => Ok(Opcode::GetTrng(m.arg1)),
                _ => Err("TRNG api: unknown BlockingScalar ID"),
            },
            _ => Err("TRNG api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::GetTrng(count) => Message::BlockingScalar(ScalarMessage {
                id: 0,
                arg1: count.into(),
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            // _ => panic!("TRNG api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}
