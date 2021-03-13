use crate::{Error, MemoryMessage, Result, CID};

use core::pin::Pin;
use rkyv::archived_value;
use rkyv::Unarchive;
use rkyv::Write;

#[derive(Copy, Clone)]
pub struct String<const N: usize> {
    bytes: [u8; N],
    len: u32, // length in bytes, not characters
}

impl<const N: usize> String<N> {
    pub fn new() -> String<N> {
        String {
            bytes: [0; N],
            len: 0,
        }
    }

    pub fn from_str(src: &str) -> String<N> {
        let mut s = Self::new();
        // Copy the string into our backing store.
        for (&src_byte, dest_byte) in src.as_bytes().iter().zip(&mut s.bytes) {
            *dest_byte = src_byte;
        }
        // Set the string length to the length of the passed-in String,
        // or the maximum possible length. Which ever is smaller.
        s.len = s.bytes.len().min(src.as_bytes().len()) as u32;

        // If the string is not valid, set its length to 0.
        if s.as_str().is_err() {
            s.len = 0;
        }

        s
    }

    pub fn as_bytes(&self) -> [u8; N] {
        self.bytes
    }

    pub fn as_str(&self) -> core::result::Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(&self.bytes[0..self.len as usize])
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Convert a `MemoryMessage` into a `String`
    pub fn from_message(
        message: &mut MemoryMessage,
    ) -> core::result::Result<String<N>, core::str::Utf8Error> {
        let buf = unsafe { crate::XousBuffer::from_memory_message(message) };
        let bytes = Pin::new(buf.as_ref());
        let value = unsafe { archived_value::<String<N>>(&bytes, message.id as usize) };
        let s = value.unarchive();
        Ok(s)
    }

    /// Perform an immutable lend of this String to the specified server.
    /// This function will block until the server returns.
    /// Note that this convenience should only be used if the server only ever
    /// expects to deal with one type of String, ever. Otherwise, this should be
    /// implemented in the API and wrapped in an Enum to help decorate the functional
    /// target of the string. An example of a server that uses this convencience function
    /// is the logger.
    pub fn lend(
        &self,
        connection: CID,
        // id: crate::MessageId,
    ) -> core::result::Result<Result, Error> {
        let mut writer = rkyv::ArchiveBuffer::new(crate::XousBuffer::new(N));
        let pos = writer
            .archive(self)
            .expect("xous::String -- couldn't archive self");
        let xous_buffer = writer.into_inner();

        // note that "id" is actually used as the position into the rkyv buffer
        xous_buffer.lend(connection, pos as u32)
    }

    /// Move this string from the client into the server.
    pub fn send(
        self,
        connection: CID,
        // id: crate::MessageId,
    ) -> core::result::Result<Result, Error> {
        let mut writer =
            rkyv::ArchiveBuffer::new(crate::XousBuffer::new(/*self.bytes.len()*/ 4096));
        let pos = writer
            .archive(&self)
            .expect("xous::String -- couldn't archive self");
        let xous_buffer = writer.into_inner();

        xous_buffer.send(connection, pos as u32)
    }

    /// Clear the contents of this String and set the length to 0
    pub fn clear(&mut self) {
        self.len = 0;
        self.bytes = [0; N];
    }

    pub fn to_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[0..self.len()]) }
    }

    /// awful, O(N) implementation because we have to iterate through the entire string
    /// and decode variable-length utf8 characters, until we can't.
    pub fn pop(&mut self) -> Option<char> {
        if self.len() < 1 {
            return None;
        }
        // first, make a copy of the string
        let tempbytes: [u8; N] = self.bytes;
        let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.len()]) }.clone();
        // clear our own string
        self.len = 0;
        self.bytes = [0; N];

        // now copy over character by character, until just before the last character
        let mut char_iter = tempstr.chars();
        let mut maybe_c = char_iter.next();
        loop {
            match maybe_c {
                Some(c) => {
                    let next_c = char_iter.next();
                    match next_c {
                        Some(_thing) => {
                            self.push(c).unwrap(); // always succeeds because we're re-encoding our string
                            maybe_c = next_c;
                        }
                        None => return next_c,
                    }
                }
                None => {
                    return None; // we should actually never get here because len() == 0 case already covered
                }
            }
        }
    }

    pub fn push(&mut self, ch: char) -> core::result::Result<usize, Error> {
        match ch.len_utf8() {
            1 => {
                if self.len() < self.bytes.len() {
                    self.bytes[self.len()] = ch as u8;
                    self.len += 1;
                    Ok(1)
                } else {
                    Err(Error::OutOfMemory)
                }
            }
            _ => {
                let mut bytes: usize = 0;
                let mut data: [u8; 4] = [0; 4];
                let subslice = ch.encode_utf8(&mut data);
                if self.len() + subslice.len() < self.bytes.len() {
                    for c in subslice.bytes() {
                        self.bytes[self.len()] = c;
                        self.len += 1;
                        bytes += 1;
                    }
                    Ok(bytes)
                } else {
                    Err(Error::OutOfMemory)
                }
            }
        }
    }

    pub fn append(&mut self, s: &str) -> core::result::Result<usize, Error> {
        let mut bytes_added = 0;
        for ch in s.chars() {
            if let Ok(bytes) = self.push(ch) {
                bytes_added += bytes;
            } else {
                return Err(Error::OutOfMemory);
            }
        }
        Ok(bytes_added)
    }

    pub fn push_byte(&mut self, b: u8) -> core::result::Result<(), Error> {
        if self.len() < self.bytes.len() {
            self.bytes[self.len()] = b;
            self.len += 1;
            Ok(())
        } else {
            Err(Error::OutOfMemory)
        }
    }
}

impl<const N: usize> core::fmt::Display for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl<const N: usize> core::fmt::Write for String<N> {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        for c in s.bytes() {
            if self.len() < self.bytes.len() {
                self.bytes[self.len()] = c;
                self.len += 1;
            }
        }
        Ok(())
    }
}

impl<const N: usize> core::fmt::Debug for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

#[repr(C)]
pub struct ArchivedString {
    ptr: rkyv::RelPtr,
    len: u32,
}

impl ArchivedString {
    // Provide a `str` view of an `ArchivedString`.
    fn as_str(&self) -> &str {
        unsafe {
            // The as_ptr() function of RelPtr will get a pointer
            // to its memory.
            let bytes = core::slice::from_raw_parts(self.ptr.as_ptr(), self.len as usize);
            core::str::from_utf8_unchecked(bytes)
        }
    }
}

pub struct StringResolver {
    bytes_pos: usize,
}

// Turn a stream of bytes into an `ArchivedString`.
impl<const N: usize> rkyv::Resolve<String<N>> for StringResolver {
    type Archived = ArchivedString;

    fn resolve(self, pos: usize, value: &String<N>) -> Self::Archived {
        Self::Archived {
            ptr: unsafe {
                rkyv::RelPtr::new(pos + rkyv::offset_of!(ArchivedString, ptr), self.bytes_pos)
            },
            len: value.len() as u32,
        }
    }
}

/// Turn a `String` into an archived object
impl<const N: usize> rkyv::Archive for String<N> {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn archive<W: rkyv::Write + ?Sized>(
        &self,
        writer: &mut W,
    ) -> core::result::Result<Self::Resolver, W::Error> {
        let bytes_pos = writer.pos();
        writer.write(&self.bytes[0..self.len()])?;
        Ok(Self::Resolver { bytes_pos })
    }
}

impl<const N: usize> rkyv::Unarchive<String<N>> for ArchivedString {
    fn unarchive(&self) -> String<N> {
        String::from_str(self.as_str())
    }
}
