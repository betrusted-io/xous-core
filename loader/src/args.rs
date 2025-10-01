/// Convert a four-letter string into a 32-bit int.
#[macro_export]
macro_rules! make_type {
    ($fcc:expr) => {{
        let mut c: [u8; 4] = Default::default();
        c.copy_from_slice($fcc.as_bytes());
        u32::from_le_bytes(c)
    }};
}

#[repr(C)]
/// This is a representation of tag data *on disk*. It can be located at any 4-byte
/// alignment within the argument list.
pub struct Tag {
    /// Ascii-printable name, not null-terminated, in little endian format.
    pub tag: u32,

    /// CRC16 of the data section, using CCITT polynomial.
    pub crc16: u16,

    /// Size of the data section, in 4-byte words.
    pub size: u16,
}

#[repr(C)]
pub struct XargArg {
    tag: Tag,
    /// Full length of the entire argument structure (not just this argument)
    /// reported in units of u32.
    arg_size_u32: u32,
    version: u32,
    ram_start: u32,
    ram_size: u32,
    ram_name: u32,
}

/// KernelArguments is a self-extracting structure which handles variable-length fields
/// that only needs a pointer to its start to derive the rest of its structure.
#[derive(Clone, Copy)]
pub struct KernelArguments {
    pub base: *const u32,
}

/// The arguments iterator is constructed by reading into kernel arguments structure, assuming
/// it is a well-formed structure where the first meta-tag is placed correctly, allowing the
/// extraction of a `size` field.
pub struct KernelArgumentsIterator {
    base: *const u32,
    size: usize,
    offset: u32,
}

impl KernelArguments {
    pub fn new(base: *const usize) -> KernelArguments { KernelArguments { base: base as *const u32 } }

    pub fn iter(self) -> KernelArgumentsIterator {
        KernelArgumentsIterator { base: self.base, size: self.size(), offset: 0 }
    }

    pub fn size(self) -> usize {
        // Add a rudimentary check to make sure that KernelArguments are pointed to the correct
        // structure, by checking for the right magic number in the first field.
        let tag = unsafe { (self.base as *const Tag).as_ref().unwrap() };
        if tag.tag != u32::from_le_bytes(*b"XArg") {
            crate::println!("FATAL: Couldn't find XArg magic number on kernel tags.");
            panic!(); // nothing we can do to repair this defect.
        }
        let xarg_tag = unsafe { (self.base as *const XargArg).as_ref().unwrap() };
        xarg_tag.arg_size_u32 as usize * size_of::<u32>()
    }
}

/// KernelArgument is a stack-allocated structure that describes a pointer to tagged data.
/// It effectively decompressed the tag name + size field and converts the embedded variable-length
/// data into a slice.
///
/// It is constructed by referring to the base of the tag structure, and then providing an
/// offset. The base + offset should point to the top of a Tag structure.
pub struct KernelArgument {
    pub name: u32,

    // Total number of bytes in the data section
    pub size: u32,

    // Data section, as a slice of 32-bit ints
    pub data: &'static [u32],
}

impl KernelArgument {
    pub fn new(base: *const u32, offset: u32) -> Self {
        let tag = unsafe { (base.add(offset as usize / size_of::<u32>()) as *const Tag).as_ref().unwrap() };
        let name = tag.tag;
        let size = tag.size as u32;
        // adding 1 to the tag yields the address immediately after the tag data.
        let data =
            unsafe { core::slice::from_raw_parts((tag as *const Tag).add(1) as *const u32, size as usize) };
        // Return the stack-allocated KernelArgument, which embeds a reference to the now base/bounds
        // protected data slice.
        KernelArgument { name, size: size * size_of::<u32>() as u32, data }
    }
}

impl Iterator for KernelArgumentsIterator {
    type Item = KernelArgument;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset as usize >= self.size {
            None
        } else {
            let new_arg = KernelArgument::new(self.base, self.offset);
            self.offset += new_arg.size + 8;
            Some(new_arg)
        }
    }
}
