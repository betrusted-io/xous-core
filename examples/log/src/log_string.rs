use core::fmt;
use core::slice;
use xous;

pub struct LogString<'a> {
    backing: &'a [u8],
    s: &'a str,
}

impl<'a> LogString<'a> {
    pub fn from_message(message: xous::MemoryMessage) -> LogString<'a> {
        println!(
            "Message address is at {:08x}",
            message.buf.expect("no buffer").get()
        );
        LogString {
            backing: unsafe {
                slice::from_raw_parts(
                    message.buf.expect("no buffer present").get() as *mut u8,
                    message.buf_size.expect("no buffer length present").get(),
                )
            },
            s: unsafe {
                core::str::from_utf8(slice::from_raw_parts(
                    message.buf.expect("no buffer present").get() as *mut u8,
                    message.valid.expect("no buffer length present").get(),
                ))
                .expect("message didn't have valid utf8")
            },
        }
    }
}

impl<'a> fmt::Display for LogString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.s)
    }
}

impl<'a> Drop for LogString<'a> {
    fn drop(&mut self) {
        xous::syscall::unmap_memory(xous::MemoryAddress::new(self.backing.as_ptr() as usize).unwrap(), self.backing.len())
            .expect("couldn't free string");
    }
}
