#![allow(dead_code)]

use xous::{SID, CID};
use hash32::{Hash, Hasher};

use core::cmp::Eq;

pub const ID_REGISTER_NAME: usize = 0;
pub const ID_LOOKUP_NAME: usize = 1;
pub const ID_AUTHENTICATE: usize = 2;

pub const AUTHENTICATE_TIMEOUT: usize = 10_000; // time in ms that a process has to respond to an authentication request

//////////////////////// handle throwing strings across IPC boundary with hash comparison

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct XousServerName {
    pub name: [u8; 64],
    pub length: usize,
}

impl hash32::Hash for XousServerName {
    fn hash<H>(&self, state: &mut H)
    where
    H: Hasher,
    {
        Hash::hash(&self.name[..], state);
        Hash::hash(&self.length, state)
    }
}

impl XousServerName {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn to_str(&self) -> &str {
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.name.as_ptr(), self.length)
        })
        .unwrap()
    }
}

impl Default for XousServerName {
    fn default() -> Self {
        XousServerName {
            name: [0; 64],
            length: 0,
        }
    }
}
impl PartialEq for XousServerName {
    fn eq(&self, other: &Self) -> bool {
        self.name[..self.length] == other.name[..other.length] && self.length == other.length
    }
}

impl Eq for XousServerName {}

// Allow using the `write!()` macro to write into a `&XousServerName`
impl core::fmt::Write for XousServerName {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        self.length = 0;
        let b = s.bytes();

        // Ensure the length is acceptable
        if b.len() > self.name.len() {
            Err(core::fmt::Error)?;
        }
        self.length = b.len();

        // Copy the string into this variable
        for (dest, src) in self.name.iter_mut().zip(s.bytes()) {
            *dest = src;
        }

        // Attempt to convert the string to UTF-8 to validate it's correct UTF-8
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.name.as_ptr(), self.length)
        })
        .map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

// Allow a `&XousServerName` to be printed out
impl core::fmt::Display for XousServerName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

// Allow a `&XousServerName` to be used anywhere that expects a `&str`
impl AsRef<str> for XousServerName {
    fn as_ref(&self) -> &str {
        self.to_str()
    }
}

//////////////////////// end server name string implementation functions



#[derive(Debug)]
#[repr(C)]
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
            sid: SID::from_u32(0,0,0,0),
            name: XousServerName::default(),
            success: false,
        }
    }
}

#[derive(Debug)]
#[repr(C)]
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
#[repr(C)]
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
