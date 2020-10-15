use core::fmt::{Error, Write};
use core::slice;

pub struct LogStr<'a> {
    raw_slice: &'a mut [u8],
    len: usize,
    string: &'a str,
}

impl<'a> LogStr<'a> {
    pub fn new() -> LogStr<'a> {
        let mem = xous::syscall::map_memory(
            None,
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't allocate memory");

        // Zero out the buffer. This is required to avoid UB when accessing
        // "uninitialized" memory.
        unsafe {
            let ptr = mem.as_mut_ptr();
            for off in 0..mem.len() {
                ptr.add(off).write_volatile(0);
            }
        }

        let raw_slice = unsafe { slice::from_raw_parts_mut(mem.as_mut_ptr(), mem.len()) };

        LogStr {
            raw_slice,
            len: 0,
            string: unsafe {
                core::str::from_utf8_unchecked(slice::from_raw_parts(mem.as_ptr(), 0))
            },
        }
    }

    #[allow(dead_code)]
    pub fn into_memory_message(self, id: xous::MessageId) -> Result<xous::MemoryMessage, xous::Error> {
        // XXX This should forget the memory allocated, as it will be sent to the other process
        Ok(xous::MemoryMessage {
            id,
            buf: xous::MemoryRange::new(self.raw_slice.as_ptr() as usize, self.raw_slice.len()).unwrap(),
            offset: None,
            valid: xous::MemorySize::new(self.len),
        })
    }

    pub fn as_memory_message(&self, id: xous::MessageId) -> Result<xous::MemoryMessage, xous::Error> {
        Ok(xous::MemoryMessage {
            id,
            buf: xous::MemoryRange::new(self.raw_slice.as_ptr() as usize, self.raw_slice.len()).unwrap(),
            offset: None,
            valid: xous::MemorySize::new(self.len),
        })
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub unsafe fn set_len(&mut self, len: usize) {
        self.len = len;
        self.string = core::str::from_utf8_unchecked(slice::from_raw_parts(self.raw_slice.as_ptr(), self.len));
    }
}

impl<'a> Write for LogStr<'a> {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for (dest, src) in self.raw_slice.iter_mut().skip(self.len).zip(s.bytes()) {
            *dest = src;
            self.len += 1;
        }
        self.string = unsafe { core::str::from_utf8_unchecked(slice::from_raw_parts(self.raw_slice.as_ptr(), self.len)) };
        Ok(())
    }
}

impl<'a> core::fmt::Display for LogStr<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.string)
    }
}
