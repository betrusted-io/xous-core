use crate::api::*;
use super::*;

use core::num::NonZeroU64;
use core::ops::{Deref, DerefMut};
use core::{mem, slice};
use core::mem::size_of;
use aes_gcm_siv::Nonce;

pub(crate) const FREE_CACHE_SIZE: usize = 16;
pub type BasisRootName = [u8; PDDB_MAX_BASIS_NAME_LEN];
/// In basis space, the BasisRoot is located at 0
/// The first 4GiB is reserved for the Basis Root.
/// Keys begin at the next 4GiB.
/// AAD associated with the BasisRoot consist of a bytewise concatenation of:
///   - Basis name
///   - version number (should match version inside; complicates downgrade attacks)
///   - FPGA's silicon DNA number (makes a naive raw-copy of the data to another device unusable;
///     but of course, the DNA ID can be forged minor efforts)
///
/// As a directory structure, the basis root is designed to be read into RAM in a contiguous block.
/// it'll typically be less than a page in length, but a pathological number of dictionaries can make it
/// much longer.
#[repr(C)]
pub(crate) struct BasisRoot {
    /// this is stored as plaintext and generated fresh every time the block is re-encrypted
    pub(crate) p_nonce: [u8; size_of::<Nonce>()],
    // everything below here is encrypted using AES-GCM-SIV
    pub(crate) magic: [u8; 4],
    pub(crate) version: u16,
    pub(crate) journal_rev: u32,
    pub(crate) name: BasisRootName,
    /// increments every time the BasisRoot is modified. This field must saturate, not roll over.
    pub(crate) age: u32,
    /*
    /// a cache of up FREE_CACHE_SIZE indicating the location of free space for use by basis
    /// functions, such as adding growing the size of this structure, adding more dictionaries,
    /// adding keys to dictionaries, or extending existing keys. This saves from having to do
    /// frequent free space search/compaction operations on memory during writes and updates.
    pub(crate) free_cache: [Option<FreeSpace>; FREE_CACHE_SIZE],
    */
    /// "open end" of the pre-allocated space for the Basis. All Basis data must exist in an extent that is
    /// less than this value. This can be grown and shrunk with allocation and compaction processes.
    pub(crate) prealloc_open_end: PageAlignedVa,
    pub(crate) num_dictionaries: u32,
    // dict_slice: [DictPointer; num_dictionaries],  // DictPointers + num_dictionaries above can be turned into a dict_slice
    ////// the following records are appended by the Serialization routine
    // pad: [u8],    // padding out to the next 4096-byte block less 16 bytes
    // p_tag: [u8; 16], // auth tag output of the AES-GCM-SIV
}
impl BasisRoot {
    /// Compute the number of memory pages consumed by the BasisRoot structure itself.
    /// This is the size of BasisRoot, plus the dictionaries allocated within the Basis.
    /// It does mean that your memory usage scales directly with the number of dictionaries
    /// you put in the Basis, because there is no way to chain or defer the Basis structure
    /// if you get thousands of Dictionaries. Note that the intent is to have typcially no
    /// more than a couple dozen dictionaries; if you want to store a lot of different records,
    /// you can create thousands of Keys more efficiently, than you can dictionaries.
    pub(crate) fn len_pages(&self) -> usize {
        let min_len = core::mem::size_of::<BasisRoot>()
            + ((self.num_dictionaries as usize)
            * core::mem::size_of::<DictPointer>())
            + core::mem::size_of::<aes_gcm_siv::Tag>();
        if (min_len & (1 - PAGE_SIZE)) == 0 {
            min_len / PAGE_SIZE
        } else {
            min_len / PAGE_SIZE + 1
        }
    }
    /// Number of bytes needed to pad between the length of the BasisRoot structure and the plaintext
    /// tag that will get appended to the end
    pub(crate) fn padding_count(&self) -> usize {
        self.len_pages() * PAGE_SIZE -
        (core::mem::size_of::<BasisRoot>()
         + ((self.num_dictionaries as usize) * core::mem::size_of::<DictPointer>())
        )
    }
}
impl Deref for BasisRoot {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const BasisRoot as *const u8, core::mem::size_of::<BasisRoot>())
                as &[u8]
        }
    }
}

impl DerefMut for BasisRoot {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut BasisRoot as *mut u8, core::mem::size_of::<BasisRoot>())
                as &mut [u8]
        }
    }
}



pub(crate) struct DictPointer {
    name: [u8; PDDB_MAX_DICT_NAME_LEN],
    age: u32,  // increment every time the dictionary pointer is modified.
    addr: u64, // the virtual address of the dictionary
}

/// FreeSpace address space is in the virtual memory space of the containing Basis
#[derive(Copy, Clone)]
pub(crate) struct FreeSpace {
    start: u64,
    len: NonZeroU64,
}

/// Typically individual dictionaries start out life having their own 4k-page, but they
/// can be compacted together if they seem to be static/non-changing and we need more space.
pub(crate) struct Dictionary {
    p_nonce: [u8; size_of::<Nonce>()],
    journal_rev: u32,
    num_keys: u32,
    age: u32, // increment every time the dictionary definition itself is modified
    // key_slice: [HashKey],  // a synthetic record that is a slice of HashKeys
    // pad: [u8],     // padding out to the next 4096-byte block less 16 bytes
    // p_tag: [u8; 16]   // auth tag output of AES-GCM-SIV
}

/// This defines a key's name, along with a pointer to its location in memory.
/// HashKeys are packed at the end of a Dictionary.
pub(crate) struct HashKey {
    name: [u8; PDDB_MAX_KEY_NAME_LEN],
    journal_rev: u32,
    /// incremented every time the key is re-written to flash. saturating add.
    age: u32,
    /// length of the data stored in the HashKey
    length: u64,
    /// location of the data of the HashKey. This is always in absolute virtual memory coordinates.
    /// Note that offsets relative to the `base_addr` need to account for the `nonce` and `tag` that
    /// are necessitated by the page-by-page encryption of the raw data itself.
    base_addr: u64,
}

/// this is the structure of the Basis Key in RAM. The "key" and "iv" are actually never committed to
/// flash; only the "salt" is written to disk. The final "salt" is computed as the XOR of the salt on disk
/// and the user-provided "basis name". We never record the "basis name" on disk, so that the existence of
/// any Basis can be denied.
pub(crate) struct BasisKey {
    salt: [u8; 16],
    key: [u8; 32], // derived from lower 256 bits of sha512(bcrypt(salt, pw))
    iv: [u8; 16], // an IV derived from the upper 128 bits of the sha512 hash from above, XOR with the salt
}

