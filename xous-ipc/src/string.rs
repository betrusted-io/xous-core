use core::convert::TryInto;
use std::cell::RefCell;

use xous::{
    map_memory, send_message, Error, MemoryFlags, MemoryMessage, MemoryRange, MemorySize, Message, Result,
    CID,
};

#[derive(Clone)]
pub struct String<const N: usize> {
    bytes: [u8; N],
    len: u32, // length in bytes, not characters
    should_drop: RefCell<bool>,
    pages: RefCell<Option<MemoryRange>>,
}
const PAGE_SIZE: usize = 0x1000;

impl<const N: usize> String<N> {
    pub fn new() -> String<N> {
        String { bytes: [0; N], len: 0, pages: RefCell::new(None), should_drop: RefCell::new(true) }
    }

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
        let mut new_str = Self::new();
        let slice = unsafe { core::slice::from_raw_parts(message.buf.as_mut_ptr(), message.buf.len()) };
        new_str.len = u32::from_le_bytes(slice[..4].try_into().unwrap());
        new_str.bytes[..new_str.len as usize].copy_from_slice(&slice[4..4 + new_str.len as usize]);
        *new_str.pages.borrow_mut() = Some(message.buf);
        Ok(new_str)
    }

    fn ensure_pages(&self) {
        let mut page_ref = self.pages.borrow_mut();
        if page_ref.is_none() {
            let flags = MemoryFlags::R | MemoryFlags::W;
            let len_to_page = (N + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);

            // Allocate enough memory to hold the requested data
            let new_mem =
                map_memory(None, None, len_to_page, flags).expect("xous-ipc: OOM in buffer allocation");
            *page_ref = Some(new_mem);
        }
    }

    /// Perform an immutable lend of this String to the specified server.
    /// This function will block until the server returns.
    /// Note that this convenience should only be used if the server only ever
    /// expects to deal with one type of String, ever. Otherwise, this should be
    /// implemented in the API and wrapped in an Enum to help decorate the functional
    /// target of the string. An example of a server that uses this convenience function
    /// is the logger.
    pub fn lend(
        &self,
        connection: CID,
        // id: crate::MessageId,
    ) -> core::result::Result<Result, Error> {
        self.ensure_pages();
        let mut pages = self.pages.borrow_mut().expect("should be allocated");
        // safety: interior representation is valid for all possible values of pages
        let page_slice: &mut [u8] = unsafe { pages.as_slice_mut() };
        page_slice[..4].copy_from_slice(&self.len.to_le_bytes());
        page_slice[4..4 + (self.len as usize)].copy_from_slice(&self.bytes[..self.len as usize]);

        let msg =
            MemoryMessage { id: 0, buf: pages, offset: None, valid: MemorySize::new(self.len as usize) };
        send_message(connection, Message::Borrow(msg))
    }

    /// Move this string from the client into the server.
    pub fn send(
        self,
        connection: CID,
        // id: crate::MessageId,
    ) -> core::result::Result<Result, Error> {
        self.ensure_pages();
        let mut pages = self.pages.borrow_mut().expect("should be allocated");
        // safety: interior representation is valid for all possible values of pages
        let page_slice: &mut [u8] = unsafe { pages.as_slice_mut() };
        page_slice[..4].copy_from_slice(&self.len.to_le_bytes());
        page_slice[4..4 + (self.len as usize)].copy_from_slice(&self.bytes[..self.len as usize]);

        let msg =
            MemoryMessage { id: 0, buf: pages, offset: None, valid: MemorySize::new(self.len as usize) };
        let result = send_message(connection, Message::Move(msg))?;
        *self.should_drop.borrow_mut() = false;
        Ok(result)
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

impl<const N: usize> Drop for String<N> {
    fn drop(&mut self) {
        if *self.should_drop.borrow() {
            if let Some(pages) = self.pages.take() {
                xous::unmap_memory(pages).expect("Buffer: failed to drop memory");
            }
        }
    }
}
