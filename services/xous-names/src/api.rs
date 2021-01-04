use xous::{Message, ScalarMessage, SID};

pub const ID_REGISTER_NAME: usize = 0;
pub const ID_LOOKUP_NAME: usize = 1;
pub const ID_AUTHENTICATE: usize = 2;

pub const AUTHENTICATE_TIMEOUT: usize = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(Debug)]
pub struct Registration {
    mid: usize,
    pub sid: SID,
    // we use fixed-length, u8-only records to pass server names. This is different from
    // your typical Rust String object; but, a key restriction on IPC calls is that the size
    // of structures must be (1) statically known and (2) contain no references. Therefore
    // the name is a pre-allocated, 64-length u8 array, and the length of the name is explicitly encoded.
    pub name: [u8; 64],
    pub name_len: usize,
    pub success: bool,
}

impl Registration {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        Registration {
            mid: ID_REGISTER_NAME,
            sid: xous::SID::from_u32(0,0,0,0),
            name: [0; 64],
            name_len: 0,
            success: false,
        }
    }
}

#[derive(Debug)]
pub struct Lookup {
    mid: usize,
    pub cid: xous::CID,
    pub name: [u8; 64],
    pub name_len: usize,
    pub success: bool,
    pub authenticate_request: bool,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 8],
}

impl Default for Lookup {
    fn default() -> Self {
        Lookup {
            mid: ID_LOOKUP_NAME,
            cid: 0,
            name: [0; 64],
            name_len: 0,
            success: false,
            authenticate_request: false,
            pubkey_id: [0; 20],
            challenge: [0; 8],
        }
    }
}
impl Lookup {
    pub fn mid(&self) -> usize { self.mid }
}

#[derive(Debug)]
pub struct Authenticate {
    mid: usize,
    pub cid: xous::CID,
    pub name: [u8; 64],
    pub name_len: usize,
    pub success: bool,
    pub response_to_challenge: [u32; 8],
}

impl Default for Authenticate {
    fn default() -> Self {
        Authenticate {
            mid: ID_AUTHENTICATE,
            cid: 0,
            name: [0; 64],
            name_len: 0,
            success: false,
            response_to_challenge: [0; 8],
        }
    }
}

impl Authenticate {
    pub fn mid(&self) -> usize { self.mid }
}
