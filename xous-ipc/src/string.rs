use core::pin::Pin;

use rkyv::ArchiveUnsized;
use rkyv::SerializeUnsized;
use rkyv::archived_value;
use rkyv::ser::Serializer;
use xous::{CID, Error, MemoryMessage, Result};

#[derive(Copy, Clone)]
pub struct String<const N: usize> {
    bytes: [u8; N],
    len: u32, // length in bytes, not characters
}

impl<const N: usize> String<N> {
    pub fn new() -> String<N> { String { bytes: [0; N], len: 0 } }

    // use a volatile write to ensure a clear operation is not optimized out
    // for ensuring that a string is cleared, e.g. at the exit of a function
    pub fn volatile_clear(&mut self) {
        let b = self.bytes.as_mut_ptr();
        for i in 0..N {
            unsafe {
                b.add(i).write_volatile(core::mem::zeroed());
            }
        }
        self.len = 0; // also set my length to 0
        // Ensure the compiler doesn't re-order the clear.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    pub fn from_str<T>(src: T) -> String<N>
    where
        T: AsRef<str>,
    {
        let src = src.as_ref();
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

    pub fn as_bytes(&self) -> [u8; N] { self.bytes }

    pub fn as_str(&self) -> core::result::Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(&self.bytes[0..self.len as usize])
    }

    pub fn len(&self) -> usize { self.len as usize }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Convert a `MemoryMessage` into a `String`
    pub fn from_message(message: &MemoryMessage) -> core::result::Result<String<N>, core::str::Utf8Error> {
        let buf = unsafe { crate::Buffer::from_memory_message(message) };
        let bytes = Pin::new(buf.as_ref());
        let value = unsafe { archived_value::<String<N>>(&bytes, message.id as usize) };
        let s = String::<N>::from_str(value.as_str());
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
        let mut writer = rkyv::ser::serializers::BufferSerializer::new(crate::Buffer::new(N));
        let pos = writer.serialize_value(self).expect("xous::String -- couldn't archive self");
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
        let mut writer = rkyv::ser::serializers::BufferSerializer::new(crate::Buffer::new(N));
        let pos = writer.serialize_value(&self).expect("xous::String -- couldn't archive self");
        let xous_buffer = writer.into_inner();

        xous_buffer.send(connection, pos as u32)
    }

    /// Clear the contents of this String and set the length to 0
    pub fn clear(&mut self) {
        self.len = 0;
        self.bytes = [0; N];
    }

    pub fn to_str(&self) -> &str { unsafe { core::str::from_utf8_unchecked(&self.bytes[0..self.len()]) } }

    /// awful, O(N) implementation because we have to iterate through the entire string
    /// and decode variable-length utf8 characters, until we can't.
    pub fn pop(&mut self) -> Option<char> {
        if self.is_empty() {
            return None;
        }
        // first, make a copy of the string
        let tempbytes: [u8; N] = self.bytes;
        let tempstr = &(*unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.len()]) });
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

impl<const N: usize> Default for String<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> core::fmt::Display for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
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
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
}

impl<const N: usize> core::convert::AsRef<str> for String<N> {
    fn as_ref(&self) -> &str { self.to_str() }
}

impl<const N: usize> PartialEq for String<N> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes[..self.len as usize] == other.bytes[..other.len as usize] && self.len == other.len
    }
}

impl<const N: usize> Eq for String<N> {}

#[repr(C)]
pub struct ArchivedString {
    ptr: rkyv::RelPtr<str>,
}

impl ArchivedString {
    // Provide a `str` view of an `ArchivedString`.
    pub fn as_str(&self) -> &str {
        unsafe {
            // The as_ptr() function of RelPtr will get a pointer
            // to its memory.
            &*self.ptr.as_ptr()
        }
    }
}

pub struct StringResolver {
    bytes_pos: usize,
    _metadata_resolver: rkyv::MetadataResolver<str>,
}

/// Turn a `String` into an archived object
impl<const N: usize> rkyv::Archive for String<N> {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
        Self::Archived {
            ptr: unsafe {
                self.as_str().unwrap().resolve_unsized(
                    pos + rkyv::offset_of!(Self::Archived, ptr),
                    resolver.bytes_pos,
                    (),
                )
            },
        }
    }
}

// Turn a stream of bytes into an `ArchivedString`.
impl<S: rkyv::ser::Serializer + ?Sized, const N: usize> rkyv::Serialize<S> for String<N> {
    fn serialize(&self, serializer: &mut S) -> core::result::Result<Self::Resolver, S::Error> {
        // This is where we want to write the bytes of our string and return
        // a resolver that knows where those bytes were written.
        // We also need to serialize the metadata for our str.
        Ok(StringResolver {
            bytes_pos: self.as_str().unwrap().serialize_unsized(serializer)?,
            _metadata_resolver: self.as_str().unwrap().serialize_metadata(serializer)?,
        })
    }
}
// Turn an `ArchivedString` back into a String
use rkyv::Fallible;
impl<D: Fallible + ?Sized, const N: usize> rkyv::Deserialize<String<N>, D> for ArchivedString {
    fn deserialize(&self, _deserializer: &mut D) -> core::result::Result<String<N>, D::Error> {
        Ok(String::<N>::from_str(self.as_str()))
    }
}

// a "no-op" deserializer for in-place data, required for rkyv 0.4.1
// perhaps in rkyv 0.5 this will go away, see https://github.com/djkoloski/rkyv/issues/67
pub struct XousDeserializer;
impl rkyv::Fallible for XousDeserializer {
    type Error = xous::Error;
}
