use xous::{Message, ScalarMessage};

#[derive(Debug)]
pub enum Opcode {
    /// Reset the timer
    // Reset,  // This is a bad idea, in retrospect.

    /// Get the elapsed time in milliseconds
    ElapsedMs,

    /// Sleep for the specified numer of milliseconds
    SleepMs(usize),

    /// Recalculate the sleep time
    RecalculateSleep,
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                // 1 => Ok(Opcode::Reset),
                131072 => Ok(Opcode::RecalculateSleep),
                _ => Err("unrecognized opcode"),
            },
            Message::BlockingScalar(m) => match m.id {
                4919 => Ok(Opcode::ElapsedMs),
                3 => Ok(Opcode::SleepMs(m.arg1)),
                _ => Err("unrecognized opcode"),
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            /* Opcode::Reset => Message::Scalar(ScalarMessage {
                id: 1,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),*/
            Opcode::RecalculateSleep => Message::Scalar(ScalarMessage {
                id: 131072,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::ElapsedMs => Message::BlockingScalar(ScalarMessage {
                id: 4919,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::SleepMs(ms) => Message::BlockingScalar(ScalarMessage {
                id: 3,
                arg1: ms,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
        }
    }
}
