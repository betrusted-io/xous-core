use hash32::{Hash, Hasher};

pub const AUTHENTICATE_TIMEOUT: u32 = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Request {
    /// Create a new server with the given name and return its SID.
    Register,

    /// Create a connection to the target server.
    Lookup,

    /// Create an authenticated connection to the target server.
    AuthenticatedLookup,

    // Return values

    /// The caller must perform an AuthenticatedLookup using this challenge
    AuthenticateRequest,

    /// The connection failed for some reason
    Failure,

    /// A server was successfully created with the given SID
    SID,

    /// A connection was successfully made with the given CID
    CID,
}
/*
pub(crate) enum Request {
    /// Create a new server with the given name and return its SID.
    Register(xous_ipc::String::<64>),

    /// Create a connection to the target server.
    Lookup(xous_ipc::String::<64>),

    /// Create an authenticated connection to the target server.
    AuthenticatedLookup(AuthenticatedLookup),

    // Return values

    /// The caller must perform an AuthenticatedLookup using this challenge
    AuthenticateRequest(AuthenticatedRequest),

    /// The connection failed for some reason
    Failure,

    /// A server was successfully created with the given SID
    SID([u32; 4]),

    /// A connection was successfully made with the given CID
    CID(CID),
}*/

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct AuthenticatedLookup {
    pub name: xous_ipc::String::<64>,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 8],
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct AuthenticatedRequest {
    pub request: AuthenicatedLookup,  // a copy of the original request. We don't trust it, but it's helpful to have for reference
    pub nonce: [u32; 4],
    pub response: [u32; 8],
}


//////////////////////////////////////////////////////////////////////////////////////////////
// We keep XousServerName around because want to be able to index off the server name, without
// burdening the Kernel String type with the Hash32 methods
//////////////////////////////////////////////////////////////////////////////////////////////

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
    pub fn new() -> XousServerName {
        XousServerName {
            value: [0; 64],
            length: 0,
        }
    }

    pub fn from_str(src: &str) -> XousServerName {
        let mut s = Self::new();
        // Copy the string into our backing store.
        for (&src_byte, dest_byte) in src.as_bytes().iter().zip(&mut s.value) {
            *dest_byte = src_byte;
        }
        // Set the string length to the length of the passed-in String,
        // or the maximum possible length. Which ever is smaller.
        s.length = s.value.len().min(src.as_bytes().len()) as u32;

        // If the string is not valid, set its length to 0.
        if s.as_str().is_err() {
            s.length = 0;
        }

        s
    }

    pub fn as_bytes(&self) -> [u8; 64] {
        self.value
    }

    pub fn as_str(&self) -> core::result::Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(&self.value[0..self.length as usize])
    }

    pub fn len(&self) -> usize {
        self.length as usize
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Clear the contents and set the length to 0
    pub fn clear(&mut self) {
        self.length = 0;
        self.value = [0; 64];
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
