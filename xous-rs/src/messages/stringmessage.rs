use crate::{MemoryMessage, MemoryRange};

pub struct StringMessage {
    backing: *mut u8,
    length: usize,
}

impl StringMessage {
    pub fn new(src: &str) -> Self {
        StringMessage {
            backing: 0 as _,
            length: 0
        }
    }

    pub fn into_message(self, id: usize) -> MemoryMessage {
        MemoryMessage {
            id,
            buf: MemoryRange::new(self.backing as _, self.length),
            offset: None,
            valid: None,
        }
    }
}
