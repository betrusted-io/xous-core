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

// Alignment target of the resulting KernelArguments structure. I think we can trim it more
// aggressively but I have had troubles in the past with unsafe code and the optimizer fighting
// each other when alignments are too ragged. 16 bytes is, iirc, what the rv32-llvm expects
// things to be aligned to for function prologues so it's a nice safe number to shoot for
// given the amount of unsafes in the block below.
const TARGET_ALIGNMENT: usize = 16;
impl KernelArguments {
    pub fn new(base: *const usize) -> KernelArguments { KernelArguments { base: base as *const u32 } }

    pub fn iter(self) -> KernelArgumentsIterator {
        KernelArgumentsIterator { base: self.base, size: self.size(), offset: 0 }
    }

    /// Returns a size in bytes
    pub fn size(&self) -> usize {
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

    /// Takes the `self` argument list as the `base`, and creates a copy of the argument list into
    /// `dest_region`. At the same time, it will iterate through any other argument lists presented to
    /// it from `args`, and filter them for the tags listed in `tag_filter`, and append them to the list.
    ///
    /// `dest_region` needs to be of a size large enough to hold the new merge arguments. `dest_region`
    /// will have the new argument list jammed up near the top of its allocation area and a new
    /// KernelArguments structure returned as a pointer to that region. The remainder of `dest_region` is
    /// free to use after this operation is done.
    ///
    /// Returns `None` if the merged arguments will not fit within the proposed destination region. The final
    /// size is often much smaller than `dest_region`; it should be read out of the KernelArguments to
    /// determine how far the free memory tracker should be advanced for optimal space packing in the
    /// loader.
    ///
    /// Returns `Some<(KernelArguments, allocation size>)` on success. The `cfg.init_size` pointer needs
    /// to be advanced by the allocation size.
    pub unsafe fn merge_args(
        self,
        incoming_args: &[KernelArguments],
        tag_filter: &[u32],
        dest_region: &mut [u32],
    ) -> Option<(KernelArguments, usize)> {
        // computes an upper bound on size. It's an upper bound because we will filter the incoming
        // merged arguments according to `tag_filter`, but we want to make sure that we have space
        // potentially for all elements before starting the operation.
        //
        // The final argument list will "waste" the space of the filtered out arguments, but this is
        // anticipated to be just a few dozen bytes per argument list, and I'm willing to pay that
        // inefficiency for simplicity and safety.
        //
        // Note that the reason we have to "waste" some space is that free space allocated *down*
        // in addresses in the loader, but the argument list has to be copied from low-to-high addresses.
        // `dest_region` is a huge area of free space handed to us, and simply placing the argument
        // list at the bottom of `dest_region` can't work. So we have to pick & align a spot at the top of
        // memory that is large enough to hold the final list before we begin populating it.
        //
        // Later on we could make this fancier and traverse the incoming argument lists and compute
        // an exact size if we wanted to be super fancy. At the moment, we're leaving about 160 bytes
        // per merged region on the table by using the simple method of summing together the argument regions.
        // I'm not going to lose sleep over this - it's worth revisiting maybe if we get to about ten merged
        // regions, but in practice I'm imagining we'll merge 2-3 regions at most.
        let size_bound =
            self.size() + incoming_args.iter().map(|x| x.size() * size_of::<u32>()).sum::<usize>();
        if size_bound > dest_region.len() * size_of::<u32>() {
            return None;
        }
        // check that size_bound meets the alignment requirements of the `KernelArguments` structure.
        // This is necessary to enforce guarantees on the `unsafe` operations later on.
        assert!(size_bound % size_of::<u32>() == 0);

        // we can fit - so now, we're going to do some unsafe casts to manipulate the region of memory
        // represented by `dest_region`. These unsafe casts are only going to be safe because every
        // value can be represented, and also the alignment guarantee of `dest_region` is appropriate
        // per the caller spec in the method invocation comments.
        //
        // NOTE: `dest_region` is assumed to be *much* larger than the region we need to go into,
        // and we want the argument buffer to slot into the top of `dest_region`.
        let mut arg_buffer =
            (dest_region.as_mut_ptr() as *mut u32).add(dest_region.len()).sub(size_bound / size_of::<u32>());
        let alignment_overhead = (arg_buffer as usize) % TARGET_ALIGNMENT;
        arg_buffer = (arg_buffer as usize & !(TARGET_ALIGNMENT - 1)) as *mut u32;
        let aligned_size = size_bound + alignment_overhead;

        // this copy whacks the base arguments into place within the `dest_region`
        // It's safe because `dest_region` has an alignment appropriate for `KernelArguments`; it's
        // not overlapping with the source buffer (the source buffer should be coming from flash,
        // this is going to RAM), and all elements have valid values.
        unsafe {
            crate::phase1::memcpy(arg_buffer, self.base as *const u32, self.size() as usize);
        }

        // now iterate through the `incoming_args` and append the records that match the `tag_filter`
        // the pointer below tracks where the incoming argument should be appended.
        let mut incoming_arg_ptr: usize = arg_buffer.add(self.size() / size_of::<u32>()) as usize;
        for arg_list in incoming_args {
            for arg in arg_list.iter() {
                // filter the argument based on the list of tags to include in the copy
                if tag_filter.iter().any(|&t| t == arg.name) {
                    let extra_size = arg.size as usize * size_of::<u32>() + size_of::<Tag>();
                    assert!(extra_size % size_of::<usize>() == 0, "Argument did not conform to size spec!");
                    // safety:
                    //  - all new arguments meet the alignment criteria (as checked by the assert above)
                    //  - all values are representable
                    //  - space at incoming_arg_ptr is guaranteed to be available
                    unsafe {
                        crate::phase1::memcpy(incoming_arg_ptr as *mut u32, arg.base as *mut u32, extra_size);
                    }
                    // increment the arg pointer to the next argument slot
                    incoming_arg_ptr += extra_size;
                }
            }
        }

        // check that we didn't blow out the size bound after doing the copies
        let final_size_bytes = incoming_arg_ptr - arg_buffer as usize;
        assert!(final_size_bytes <= size_bound);
        // note how much space we ended up wasting. We can reduce this by improving the computation of
        // `size_bound` later on if we need to wring out a few more bytes from the boot process. Part
        // of the reason the few bytes matters is that these are "unrecoverable", i.e. they are mapped
        // into the kernel, used once, and never de-allocated, so we want the convenience overhead to be
        // reasonable.
        crate::println!("Argument merge complete. {} bytes over-allocated", size_bound - final_size_bytes);

        // now fixup the CRC & size of the base XArg argument.
        // extract the first argument as an XargArg by just overlaying the structure template on the pointer.
        // only safe because the arg_buffer pointer is aligned and is actually an XArg tag.
        let xarg = unsafe { (arg_buffer as *mut XargArg).as_mut().unwrap() };
        xarg.arg_size_u32 = (final_size_bytes / size_of::<u32>()) as u32;

        // compute the CRC of the XArg argument's data section
        use crc::{Hasher16, crc16};
        // create a u8-region out of the XArg's data section. The data section is known to follow the Tag
        // header and be exactly the length of the size in the Tag section.
        let xarg_data = unsafe {
            core::slice::from_raw_parts(
                (self.base as *const u8).add(size_of::<Tag>()),
                xarg.tag.size as usize,
            )
        };
        let mut digest = crc16::Digest::new(crc16::X25);
        digest.write(&xarg_data);
        xarg.tag.crc16 = digest.sum16();

        // return the new argument buffer
        Some((KernelArguments::new(arg_buffer as *const usize), aligned_size))
    }
}

/// KernelArgument is a stack-allocated structure that describes a pointer to tagged data.
/// It effectively decompressed the tag name + size field and converts the embedded variable-length
/// data into a slice.
///
/// It is constructed by referring to the base of the tag structure, and then providing an
/// offset. The base + offset should point to the top of a Tag structure.
pub struct KernelArgument {
    pub base: *const u32,

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
        KernelArgument { base, name, size: size * size_of::<u32>() as u32, data }
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
