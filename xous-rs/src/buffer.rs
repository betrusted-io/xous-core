use crate::{Result, Error, MemoryMessage, MemoryRange, MemoryFlags, MemorySize, Message, CID, map_memory, send_message};

#[derive(Debug)]
pub struct XousBuffer<'a> {
    range: MemoryRange,
    valid: MemoryRange,
    slice: &'a mut [u8],
    should_drop: bool,
}

impl<'a> XousBuffer<'a> {
    #[allow(dead_code)]
    pub fn new(len: usize) -> Self {
        let remainder = (4096 - len & !4096) & !4095;

        let new_mem = map_memory(
            None,
            None,
            // Ensure our byte size is a multiple of 4096
            len + remainder,
            MemoryFlags::R | MemoryFlags::W,
        )
        .unwrap();

        let mut valid = new_mem;
        valid.size = MemorySize::new(len).unwrap();
        XousBuffer {
            range: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), len) },
            valid,
            should_drop: true,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message(mem: &MemoryMessage) -> Self {
        XousBuffer {
            range: mem.buf,
            slice: core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len()),
            valid: mem.buf,
            should_drop: false,
        }
    }

    /// Perform a mutable lend of this Carton to the server.
    #[allow(dead_code)]
    pub fn lend_mut(
        &mut self,
        connection: CID,
        id: u32,
    ) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: None,
            valid: None,
        };
        send_message(connection, Message::MutableBorrow(msg))
    }

    #[allow(dead_code)]
    pub fn lend(
        &self,
        connection: CID,
        id: u32
    ) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: None,
            valid: None,
        };
        send_message(connection, Message::Borrow(msg))
    }

    #[allow(dead_code)]
    pub fn send(
        mut self,
        connection: CID,
        id: u32,
    ) -> core::result::Result<Result, Error> {
        let msg = crate::MemoryMessage {
            id: id as usize,
            buf: self.valid,
            offset: None,
            valid: crate::MemorySize::new(self.slice.len()),
        };
        let result = send_message(connection, Message::Move(msg))?;

        // prevents it from being Dropped.
        self.should_drop = false;
        Ok(result)
    }
}

impl<'a> core::convert::AsRef<[u8]> for XousBuffer<'a> {
    fn as_ref(&self) -> &[u8] {
        self.slice
    }
}

impl<'a> core::convert::AsMut<[u8]> for XousBuffer<'a> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.slice
    }
}

impl<'a> core::ops::Deref for XousBuffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &*self.slice
    }
}

impl<'a> core::ops::DerefMut for XousBuffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.slice
    }
}
