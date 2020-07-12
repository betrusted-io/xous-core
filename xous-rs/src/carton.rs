//! A Carton is an object that wraps another object for shipping across the kernel
//! boundary. Structs that are stored in Cartons can be sent as messages.
extern crate alloc;
extern crate core;
use alloc::alloc::{alloc, dealloc, Layout};

use crate::{MemoryMessage, MemoryRange};

pub struct Carton {
    contents: *mut u8,
    size: usize,
}

impl Carton {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let src_mem = bytes.as_ptr();
        let len = bytes.len();
        let layout = Layout::from_size_align(len, 4096).unwrap();
        let new_mem = unsafe {
            let new_mem = alloc(layout);
            core::ptr::copy(src_mem, new_mem, len);
            new_mem
        };
        Carton {
            contents: new_mem,
            size: len,
        }
    }

    pub fn into_message(self, id: usize) -> MemoryMessage {
        MemoryMessage {
            id,
            buf: MemoryRange::new(self.contents as usize, self.size),
            offset: None,
            valid: None,
        }
    }
}

impl Drop for Carton {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.size, 4096).unwrap();
        let ptr = self.contents;
        unsafe { dealloc(ptr, layout) };
    }
}
