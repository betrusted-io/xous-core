use xous::{SID, CID};
use hash32_derive::*;

use core::cmp::Eq;

pub const ID_REGISTER_NAME: usize = 0;
pub const ID_LOOKUP_NAME: usize = 1;
pub const ID_AUTHENTICATE: usize = 2;

pub const AUTHENTICATE_TIMEOUT: usize = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(Hash32, Debug, Copy, Clone)]
pub struct XousServerName{pub name: [u8; 32]}
impl Default for XousServerName {
    fn default() -> Self { XousServerName{name: [0; 32]} }
}
impl PartialEq for XousServerName {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for XousServerName {}

#[derive(Debug)]
pub struct Registration {
    mid: usize,
    pub sid: SID, // query: do we even want to return this to the registering process??

    // we use fixed-length, u8-only records to pass server names. This is different from
    // your typical Rust String object; but, a key restriction on IPC calls is that the size
    // of structures must be (1) statically known and (2) contain no references. Therefore
    // the name is a pre-allocated, 64-length u8 array, and the *entire* array is the name,
    // including characters after the first "null"
    pub name: XousServerName,
    pub success: bool,
}

impl Registration {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        Registration {
            mid: ID_REGISTER_NAME,
            sid: xous::SID::from_u32(0,0,0,0),
            name: XousServerName::default(),
            success: false,
        }
    }
}

#[derive(Debug)]
pub struct Lookup {
    mid: usize,
    pub cid: CID,
    pub name: XousServerName,
    pub success: bool,
    pub authenticate_request: bool,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 8],
}

impl Lookup {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        Lookup {
            mid: ID_LOOKUP_NAME,
            cid: 0,
            name: XousServerName::default(),
            success: false,
            authenticate_request: false,
            pubkey_id: [0; 20],
            challenge: [0; 8],
        }
    }
}

#[derive(Debug)]
pub struct Authenticate {
    mid: usize,
    pub cid: CID,
    pub name: XousServerName,
    pub success: bool,
    pub response_to_challenge: [u32; 8],
}

impl Authenticate {
    pub fn mid(&self) -> usize { self.mid }

    pub fn new() -> Self {
        Authenticate {
            mid: ID_AUTHENTICATE,
            cid: 0,
            name: XousServerName::default(),
            success: false,
            response_to_challenge: [0; 8],
        }
    }
}
