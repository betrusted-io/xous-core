#![cfg_attr(target_os = "none", no_std)]

use xous::{Message, ScalarMessage};
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub struct Prediction {
    pub index: u32,
    pub string: xous::String<4096>,
}

////////////////////////////////////////////////
/////////////// TODO: refactor this into a multi-crate API, so that we an inheret this API across multiple implementations
////////////////////////////////////////////////

#[derive(Debug, Default, Copy, Clone)]
pub struct PredictionTriggers {
    /// trigger predictions on newline
    pub trigger_newline: bool,
    /// trigger predictions punctuation
    pub trigger_punctuation: bool,
    /// trigger predictions on whitespace
    pub trigger_whitespace: bool,
}
impl Into<usize> for PredictionTriggers {
    fn into(self) -> usize {
        let mut ret: usize = 0;
        if self.trigger_newline { ret |= 0x1; }
        if self.trigger_punctuation { ret |= 0x2; }
        if self.trigger_whitespace { ret |= 0x4; }
        ret
    }
}
impl From<usize> for PredictionTriggers {
    fn from(code: usize) -> PredictionTriggers {
        PredictionTriggers {
            trigger_newline: (code & 0x1) != 0,
            trigger_punctuation: (code & 0x2) != 0,
            trigger_whitespace: (code & 0x4) != 0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum Opcode {
    /// update with the latest input candidate. Replaces the previous input.
    Input(xous::String<4096>),

    /// feed back to the IME plugin as to what was picked, so predictions can be updated
    Picked(xous::String<4096>),

    /// Undo the last Picked value. To be used when a user hits backspace after picking a prediction
    /// note that repeated calls to Unpick will have an implementation-defined behavior
    Unpick,

    /// fetch the prediction at a given index, where the index is ordered from 0..N, where 0 is the most likely prediction
    /// if there is no prediction available, just return an empty string
    Prediction(Prediction),

    /// return the prediction triggers used by this IME. These are characters that can indicate that a
    /// whole predictive unit has been entered.
    GetPredictionTriggers,
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::Unpick),
                _ => Err("IME_SH api: unknown Scalar ID"),
            },
            Message::BlockingScalar(m) => match m.id {
                1 => Ok(Opcode::GetPredictionTriggers),
                _ => Err("IME_SH api: unknown BlockingScalar ID"),
            },
            _ => Err("IME_SH api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::Unpick => Message::Scalar(ScalarMessage {
                id: 0,
                arg1: 0, arg2: 0, arg3: 0, arg4: 0,
            }),
            Opcode::GetPredictionTriggers => Message::BlockingScalar(ScalarMessage {
                id: 1,
                arg1: 0, arg2: 0, arg3: 0, arg4: 0,
            }),
            _ => panic!("IME_SH api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}
