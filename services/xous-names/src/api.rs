use xous::{Message, ScalarMessage, SID};

pub struct Registration {
    pub sid: SID,
    pub name: [u8; 64],
    pub name_len: usize,
    pub success: bool,
    // _padding: [u8; 4096-(core::mem::size_of::<SID>() + 64 + core::mem::size_of::<usize>() + core::mem::size_of::<bool>],
}

pub struct Lookup {
    sid: SID,
    name: [u8; 64],
    name_len: usize,
    success: bool,
    autheticate: bool,
    challenge: [u32; 8],
    response: [u32; 8],
    // _padding: [u8; 4096-(core::mem::size_of::<SID>() + 64 + core::mem::size_of::<usize>() + core::mem::size_of::<bool>],
}

pub const ID_REGISTER_NAME: usize = 0;
pub const ID_LOOKUP_NAME: usize = 1;
pub const ID_AUTHENTICATE: usize = 2;

/*
#[derive(Debug)]
pub enum Opcode {
    /// Register a 128-bit SID with a preferred, unique lookup name
    RegisterName(Registration),

    /// Retrieve the 128-bit SID for a server, given its unique lookup name
    LookupName(Lookup),

    /// Authenticate to the name server, based on nonce embedded in LookupName response
    /// this is always as a response to a lookupname that reqeusts the authentication challenge-response
    Authenticate(Lookup),
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::MutableBorrow(m) => {
                if m.id as u16 == ID_REGISTER_NAME {
                    let registration: Registration = unsafe {
                        &mut *(m.buf.as_mut_ptr() as *mut Registration)
                    };
                    Ok(Opcode::RegisterName(registration))
                }
            }
            _ => Err("unhandled message type"),
        }
    }
}*/

/*  No "Into<>" methods -- we use the ipc::lend() to create messages
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
*/