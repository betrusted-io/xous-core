use core::convert::TryInto;

use rkyv::{Fallible, ser::Serializer};
use xous::{
    CID, Error, MemoryAddress, MemoryFlags, MemoryMessage, MemoryRange, MemorySize, Message, Result,
    map_memory, send_message, unmap_memory,
};

#[derive(Debug)]
pub struct Buffer<'a> {
    range: MemoryRange,
    valid: MemoryRange,
    offset: Option<MemoryAddress>,
    slice: &'a mut [u8],
    should_drop: bool,
    memory_message: Option<&'a mut MemoryMessage>,
}

pub struct XousDeserializer;

// Unreachable enum pattern, swap out for the never type (!) whenever that gets stabilized
#[derive(Debug)]
pub enum XousUnreachable {}

impl rkyv::Fallible for XousDeserializer {
    type Error = XousUnreachable;
}

impl<'a> Buffer<'a> {
    #[allow(dead_code)]
    pub fn new(len: usize) -> Self {
        let remainder = if ((len & 0xFFF) == 0) && (len > 0) { 0 } else { 0x1000 - (len & 0xFFF) };

        let flags = MemoryFlags::R | MemoryFlags::W;

        // Allocate enough memory to hold the requested data
        let new_mem = map_memory(
            None,
            None,
            // Ensure our byte size is a multiple of 4096
            len + remainder,
            flags,
        )
        .expect("Buffer: error in new()/map_memory");

        let valid = unsafe { MemoryRange::new(new_mem.as_mut_ptr() as usize, len + remainder).unwrap() };
        Buffer {
            range: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), len + remainder) },
            valid,
            offset: None,
            should_drop: true,
            memory_message: None,
        }
    }

    // use a volatile write to ensure a clear operation is not optimized out
    // for ensuring that a buffer is cleared, e.g. at the exit of a function
    pub fn volatile_clear(&mut self) {
        let b = self.slice.as_mut_ptr();
        for i in 0..self.slice.len() {
            unsafe {
                b.add(i).write_volatile(core::mem::zeroed());
            }
        }
        // Ensure the compiler doesn't re-order the clear.
        // We use `SeqCst`, because `Acquire` only prevents later accesses from being reordered before
        // *reads*, but this method only *writes* to the locations.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    // use to serialize a buffer between process-local threads. mainly for spawning new threads with more
    // complex argument structures.
    #[allow(dead_code)]
    pub unsafe fn to_raw_parts(&self) -> (usize, usize, usize) {
        if let Some(offset) = self.offset {
            (self.valid.as_ptr() as usize, self.valid.len(), usize::from(offset))
        } else {
            (self.valid.as_ptr() as usize, self.valid.len(), 0)
        }
    }

    // use to serialize a buffer between process-local threads. mainly for spawning new threads with more
    // complex argument structures.
    #[allow(dead_code)]
    pub unsafe fn from_raw_parts(address: usize, len: usize, offset: usize) -> Self {
        let mem = MemoryRange::new(address, len).expect("invalid memory range args");
        let off = if offset != 0 { Some(offset.try_into().unwrap()) } else { None };
        Buffer {
            range: mem,
            slice: core::slice::from_raw_parts_mut(mem.as_mut_ptr(), mem.len()),
            valid: mem,
            offset: off,
            should_drop: false,
            memory_message: None,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message(mem: &'a MemoryMessage) -> Self {
        Buffer {
            range: mem.buf,
            slice: core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len()),
            valid: mem.buf,
            offset: mem.offset,
            should_drop: false,
            memory_message: None,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message_mut(mem: &'a mut MemoryMessage) -> Self {
        Buffer {
            range: mem.buf,
            slice: core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len()),
            valid: mem.buf,
            offset: mem.offset,
            should_drop: false,
            memory_message: Some(mem),
        }
    }

    /// Perform a mutable lend of this Buffer to the server.
    #[allow(dead_code)]
    pub fn lend_mut(&mut self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: self.offset,
            valid: MemorySize::new(self.slice.len()),
        };

        // Update the offset pointer if the server modified it.
        let result = send_message(connection, Message::MutableBorrow(msg));
        if let Ok(Result::MemoryReturned(offset, _valid)) = result {
            self.offset = offset;
        }

        result
    }

    #[allow(dead_code)]
    pub fn lend(&self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: self.offset,
            valid: MemorySize::new(self.slice.len()),
        };
        send_message(connection, Message::Borrow(msg))
    }

    #[allow(dead_code)]
    pub fn send(mut self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: self.offset,
            valid: MemorySize::new(self.slice.len()),
        };
        let result = send_message(connection, Message::Move(msg))?;

        // prevents it from being Dropped.
        self.should_drop = false;
        Ok(result)
    }

    #[allow(dead_code)]
    pub fn into_buf<S>(src: S) -> core::result::Result<Self, ()>
    where
        S: rkyv::Serialize<rkyv::ser::serializers::BufferSerializer<Buffer<'a>>>,
    {
        let buf = Self::new(core::mem::size_of::<S>());
        let mut ser = rkyv::ser::serializers::BufferSerializer::new(buf);
        let pos = ser.serialize_value(&src).or(Err(()))?;
        let mut buf = ser.into_inner();
        buf.offset = MemoryAddress::new(pos);
        Ok(buf)
    }

    // erase ourself and re-use our allocated storage
    #[allow(dead_code)]
    pub fn rewrite<S>(&mut self, src: S) -> core::result::Result<(), xous::Error>
    where
        S: rkyv::Serialize<rkyv::ser::serializers::BufferSerializer<&'a mut [u8]>>,
    {
        let copied_slice =
            unsafe { core::slice::from_raw_parts_mut(self.slice.as_mut_ptr(), self.slice.len()) };
        // zeroize the slice before using it
        /*for &mut s in copied_slice {
            s = 0;
        }*/
        let mut ser = rkyv::ser::serializers::BufferSerializer::new(copied_slice);
        let pos = ser.serialize_value(&src).or(Err(())).unwrap();
        self.slice = ser.into_inner();
        self.offset = MemoryAddress::new(pos);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn replace<S>(&mut self, src: S) -> core::result::Result<(), &'static str>
    where
        S: rkyv::Serialize<rkyv::ser::serializers::BufferSerializer<&'a mut [u8]>>,
    {
        // We must have a `memory_message` to update in order for this to work.
        // Otherwise, we risk having the pointer go to somewhere invalid.
        if self.memory_message.is_none() {
            // Create this message using `from_memory_message_mut()` instead of
            // `from_memory_message()`.
            Err("couldn't serialize because buffer wasn't mutable")?;
        }
        // Unsafe Warning: Create a copy of the backing slice to hand to the deserializer.
        // This is required because the deserializer consumes the buffer and returns it
        // later as part of `.into_inner()`.
        // The "correct" way to do this would be to implement `rkyv::Serializer` an `rkyv::Fallible`
        // for ourselves.
        let copied_slice =
            unsafe { core::slice::from_raw_parts_mut(self.slice.as_mut_ptr(), self.slice.len()) };
        let mut ser = rkyv::ser::serializers::BufferSerializer::new(copied_slice);
        let pos = ser.serialize_value(&src).map_err(|err| err).unwrap();
        self.offset = MemoryAddress::new(pos);
        if let Some(ref mut msg) = self.memory_message.as_mut() {
            msg.offset = MemoryAddress::new(pos);
        }
        Ok(())
    }

    /// Zero-copy representation of the data on the receiving side, wrapped in an "Archived" trait and left in
    /// the heap. Cheap so uses "as_" prefix.
    #[allow(dead_code)]
    pub fn as_flat<T, U>(&self) -> core::result::Result<&U, ()>
    where
        T: rkyv::Archive<Archived = U>,
    {
        let pos = self.offset.map(|o| o.get()).unwrap_or_default();
        let r = unsafe { rkyv::archived_value::<T>(self.slice, pos) };
        Ok(r)
    }

    /// A representation identical to the original, but reequires copying to the stack. More expensive so uses
    /// "to_" prefix.
    #[allow(dead_code)]
    pub fn to_original<T, U>(&self) -> core::result::Result<T, ()>
    where
        T: rkyv::Archive<Archived = U>,
        U: rkyv::Deserialize<T, dyn Fallible<Error = XousUnreachable>>,
    {
        let pos = self.offset.map(|o| o.get()).unwrap_or_default();
        let r = unsafe { rkyv::archived_value::<T>(self.slice, pos) };
        Ok(r.deserialize(&mut XousDeserializer {}).unwrap())
    }
}

impl<'a> core::convert::AsRef<[u8]> for Buffer<'a> {
    fn as_ref(&self) -> &[u8] { self.slice }
}

impl<'a> core::convert::AsMut<[u8]> for Buffer<'a> {
    fn as_mut(&mut self) -> &mut [u8] { self.slice }
}

impl<'a> core::ops::Deref for Buffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target { &*self.slice }
}

impl<'a> core::ops::DerefMut for Buffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.slice }
}

impl<'a> Drop for Buffer<'a> {
    fn drop(&mut self) {
        if self.should_drop {
            unmap_memory(self.range).expect("Buffer: failed to drop memory");
        }
    }
}
