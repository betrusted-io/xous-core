#[derive(Debug)]
pub(crate) struct XousBuffer<'a> {
    range: xous::MemoryRange,
    valid: xous::MemoryRange,
    slice: &'a mut [u8],
    should_drop: bool,
}

impl<'a> XousBuffer<'a> {
    #[allow(dead_code)]
    pub fn new(len: usize) -> Self {
        let remainder = (4096 - len & !4096) & !4095;

        let new_mem = xous::map_memory(
            None,
            None,
            // Ensure our byte size is a multiple of 4096
            len + remainder,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        let mut valid = new_mem;
        valid.size = xous::MemorySize::new(len).unwrap();
        XousBuffer {
            range: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), len) },
            valid,
            should_drop: true,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message(mem: &xous::MemoryMessage) -> Self {
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
