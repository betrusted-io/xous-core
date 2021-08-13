// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

static mut KERNEL_ARGUMENTS_BASE: *const u32 = 0 as *const u32;

pub struct KernelArguments {
    pub base: *const u32,
}

pub struct KernelArgumentsIterator {
    base: *const u32,
    size: usize,
    offset: usize,
}

#[allow(dead_code)]
impl KernelArguments {
    pub fn get() -> Self {
        KernelArguments {
            base: unsafe { KERNEL_ARGUMENTS_BASE },
        }
    }

    pub unsafe fn init(base: *const u32) {
        KERNEL_ARGUMENTS_BASE = base;
    }

    pub fn iter(&self) -> KernelArgumentsIterator {
        KernelArgumentsIterator {
            base: self.base,
            size: self.size(),
            offset: 0,
        }
    }

    /// Get the size of the entire kernel argument structure
    pub fn size(&self) -> usize {
        unsafe { self.base.add(2).read() as usize * 4 }
    }
}

pub struct KernelArgument {
    pub name: u32,
    pub size: usize,
    pub data: &'static [u32],
}

impl KernelArgument {
    pub fn new(base: *const u32, offset: usize) -> Self {
        let name = unsafe { base.add(offset / 4).read() } as u32;
        let size = unsafe { (base.add(offset / 4 + 1) as *const u16).add(1).read() } as usize;
        let data = unsafe { core::slice::from_raw_parts(base.add(offset / 4 + 2), size) };
        KernelArgument {
            name,
            size: size * 4,
            data,
        }
    }
}

impl Iterator for KernelArgumentsIterator {
    type Item = KernelArgument;
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.size {
            None
        } else {
            let new_arg = KernelArgument::new(self.base, self.offset);
            self.offset += new_arg.size + 8;
            Some(new_arg)
        }
    }
}

impl fmt::Display for KernelArgument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag_name_bytes = self.name.to_le_bytes();
        let s = unsafe {
            use core::slice;
            use core::str;
            // First, we build a &[u8]...
            let slice = slice::from_raw_parts(tag_name_bytes.as_ptr(), 4);
            // ... and then convert that slice into a string slice
            str::from_utf8_unchecked(slice)
        };

        write!(f, "{} ({:08x}, {} bytes):", s, self.name, self.size)?;
        for word in self.data {
            write!(f, " {:08x}", word)?;
        }
        Ok(())
    }
}
