use crate::{
    map_memory, send_message, unmap_memory, Error, MemoryMessage, MemoryRange, MemorySize, Message, Result,
    CID,
};

/// A buffered String suitable for sending across as a message
pub struct StringBuffer<'a> {
    /// The backing store for this string, as a mutable pointer to
    /// page-aligned memory.
    bytes: Option<&'a mut [u8]>,

    // Current length of the string, in bytes
    length: u32,

    /// `true` if the backing should be freed when the struct is dropped,
    /// `false` when it shouldn't. It should not be freed when it is `Sent`,
    /// because the kernel will simply move the backing to the target process.
    should_free: bool,

    /// When this object is reconstituted from a MemoryMessage, that object
    /// is placed here. That way, when the object is returned, we can update
    /// the MemoryMessage `valid` field with the new length.
    memory_message: Option<&'a mut MemoryMessage>,
}

impl<'a> StringBuffer<'a> {
    /// Create a new StringBuffer with no backing. This will get
    /// resized as soon as data is written to it.
    pub fn new() -> Self { StringBuffer { bytes: None, length: 0, should_free: true, memory_message: None } }

    /// Create a new StringBuffer with enough space to hold
    /// `usize` characters
    pub fn with_capacity(capacity: usize) -> Self {
        let remainder =
            if ((capacity & 0xFFF) == 0) && (capacity > 0) { 0 } else { 0x1000 - (capacity & 0xFFF) };

        let flags = crate::MemoryFlags::R | crate::MemoryFlags::W;

        // Allocate enough memory to hold the requested data
        let new_mem = map_memory(
            None,
            None,
            // Ensure our byte size is a multiple of 4096
            capacity + remainder,
            flags,
        )
        .expect("Buffer: error in new()/map_memory");

        StringBuffer {
            bytes: Some(unsafe {
                core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), capacity + remainder)
            }),
            length: 0,
            should_free: true,
            memory_message: None,
        }
    }

    fn resize(&mut self, new_capacity: usize) {
        // It is an error to resize a loaned StringBuffer
        if self.memory_message.is_some() {
            return;
        }

        let remainder = if ((new_capacity & 0xFFF) == 0) && (new_capacity > 0) {
            0
        } else {
            0x1000 - (new_capacity & 0xFFF)
        };

        // If the new size is the same as the current size, don't do anything.
        let rounded_new_capacity = new_capacity + remainder;
        if rounded_new_capacity == self.bytes.as_ref().map(|b| b.len()).unwrap_or(0) {
            return;
        }

        let flags = crate::MemoryFlags::R | crate::MemoryFlags::W;

        // Allocate enough memory to hold the new requested data
        let new_slice = if rounded_new_capacity > 0 {
            let new_mem = map_memory(None, None, rounded_new_capacity, flags)
                .expect("Buffer: error in new()/map_memory");
            let new_slice =
                unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), rounded_new_capacity) };
            // Copy the existing string to the new slice
            for (dest_byte, src_byte) in new_slice.iter_mut().zip(self.as_bytes()[0..self.len()].iter()) {
                *dest_byte = *src_byte;
            }
            Some(new_slice)
        } else {
            None
        };

        if let Some(old_slice) = self.bytes.take() {
            let old_addr = old_slice.as_ptr();
            let old_length = old_slice.len();
            unmap_memory(unsafe { MemoryRange::new(old_addr as usize, old_length).unwrap() }).unwrap();
        }
        self.bytes = new_slice;

        // If the string has shrunk, truncate the string.
        if new_capacity < self.len() {
            self.length = new_capacity as u32;
        }
    }

    pub fn as_bytes(&self) -> &[u8] { if let Some(bytes) = &self.bytes { bytes } else { &[] } }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        if let Some(bytes) = self.bytes.as_mut() { bytes } else { &mut [] }
    }

    pub fn as_str(&self) -> core::result::Result<&str, core::str::Utf8Error> {
        if let Some(bytes) = &self.bytes { core::str::from_utf8(&bytes[0..self.len()]) } else { Ok("") }
    }

    pub fn len(&self) -> usize { self.length as usize }

    pub fn is_empty(&self) -> bool { self.length == 0 }

    /// Clear the contents of this String and set the length to 0
    pub fn clear(&mut self) {
        self.length = 0;
        if let Some(mm) = self.memory_message.as_mut() {
            mm.valid = None;
        }
    }

    pub fn to_str(&self) -> &str {
        if let Some(bytes) = &self.bytes {
            unsafe { core::str::from_utf8_unchecked(&bytes[0..self.len()]) }
        } else {
            ""
        }
    }

    fn create_memory_message(&self, id: u32) -> MemoryMessage {
        if let Some(bytes) = &self.bytes {
            let backing_store = unsafe { MemoryRange::new(bytes.as_ptr() as _, bytes.len()).unwrap() };
            MemoryMessage {
                id: id as usize,
                buf: backing_store,
                offset: None,
                valid: MemorySize::new(self.len()),
            }
        } else {
            panic!("Tried to create a memory message with no string!");
        }
    }

    /// # Safety
    ///
    /// This can turn any `MemoryMessage` into a `StringBuffer`, so ensure
    /// that this function is only called when the contents are a `StringBuffer`
    pub unsafe fn from_memory_message(mem: &'a MemoryMessage) -> Self {
        StringBuffer {
            // range: mem.buf,
            bytes: Some(core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len())),
            length: mem.valid.map(|v| v.get()).unwrap_or(0) as u32,
            // offset: mem.offset,
            should_free: false,
            memory_message: None,
        }
    }

    /// # Safety
    ///
    /// This can turn any `MemoryMessage` into a `StringBuffer`, so ensure
    /// that this function is only called when the contents are a `StringBuffer`
    pub unsafe fn from_memory_message_mut(mem: &'a mut MemoryMessage) -> Self {
        StringBuffer {
            // range: mem.buf,
            bytes: Some(core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len())),
            length: mem.valid.map(|v| v.get()).unwrap_or(0) as u32,
            // valid: mem.buf,
            // offset: mem.offset,
            should_free: false,
            memory_message: Some(mem),
        }
    }

    /// Perform a mutable lend of this String to the server.
    pub fn lend_mut(&mut self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = self.create_memory_message(id);

        // Update the offset pointer if the server modified it.
        let result = send_message(connection, Message::MutableBorrow(msg));
        if let Ok(Result::MemoryReturned(_offset, valid)) = result {
            self.length = valid.map(|v| v.get()).unwrap_or(0) as u32;
        }

        result
    }

    pub fn lend(&self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = self.create_memory_message(id);
        send_message(connection, Message::Borrow(msg))
    }

    pub fn send(mut self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = self.create_memory_message(id);
        let result = send_message(connection, Message::Move(msg))?;

        // prevents it from being Dropped.
        self.should_free = false;
        Ok(result)
    }
}

impl<'a> core::str::FromStr for StringBuffer<'a> {
    type Err = &'static str;

    fn from_str(src: &str) -> core::result::Result<StringBuffer<'a>, &'static str> {
        let mut s = Self::with_capacity(src.len());
        // Copy the string into our backing store.
        for (dest_byte, src_byte) in s.bytes.as_mut().unwrap().iter_mut().zip(src.as_bytes()) {
            *dest_byte = *src_byte;
        }
        // Set the string length to the length of the passed-in String,
        // or the maximum possible length. Which ever is smaller.
        s.length = s.as_bytes().len().min(src.as_bytes().len()) as u32;

        // If the string is not valid, set its length to 0.
        if s.as_str().is_err() {
            s.length = 0;
        }

        Ok(s)
    }
}

impl<'a> Default for StringBuffer<'a> {
    fn default() -> Self { Self::new() }
}

impl<'a> core::fmt::Display for StringBuffer<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
}

impl<'a> core::fmt::Write for StringBuffer<'a> {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        // Ensure the string can hold the new data
        self.resize(self.len() + s.len());

        // Copy the data over
        let length = self.len();
        for (dest, src) in self.as_bytes_mut()[length..].iter_mut().zip(s.as_bytes()) {
            *dest = *src;
        }
        self.length += s.len() as u32;
        if let Some(mm) = self.memory_message.as_mut() {
            mm.valid = MemorySize::new(self.length as _).or(None);
        }
        Ok(())
    }
}

impl<'a> core::fmt::Debug for StringBuffer<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
}

impl<'a> core::convert::AsRef<str> for StringBuffer<'a> {
    fn as_ref(&self) -> &str { self.to_str() }
}

impl<'a> PartialEq for StringBuffer<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.length == other.length && self.as_bytes()[..self.len()] == other.as_bytes()[..other.len()]
    }
}

impl<'a> Eq for StringBuffer<'a> {}

impl<'a> Drop for StringBuffer<'a> {
    fn drop(&mut self) {
        if self.should_free && !self.as_bytes().is_empty() {
            let range =
                unsafe { MemoryRange::new(self.as_bytes().as_ptr() as _, self.as_bytes().len()).unwrap() };
            unmap_memory(range).expect("Buffer: failed to drop memory");
        }
    }
}
