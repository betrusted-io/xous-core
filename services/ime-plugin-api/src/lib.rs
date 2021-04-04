#![cfg_attr(target_os = "none", no_std)]

use xous::{Message, CID, send_message};
use xous_ipc::{Buffer, String};
use num_traits::ToPrimitive;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Prediction {
    pub index: u32,
    pub valid: bool,
    pub string: String::<1000>,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct PredictionTriggers {
    /// trigger line predictions on newline -- if set, sends the *whole* line to the predictor
    /// if just wanting the last word, set `punctuation = true`
    pub newline: bool,
    /// trigger word predictions punctuation
    pub punctuation: bool,
    /// trigger word predictions on whitespace
    pub whitespace: bool,
}
impl Into<usize> for PredictionTriggers {
    fn into(self) -> usize {
        let mut ret: usize = 0;
        if self.newline { ret |= 0x1; }
        if self.punctuation { ret |= 0x2; }
        if self.whitespace { ret |= 0x4; }
        ret
    }
}
impl From<usize> for PredictionTriggers {
    fn from(code: usize) -> PredictionTriggers {
        PredictionTriggers {
            newline: (code & 0x1) != 0,
            punctuation: (code & 0x2) != 0,
            whitespace: (code & 0x4) != 0,
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    /// update with the latest input candidate. Replaces the previous input.
    Input, //(String<4000>),

    /// feed back to the IME plugin as to what was picked, so predictions can be updated
    Picked, //(String<4000>),

    /// Undo the last Picked value. To be used when a user hits backspace after picking a prediction
    /// note that repeated calls to Unpick will have an implementation-defined behavior
    Unpick,

    /// fetch the prediction at a given index, where the index is ordered from 0..N, where 0 is the most likely prediction
    /// if there is no prediction available, just return an empty string
    Prediction, //(Prediction),

    /// return the prediction triggers used by this IME. These are characters that can indicate that a
    /// whole predictive unit has been entered.
    GetPredictionTriggers,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Return {
    Prediction(Prediction),
    Failure,
}

pub trait PredictionApi {
    fn get_prediction_triggers(&self) -> Result<PredictionTriggers, xous::Error>;
    fn unpick(&self) -> Result<(), xous::Error>;
    fn set_input(&self, s: String<4000>) -> Result<(), xous::Error>;
    fn feedback_picked(&self, s: String<4000>) -> Result<(), xous::Error>;
    fn get_prediction(&self, index: u32) -> Result<Option<String<4000>>, xous::Error>;
}

// provide a convenience version of the API for generic/standard calls
#[derive(Debug, Default, Copy, Clone)]
pub struct PredictionPlugin {
    pub connection: Option<CID>,
}

impl PredictionApi for PredictionPlugin {
    fn get_prediction_triggers(&self) -> Result<PredictionTriggers, xous::Error> {
        match self.connection {
            Some(cid) => {
                let response = send_message(cid,
                    Message::new_blocking_scalar(Opcode::GetPredictionTriggers.to_usize().unwrap(), 0, 0, 0, 0))?;
                if let xous::Result::Scalar1(code) = response {
                    Ok(code.into())
                } else {
                    Err(xous::Error::InternalError)
                }
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }

    fn unpick(&self) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                send_message(cid,
                   Message::new_scalar(Opcode::Unpick.to_usize().unwrap(), 0, 0, 0, 0))?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_input(&self, s: String<4000>) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let buf = Buffer::into_buf(s).or(Err(xous::Error::InternalError))?;
                buf.lend(cid, Opcode::Input.to_u32().unwrap()).expect("|API: set_input operation failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }

    fn feedback_picked(&self, s: String<4000>) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let buf = Buffer::into_buf(s).or(Err(xous::Error::InternalError))?;
                buf.lend(cid, Opcode::Picked.to_u32().unwrap()).expect("|API: feedback_picked operation failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }

    fn get_prediction(&self, index: u32) -> Result<Option<String<4000>>, xous::Error> {
        match self.connection {
            Some(cid) => {
                let prediction = Prediction {
                    index,
                    string: String::<1000>::new(),
                    valid: false,
                };
                let mut buf = Buffer::into_buf(prediction).or(Err(xous::Error::InternalError))?;
                buf.lend_mut(cid, Opcode::Prediction.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

                log::trace!("IME|API: returned from get_prediction");

                match buf.to_original().unwrap() {
                    Return::Prediction(pred) => {
                        log::trace!("|API: got {:?}", pred);
                        if pred.valid {
                            let mut ret = String::<4000>::new();
                            use core::fmt::Write as CoreWrite;
                            write!(ret, "{}", pred.string).unwrap();
                            Ok(Some(ret))
                        } else {
                            Ok(None)
                        }
                    }
                    _ => {
                        log::error!("API get_prediction returned an invalid result");
                        Err(xous::Error::InternalError)
                    }
                }
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }
}


//////////////////////////////////////////////////////
//////////////////// FRONT END API
//////////////////////////////////////////////////////
// Most people won't need to touch this, but it's packaged
// in this crate so we can break circular dependencies
// between the IMEF, GAM, and graphics server

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ImefOpcode {
    /// informs me where my input canvas is
    SetInputCanvas, //(Gid),

    /// informs me where my prediction canvas is
    SetPredictionCanvas, //(Gid),

    /// set prediction server. Must be a String of the name of a server that is loaded in the system.
    SetPredictionServer, //(String<64>),

    /// register a listener for finalized inputs
    RegisterListener, //(String<64>),

    // this is the event opcode used by listeners
    //GotInputLine, //(String<4000>),
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum ImefReturn {
    GotInputLine(String<4000>),
    Failure,
}

/*
impl core::convert::TryFrom<& Message> for ImefOpcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(ImefOpcode::SetInputCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                1 => Ok(ImefOpcode::SetPredictionCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                _ => Err("IMEF api: unknown Scalar ID"),
            },
            _ => Err("IMEF api: unhandled message type"),
        }
    }
}

impl Into<Message> for ImefOpcode {
    fn into(self) -> Message {
        match self {
            ImefOpcode::SetInputCanvas(gid) => Message::Scalar(ScalarMessage {
                id: 0, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            ImefOpcode::SetPredictionCanvas(gid) => Message::Scalar(ScalarMessage {
                id: 1, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            _ => panic!("IMEF api: Opcode type not handled by into()"),
        }
    }
}*/

pub trait ImeFrontEndApi {
    fn set_input_canvas(&self, g: graphics_server::Gid) -> Result<(), xous::Error>;
    fn set_prediction_canvas(&self, g: graphics_server::Gid) -> Result<(), xous::Error>;
    fn set_predictor(&self, servername: &str) -> Result<(), xous::Error>;
    fn register_listener(&self, servername: &str) -> Result<(), xous::Error>;
}

pub struct ImeFrontEnd {
    pub connection: Option<CID>,
}

impl ImeFrontEndApi for ImeFrontEnd {
    fn set_input_canvas(&self, g: graphics_server::Gid) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                send_message(cid,
                    Message::new_scalar(ImefOpcode::SetInputCanvas.to_usize().unwrap(),
                    g.gid()[0] as _, g.gid()[1] as _, g.gid()[2] as _, g.gid()[3] as _
                ))?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_prediction_canvas(&self, g: graphics_server::Gid) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                send_message(cid,
                    Message::new_scalar(ImefOpcode::SetPredictionCanvas.to_usize().unwrap(),
                    g.gid()[0] as _, g.gid()[1] as _, g.gid()[2] as _, g.gid()[3] as _
                ))?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_predictor(&self, servername: &str) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let mut server = String::<64>::new();
                use core::fmt::Write;
                write!(server, "{}", servername).expect("couldn't write set_predictor server name");
                let buf = Buffer::into_buf(server).or(Err(xous::Error::InternalError))?;
                buf.lend(cid, ImefOpcode::SetPredictionServer.to_u32().unwrap()).map(|_|())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn register_listener(&self, servername: &str) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let mut server = String::<64>::new();
                use core::fmt::Write;
                write!(server, "{}", servername).expect("couldn't write set_predictor server name");
                let buf = Buffer::into_buf(server).or(Err(xous::Error::InternalError))?;
                buf.lend(cid, ImefOpcode::RegisterListener.to_u32().unwrap()).map(|_|())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }
}
