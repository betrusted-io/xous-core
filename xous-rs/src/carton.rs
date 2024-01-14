//! A Carton is an object that wraps another object for shipping across the kernel
//! boundary. Structs that are stored in Cartons can be sent as messages.

use crate::{Error, MemoryMessage, MemoryRange, Message, CID};

#[derive(Debug)]
pub struct Carton<'a> {
    range: MemoryRange,
    valid: MemoryRange,
    slice: &'a [u8],
    should_drop: bool,
}

impl<'a> Carton<'a> {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let src_mem = bytes.as_ptr();

        // Ensure our byte size is a multiple of 4096
        let remainder = bytes.len() & 4095;
        let size = bytes.len() + (4096 - remainder);

        let flags = crate::MemoryFlags::R | crate::MemoryFlags::W;

        let new_mem = crate::map_memory(None, None, size, flags).unwrap();

        // NOTE: Remaining bytes are not zeroed. We assume the kernel has done this for us.
        unsafe {
            core::ptr::copy(src_mem, new_mem.as_mut_ptr(), bytes.len());
        };
        let valid = unsafe { MemoryRange::new(new_mem.as_mut_ptr() as usize, bytes.len()).unwrap() };
        Carton {
            range: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), bytes.len()) },
            valid,
            should_drop: true,
        }
    }

    pub fn into_message(mut self, id: usize) -> MemoryMessage {
        // Leak the memory buffer, since it will be taken care of
        // when the MemoryMessage is dropped.
        self.should_drop = false;
        MemoryMessage { id, buf: self.valid, offset: None, valid: None }
    }

    /// Perform an immutable lend of this Carton to the specified server.
    /// This function will block until the server returns.
    pub fn lend(&self, connection: CID, id: usize) -> Result<crate::Result, Error> {
        let msg = MemoryMessage { id, buf: self.valid, offset: None, valid: None };
        crate::send_message(connection, Message::Borrow(msg))
    }

    /// Perform a mutable lend of this Carton to the server.
    pub fn lend_mut(&mut self, connection: CID, id: usize) -> Result<crate::Result, Error> {
        let msg = MemoryMessage { id, buf: self.valid, offset: None, valid: None };
        crate::send_message(connection, Message::MutableBorrow(msg))
    }
}

impl<'a> AsRef<MemoryRange> for Carton<'a> {
    fn as_ref(&self) -> &MemoryRange { &self.valid }
}

impl<'a> AsRef<[u8]> for Carton<'a> {
    fn as_ref(&self) -> &[u8] { self.slice }
}

impl<'a> Drop for Carton<'a> {
    fn drop(&mut self) {
        if self.should_drop {
            crate::unmap_memory(self.range).unwrap();
        }
    }
}
