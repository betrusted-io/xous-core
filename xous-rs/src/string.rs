use crate::{CID, Error, MemoryMessage, Result};

use rkyv::Write;
use rkyv::Unarchive;
use rkyv::archived_value;
use core::pin::Pin;

#[derive(Copy, Clone)]
pub struct String<const N: usize> {
    bytes: [u8; N],
    len: u32,
}

impl<const N: usize> String<N> {
    pub fn new() -> String<N> {
        String {
            bytes: [0; N],
            len: 0,
        }
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
        message: & mut MemoryMessage,
    ) -> core::result::Result<String<N>, core::str::Utf8Error> {
        let buf = unsafe{ crate::XousBuffer::from_memory_message(message) };
        let bytes = Pin::new(buf.as_ref());
        let value = unsafe {
            archived_value::<String<N>>(&bytes, message.id as usize)
        };
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

        let mut writer = rkyv::ArchiveBuffer::new(crate::XousBuffer::new( N ));
        let pos = writer.archive(self).expect("xous::String -- couldn't archive self");
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
        let mut writer = rkyv::ArchiveBuffer::new(crate::XousBuffer::new(/*self.bytes.len()*/ 4096));
        let pos = writer.archive(&self).expect("xous::String -- couldn't archive self");
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

pub struct ArchivedString {
    ptr: rkyv::RelPtr,
    len: u32,
}
pub struct StringResolver {
    bytes_pos: usize,
}
impl<const N: usize> rkyv::Resolve<String<N>> for StringResolver {
    type Archived = ArchivedString;

    fn resolve(self, pos: usize, value: &String<N>) -> Self::Archived {
        Self::Archived {
            ptr: unsafe {
                rkyv::RelPtr::new(
                pos + rkyv::offset_of!(ArchivedString, ptr),
                self.bytes_pos)
            },
            len: value.len() as u32,
        }
    }
}

impl<const N: usize> rkyv::Archive for String<N> {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn archive<W: rkyv::Write + ?Sized>(&self, writer: &mut W) -> core::result::Result<Self::Resolver, W::Error> {
        let bytes_pos = writer.pos();
        writer.write(&self.bytes[0..self.len()])?;
        Ok(Self::Resolver { bytes_pos })
    }
}
impl<const N: usize> rkyv::Unarchive<String<N>> for ArchivedString {
    fn unarchive(&self) -> String<N> {
        let mut s: String<N> = String::<N>::new();
        unsafe {
            let p = self.ptr.as_ptr() as *const u8;
            for(i, val) in s.bytes.iter_mut().enumerate() {
                *val = p.add(i).read();
            }
        };
        s.len = self.len;
        s
    }
}
