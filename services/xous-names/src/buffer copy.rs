use core::convert::{AsMut, AsRef};
use core::ops::{Deref, DerefMut};

#[derive(Debug)]
pub(crate) struct XousBuffer<'a, T> {
    range: xous::MemoryRange,
    valid: xous::MemoryRange,
    val: &'a mut T,
    should_drop: bool,
}

impl<'a, T> XousBuffer<'a, T> {
    pub fn new() -> Self {
        let len = core::mem::size_of::<T>();
        let remainder = len & 4095;

        let new_mem = xous::map_memory(
            None,
            None,
            // Ensure our byte size is a multiple of 4096
            len + (4096 - remainder),
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        let mut valid = new_mem;
        valid.size = xous::MemorySize::new(len).unwrap();
        XousBuffer {
            range: new_mem,
            val: unsafe { &mut *(new_mem.as_mut_ptr() as *mut T) },
            valid,
            should_drop: true,
        }
    }

    pub unsafe fn from_memory_message(mem: &xous::MemoryMessage) -> Self {
        XousBuffer {
            range: mem.buf,
            val: &mut *(mem.buf.as_mut_ptr() as *mut T),
            valid: mem.buf,
            should_drop: false,
        }
    }

    /// Perform a mutable lend of this Carton to the server.
    pub fn lend_mut(
        &mut self,
        connection: xous::CID,
        id: usize,
    ) -> Result<xous::Result, xous::Error> {
        let msg = xous::MemoryMessage {
            id,
            buf: self.valid,
            offset: None,
            valid: None,
        };
        xous::send_message(connection, xous::Message::MutableBorrow(msg))
    }
}

impl<'a, T: Deref> Deref for XousBuffer<'a, T> {
    type Target = T::Target;

    fn deref(&self) -> &Self::Target {
        &*self.val
    }
}

impl<'a, T: DerefMut> DerefMut for XousBuffer<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.val
    }
}

impl<'a, T: AsRef<[U]>, U> AsRef<[U]> for XousBuffer<'a, T> {
    fn as_ref(&self) -> &[U] {
        self.val.as_ref()
    }
}

impl<'a, T: AsMut<[U]>, U> AsMut<[U]> for XousBuffer<'a, T> {
    fn as_mut(&mut self) -> &mut [U] {
        self.val.as_mut()
    }
}
