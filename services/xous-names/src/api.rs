#![allow(dead_code)]

use hash32::{Hash, Hasher};
use xous::CID;

use core::cmp::Eq;

// bottom 16 bits are reserved for structure re-use by other servers
pub const ID_REGISTER_NAME: u32 = 0x1_0000;
pub const ID_LOOKUP_NAME: u32 = 0x2_0000;
pub const ID_AUTHENTICATE: u32 = 0x3_0000;

pub const AUTHENTICATE_TIMEOUT: u32 = 10_000; // time in ms that a process has to respond to an authentication request

#[derive(rkyv::Archive, Debug)]
pub(crate) enum Request {
    /// Create a new server with the given name and return its SID.
    Register(XousServerName),

    /// Create a connection to the target server.
    Lookup(XousServerName),

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

#[derive(Debug, Default, rkyv::Archive)]
pub(crate) struct Lookup {
    pub name: XousServerName,
}

#[derive(Debug, Default, rkyv::Archive)]
pub(crate) struct AuthenticatedLookup {
    pub name: XousServerName,
    pub pubkey_id: [u8; 20], // 160-bit pubkey ID encoded in network order (big endian)
    pub challenge: [u32; 8],
}

impl core::convert::TryFrom<&str> for XousServerName {
    type Error = xous::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut s = XousServerName::default();
        use core::fmt::Write;
        write!(s, "{}", value).map_err(|_| xous::Error::AccessDenied)?;
        Ok(s)
    }
}
// impl Lookup {
//     pub fn new() -> Self {
//         Lookup {
//             cid: 0,
//             name: Default::default(),
//             authenticate_request: false,
//             pubkey_id: [0; 20],
//             challenge: [0; 8],
//         }
//     }
// }

#[derive(Debug)]
pub struct Authenticate {
    pub name: XousServerName,
    pub success: bool,
    pub response_to_challenge: [u32; 8],
}

impl Authenticate {
    pub fn mid(&self) -> u32 {
        ID_AUTHENTICATE as u32
    }
}

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

pub struct ArchivedXousServerName {
    // This will be a relative pointer to the bytes of our string.
    ptr: rkyv::RelPtr,
    // The length of the archived version must be explicitly sized for
    // 32/64-bit compatibility. Archive is not implemented for usize and
    // isize to help you avoid making this mistake.
    len: u32,
}

impl ArchivedXousServerName {
    // This will help us get the bytes of our type as a str again.
    pub fn as_str(&self) -> &str {
        unsafe {
            // The as_ptr() function of RelPtr will get a pointer
            // to its memory.
            let bytes = core::slice::from_raw_parts(self.ptr.as_ptr(), self.len as usize);
            core::str::from_utf8_unchecked(bytes)
        }
    }
}

pub struct XousServerNameResolver {
    // This will be the position that the bytes of our string are stored at.
    // We'll use this to make the relative pointer of our ArchivedXousServerName.
    bytes_pos: usize,
}

impl rkyv::Resolve<XousServerName> for XousServerNameResolver {
    // This is essentially the output type of the resolver. It must match
    // the Archived associated type in our impl of Archive for XousServerName.
    type Archived = ArchivedXousServerName;

    // The resolve function consumes the resolver and produces the archived
    // value at the given position.
    fn resolve(self, pos: usize, value: &XousServerName) -> Self::Archived {
        Self::Archived {
            // We have to be careful to add the offset of the ptr field,
            // otherwise we'll be using the position of the ArchivedXousServerName
            // instead of the position of the ptr. That's the reason why
            // RelPtr::new is unsafe.
            ptr: unsafe {
                rkyv::RelPtr::new(
                    pos + rkyv::offset_of!(ArchivedXousServerName, ptr),
                    self.bytes_pos,
                )
            },
            len: value.length,
        }
    }
}

impl rkyv::Archive for XousServerName {
    type Archived = ArchivedXousServerName;
    /// This is the resolver we'll return from archive.
    type Resolver = XousServerNameResolver;

    fn archive<W: rkyv::Write + ?Sized>(&self, writer: &mut W) -> Result<Self::Resolver, W::Error> {
        // This is where we want to write the bytes of our string and return
        // a resolver that knows where those bytes were written.
        let bytes_pos = writer.pos();
        writer.write(&self.value[0..(self.length as usize)])?;
        Ok(Self::Resolver { bytes_pos })
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

// Allow a `&XousServerName` to be printed out
impl core::fmt::Display for ArchivedXousServerName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Allow a `&XousServerName` to be used anywhere that expects a `&str`
impl AsRef<str> for ArchivedXousServerName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl PartialEq for ArchivedXousServerName {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ArchivedXousServerName {}

impl hash32::Hash for ArchivedXousServerName {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Hash::hash(&self.as_str(), state)
    }
}

impl rkyv::Unarchive<XousServerName> for ArchivedXousServerName {
    fn unarchive(&self) -> XousServerName {
        let mut s: XousServerName = Default::default();
        unsafe {
            let p = self.ptr.as_ptr() as *const u8;
            for (i, val) in s.value.iter_mut().enumerate() {
                *val = p.add(i).read();
            }
        };
        s.length = self.len;
        s
    }
}
