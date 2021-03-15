#![allow(dead_code)]

use xous::CID;

// bottom 16 bits are reserved for structure re-use by other servers
pub const ID_REGISTER_NAME: u32 = 0x1_0000;
pub const ID_LOOKUP_NAME: u32 = 0x2_0000;
pub const ID_AUTHENTICATE: u32 = 0x3_0000;

pub const AUTHENTICATE_TIMEOUT: u32 = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub(crate) enum Request {
    /// Create a new server with the given name and return its SID.
    Register(xous::String::<64>),

    /// Create a connection to the target server.
    Lookup(xous::String::<64>),

    /// Create an authenticated connection to the target server.
    AuthenticatedLookup(AuthenticatedLookup),

    // Return values

    /// The caller must perform an AuthenticatedLookup using this challenge
    AuthenticateRequest([u32; 8]),

    /// The connection failed for some reason
    Failure,

    /// A server was successfully created with the given SID
    SID([u32; 4]),

    /// A connection was successfully made with the given CID
    CID(CID),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct Lookup {
    pub name: xous::String::<64>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct AuthenticatedLookup {
    pub name: xous::String::<64>,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 8],
}

#[derive(Debug)]
pub struct Authenticate {
    pub name: xous::String::<64>,
    pub success: bool,
    pub response_to_challenge: [u32; 8],
}

impl Authenticate {
    pub fn mid(&self) -> u32 {
        ID_AUTHENTICATE as u32
    }
}
