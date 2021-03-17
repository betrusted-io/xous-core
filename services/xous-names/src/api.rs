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

// We keep XousServerName around because want to be able to index off the server name, without
// burdening the Kernel String type with the Hash32 methods

// --------------------- Taken from rkyv docs https://docs.rs/rkyv/0.3.0/rkyv/trait.Archive.html //
#[derive(Debug, Copy, Clone)]
pub struct XousServerName {
    value: [u8; 64],
    length: u32,
}

impl Default for XousServerName {
    fn default() -> Self {
        XousServerName {
            value: [0u8; 64],
            length: 0,
        }
    }
}

impl XousServerName {
    pub fn to_str(&self) -> &str {
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.value.as_ptr(), self.length as usize)
        })
        .unwrap()
    }
}

// Allow using the `write!()` macro to write into a `&XousServerName`
impl core::fmt::Write for XousServerName {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        self.length = 0;
        let b = s.bytes();

        // Ensure the length is acceptable
        if b.len() > self.value.len() {
            Err(core::fmt::Error)?;
        }
        self.length = b.len() as u32;

        // Copy the string into this variable
        for (dest, src) in self.value.iter_mut().zip(s.bytes()) {
            *dest = src;
        }

        // Attempt to convert the string to UTF-8 to validate it's correct UTF-8
        core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(self.value.as_ptr(), self.length as usize)
        })
        .map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

impl hash32::Hash for XousServerName {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Hash::hash(&self.value[..self.length as usize], state);
        Hash::hash(&self.length, state)
    }
}

impl PartialEq for XousServerName {
    fn eq(&self, other: &Self) -> bool {
        self.value[..self.length as usize] == other.value[..other.length as usize]
            && self.length == other.length
    }
}

impl Eq for XousServerName {}

impl core::fmt::Display for XousServerName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}
