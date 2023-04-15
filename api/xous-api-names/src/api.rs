mod rkyv_enum;
pub use rkyv_enum::*;

#[allow(dead_code)]
pub const AUTHENTICATE_TIMEOUT: u32 = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(C)]
pub enum Opcode {
    /// Create a new server with the given name and return its SID.
    Register = 0,

    /// Create a connection to the target server.
    Lookup = 1,

    /// Create an authenticated connection to the target server.
    AuthenticatedLookup = 2,

    /// unregister a server, given its cryptographically unique SID.
    Unregister = 3,

    /// disconnect, given a server name and a cryptographically unique, one-time use token
    Disconnect = 4,

    /// indicates if all inherently trusted slots have been occupied. Should not run untrusted code until this is the case.
    TrustedInitDone = 5,

    /// Connect to a Server, blocking if the Server does not exist. When the Server is started,
    /// return with either the CID or an AuthenticationRequest
    ///
    /// # Message Types
    ///
    ///     * MutableLend
    ///
    /// # Arguments
    ///
    /// The memory being pointed to should be a &str, and the length of the string should
    /// be specified in the `valid` field.
    ///
    /// # Return Values
    ///
    /// Memory is overwritten to contain a return value.  This return value can be defined
    /// as the following enum:
    ///
    /// ```rust
    /// #[repr(C)]
    /// #[non_exhaustive]
    /// enum ConnectResult {
    ///     Success(xous::CID /* connection ID */, [u32; 4] /* Disconnection token */),
    ///     Error(u32 /* error code */),
    ///     Unhandled, /* Catchall for future Results */
    /// }
    /// ```
    BlockingConnect = 6,

    /// Connect to a Server, returning the connection ID or an authentication request if
    /// it exists, and returning ServerNotFound if it does not exist.
    ///
    /// # Message Types
    ///
    ///     * MutableLend
    ///
    /// # Arguments
    ///
    /// The memory being pointed to should be a &str, and the length of the string should
    /// be specified in the `valid` field.
    ///
    /// # Return Values
    ///
    /// Memory is overwritten to contain a return value.  This return value can be defined
    /// as the following enum:
    ///
    /// ```rust
    /// #[repr(C)]
    /// #[non_exhaustive]
    /// enum ConnectResult {
    ///     Success(xous::CID /* connection ID */, [u32; 4] /* Disconnection token */),
    ///     Error(u32 /* error code */),
    ///     Unhandled, /* Catchall for future Results */
    /// }
    /// ```
    TryConnect = 7,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Registration {
    pub name: xous_ipc::String<64>,
    pub conn_limit: Option<u32>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Disconnect {
    pub name: xous_ipc::String<64>,
    pub token: [u32; 4],
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AuthenticatedLookup {
    pub name: xous_ipc::String<64>,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub response: [u32; 8],
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[repr(C)]
pub struct AuthenticateRequest {
    pub name: xous_ipc::String<64>, // a copy of the originally requested lookup
    pub pubkey_id: [u8; 20],        // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 4],
}

//////////////////////////////////////////////////////////////////////////////////////////////
// We keep XousServerName around because want to be able to index off the server name, without
// burdening the Kernel String type with the Hash32 methods
//////////////////////////////////////////////////////////////////////////////////////////////

// --------------------- Taken from rkyv docs https://docs.rs/rkyv/0.3.0/rkyv/trait.Archive.html //
#[derive(Debug, Copy, Clone)]
pub struct XousServerName {
    value: [u8; 64],
    length: usize,
}

impl Default for XousServerName {
    fn default() -> Self {
        XousServerName {
            value: [0u8; 64],
            length: 0,
        }
    }
}

#[allow(dead_code)]
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
        s.length = s.value.len().min(src.as_bytes().len());

        // If the string is not valid, set its length to 0.
        if s.as_str().is_err() {
            s.length = 0;
        }
        assert!(s.length < s.value.len(), "incorrect length derivation!");

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
        self.length = b.len();
        assert!(
            self.length < self.value.len(),
            "incorrect length derivation!"
        );

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

impl std::hash::Hash for XousServerName {
    fn hash<H>(&self, state: &mut H)
    where
        H: std::hash::Hasher,
    {
        assert!(self.length < self.value.len(), "incorret length on hash!");
        std::hash::Hash::hash(&self.value[..self.length as usize], state);
        std::hash::Hash::hash(&self.length, state)
    }
}

impl PartialEq for XousServerName {
    fn eq(&self, other: &Self) -> bool {
        assert!(self.length < self.value.len(), "incorret length on Eq!");
        assert!(
            other.length < other.value.len(),
            "incorrect length on Eq (other)!"
        );
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
