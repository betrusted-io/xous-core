use core::fmt;
use core::slice;
use xous;

pub struct LogString<'a> {
    s: &'a str,
}

impl<'a> LogString<'a> {
    pub fn from_message(message: &xous::MemoryMessage) -> LogString<'a> {
        println!(
            "Message address is at {:08x}",
            message.buf.addr.get()
        );
        LogString {
            s: unsafe {
                core::str::from_utf8(slice::from_raw_parts(
                    message.buf.addr.get() as *mut u8,
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
