#![cfg_attr(target_os = "none", no_std)]

use xous::{Message, ScalarMessage, String, CID};
use graphics_server::Gid;

#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub struct Prediction {
    pub index: u32,
    pub valid: bool,
    // to *return* a value in rkyv, we can't have a variable-length string, as the "pos" argument changes
    // this is OK for predictions, we only show the first few characters anyways.
    pub string: [u8; 31], // 31 is the longest fixed-length array rkyv supports
    pub len: u32,
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

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum Opcode {
    /// update with the latest input candidate. Replaces the previous input.
    Input(xous::String<4000>),

    /// feed back to the IME plugin as to what was picked, so predictions can be updated
    Picked(xous::String<4000>),

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

pub trait PredictionApi {
    fn get_prediction_triggers(&self) -> Result<PredictionTriggers, xous::Error>;
    fn unpick(&self) -> Result<(), xous::Error>;
    fn set_input(&self, s: String<4000>) -> Result<(), xous::Error>;
    fn feedback_picked(&self, s: String<4000>) -> Result<(), xous::Error>;
    fn get_prediction(&self, index: u32) -> Result<Option<xous::String<4000>>, xous::Error>;
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
                let response = xous::send_message(cid, Opcode::GetPredictionTriggers.into())?;
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
                xous::send_message(cid, Opcode::Unpick.into())?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_input(&self, s: String<4000>) -> Result<(), xous::Error> {
        use rkyv::Write;
        match self.connection {
            Some(cid) => {
                let rkyv_input = Opcode::Input(s);
                let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
                let pos = writer.archive(&rkyv_input).expect("IME|API: couldn't archive input string");
                let xous_buffer = writer.into_inner();

                xous_buffer.lend(cid, pos as u32).expect("IME|API: set_input operation failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }

    fn feedback_picked(&self, s: String<4000>) -> Result<(), xous::Error> {
        use rkyv::Write;
        match self.connection {
            Some(cid) => {
                let rkyv_picked = Opcode::Picked(s);
                let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
                let pos = writer.archive(&rkyv_picked).expect("IME|API: couldn't archive picked string");
                let xous_buffer = writer.into_inner();

                xous_buffer.lend(cid, pos as u32).expect("IME|API: feedback_picked operation failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit),
        }
    }

    fn get_prediction(&self, index: u32) -> Result<Option<xous::String<4000>>, xous::Error> {
        use rkyv::Write;
        use rkyv::Unarchive;
        let debug1 = false;
        match self.connection {
            Some(cid) => {
                let prediction = Prediction {
                    index,
                    string: [0; 31],
                    len: 0,
                    valid: false,
                };
                let pred_op = Opcode::Prediction(prediction);
                let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
                let pos = writer.archive(&pred_op).expect("IME|API: couldn't archive prediction request");
                let mut xous_buffer = writer.into_inner();
                if debug1{log::info!("IME|API: lending Prediction with pos {}", pos);}

                xous_buffer.lend_mut(cid, pos as u32).expect("IME|API: prediction fetch operation failure");

                if debug1{log::info!("IME|API: returned from get_prediction");}
                let returned = unsafe { rkyv::archived_value::<Opcode>(xous_buffer.as_ref(), pos)};
                if let rkyv::Archived::<Opcode>::Prediction(result) = returned {
                    let pred_r: Prediction = result.unarchive();
                    if debug1{log::info!("IME|API: got {:?}", pred_r);}
                    if pred_r.valid {
                        let mut ret = xous::String::<4000>::new();
                        use core::fmt::Write as CoreWrite;
                        write!(ret, "{}", core::str::from_utf8(&pred_r.string[0..pred_r.len as usize]).unwrap()).unwrap();
                        Ok(Some(ret))
                    } else {
                        Ok(None)
                    }
                } else {
                    let r = returned.unarchive();
                    log::error!("IME: API get_prediction returned an invalid result {:?}", r);
                    Err(xous::Error::InvalidString)
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

#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum ImefOpcode {
    /// informs me where my input canvas is
    SetInputCanvas(Gid),

    /// informs me where my prediction canvas is
    SetPredictionCanvas(Gid),

    /// set prediction server. Must be a String of the name of a server that is loaded in the system.
    SetPredictionServer(xous::String<256>),

    /// register a listener for finalized inputs
    RegisterListener(xous::String<256>),

    /// this is the event opcode used by listeners
    GotInputLine(xous::String<4000>),
}

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
}

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
                xous::send_message(cid, ImefOpcode::SetInputCanvas(g).into())?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_prediction_canvas(&self, g: graphics_server::Gid) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                xous::send_message(cid, ImefOpcode::SetPredictionCanvas(g).into())?;
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn set_predictor(&self, servername: &str) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let mut server = xous::String::<256>::new();
                use core::fmt::Write;
                write!(server, "{}", servername).expect("IMEF: couldn't write set_predictor server name");
                let ime_op = ImefOpcode::SetPredictionServer(server);

                use rkyv::Write as ArchiveWrite;
                let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
                let pos = writer.archive(&ime_op).expect("IMEF: couldn't archive SetPredictionServer");
                writer.into_inner().lend(cid, pos as u32).expect("IMEF: SetPredictionServer request failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }

    fn register_listener(&self, servername: &str) -> Result<(), xous::Error> {
        match self.connection {
            Some(cid) => {
                let mut server = xous::String::<256>::new();
                use core::fmt::Write;
                write!(server, "{}", servername).expect("IMEF: couldn't write set_predictor server name");
                let ime_op = ImefOpcode::RegisterListener(server);

                use rkyv::Write as ArchiveWrite;
                let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
                let pos = writer.archive(&ime_op).expect("IMEF: couldn't archive SetPredictionServer");
                writer.into_inner().lend(cid, pos as u32).expect("IMEF: SetPredicitonServer request failure");
                Ok(())
            },
            _ => Err(xous::Error::UseBeforeInit)
        }
    }
}