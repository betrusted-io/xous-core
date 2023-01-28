use crate::api::*;
use super::*;

use std::num::NonZeroU32;
use core::ops::{Deref, DerefMut};
use std::cmp::Ordering;
use std::io::{Result, Error, ErrorKind};

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
#[repr(C, align(8))]
pub struct KeyName {
    pub len: u8,
    pub data: [u8; KEY_NAME_LEN - 1],
}
impl KeyName {
    pub fn try_from_str(name: &str) -> Result<KeyName> {
        let mut alloc = [0u8; KEY_NAME_LEN - 1];
        let bytes = name.as_bytes();
        if bytes.len() > (KEY_NAME_LEN - 1) {
            Err(Error::new(ErrorKind::InvalidInput, "key name is too long"))
        } else {
            for (&src, dst) in bytes.iter().zip(alloc.iter_mut()) {
                *dst = src;
            }
            Ok(KeyName {
                len: bytes.len() as u8, // this as checked above to be short enough
                data: alloc,
            })
        }
    }
}
impl Default for KeyName {
    fn default() -> KeyName {
        KeyName {
            len: 0,
            data: [0; KEY_NAME_LEN - 1]
        }
    }
}

/// On-disk representation of the Key. Note that the storage on disk is mis-aligned relative
/// to Rust's expecatation of in-RAM format, so any deserialization must essentially come with
/// a copy step to re-align the record to meet Rust's placement rules.
#[repr(C, align(8))]
pub(crate) struct KeyDescriptor {
    /// virtual address of the key's start
    pub(crate) start: u64,
    /// length of the key's stored data
    pub(crate) len: u64,
    /// amount of space reserved for the key. Must be >= len.
    pub(crate) reserved: u64,
    /// Reserved for flags on the record entry
    pub(crate) flags: KeyFlags,
    /// Access count to the key
    pub(crate) age: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    pub(crate) name: KeyName,
}
impl Default for KeyDescriptor {
    fn default() -> Self {
        KeyDescriptor {
            start: 0,
            len: 0,
            reserved: 0,
            flags: KeyFlags(0),
            age: 0,
            name: KeyName::default(),
        }
    }
}
impl Deref for KeyDescriptor {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const KeyDescriptor as *const u8, core::mem::size_of::<KeyDescriptor>())
                as &[u8]
        }
    }
}
impl DerefMut for KeyDescriptor {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut KeyDescriptor as *mut u8, core::mem::size_of::<KeyDescriptor>())
                as &mut [u8]
        }
    }
}

/// In-RAM representation of a key. This file defines the storage for the KeyCacheEntry; most of the structure
/// manipulations happen inside `dictionary.rs`, in part because to locate a Key in absolute memory space you need
/// to know what Dictionary it comes from. This is a point to consider for a refactor: if we pull some info about
/// the containing Dictionary into the key, we could associate more methods with the data structure. However, this
/// means duplicating the dictionary index, a field that can then get out of sync.
pub(crate) struct KeyCacheEntry {
    pub(crate) start: u64,
    pub(crate) len: u64,
    pub(crate) reserved: u64,
    pub(crate) flags: KeyFlags,
    pub(crate) age: u32,
    /// this is an ephemeral time since boot, not commited to disk, used only for deciding which entries to evict
    pub(crate) atime: u64,
    /// the current on-disk index of the KeyCacheEntry item, enumerated as "0" being the Dict descriptor and
    /// "1" being the first valid key. This is used to find the location of the key's metadata as stored on disk;
    /// it has nothing to do with where the data itself is stored (that's derived from `start`).
    pub(crate) descriptor_index: NonZeroU32,
    /// indicates if the descriptor cache entry is currently synchronized with what's on disk. Does not imply anything about the data,
    /// but if the `data` field is None then there is nothing to in cache to be dirtied.
    pub(crate) clean: bool,
    /// if Some, contains the keys data contents. if None, you must refer to the disk contents to retrieve it.
    /// Current rule: "small" keys always have their data "hot"; large keys may often not keep their data around.
    pub(crate) data: Option<KeyCacheData>,
}
impl KeyCacheEntry {
    /// Given a base offset of the dictionary containing the key, compute the starting VirtAddr of the key itself.
    pub(crate) fn descriptor_vaddr(&self, dict_offset: VirtAddr) -> VirtAddr {
        VirtAddr::new(dict_offset.get() + ((self.descriptor_index.get() as u64) * DK_STRIDE as u64)).unwrap()
    }
    /// Computes the modular position of the KeyDescriptor within a vpage.
    #[allow(dead_code)] // I feel like we should have been calling this /somewhere/ at some point in time, but I probably just re-wrote the math long-hand.
    pub(crate) fn descriptor_modulus(&self) -> usize {
        (self.descriptor_index.get() as usize) % (VPAGE_SIZE / DK_STRIDE)
    }
    /// Computes the vpage offset as measured from the start of the dictionary storage region
    pub(crate) fn descriptor_vpage_num(&self) -> usize {
        (self.descriptor_index.get() as usize) / DK_PER_VPAGE
    }
    /// returns the list of large-pool virtual pages belonging to this entry, if any.
    pub(crate) fn large_pool_vpages(&self) -> Vec::<VirtAddr> {
        let mut vpages = Vec::<VirtAddr>::new();
        if self.start >= LARGE_POOL_START {
            for vbase in (self.start..self.start + self.reserved).step_by(VPAGE_SIZE) {
                vpages.push(VirtAddr::new((vbase / VPAGE_SIZE as u64) * VPAGE_SIZE as u64).unwrap());
            }
        }
        vpages
    }
    /// returns a rough measure of the amount of data consumed by a given entry. It's not meant to
    /// be absolutely accurate, but it should give an accurate impression of the relative size of
    /// different cache entries. This metric is used to help decide which entries to discard.
    pub(crate) fn size(&self) -> usize {
        let data_size = match &self.data {
            None => 0,
            Some(kcd) => match kcd {
                KeyCacheData::Small(ksd) => ksd.data.len(),
                KeyCacheData::Large(kld) => kld.data.len(),
            }
        };
        core::mem::size_of::<KeyCacheEntry>() + data_size
    }
    pub(crate) fn atime(&self) -> u64 { self.atime }
    pub(crate) fn set_atime(&mut self, atime: u64) { self.atime = atime; }
}

pub (crate) enum KeyCacheData {
    Small(KeySmallData),
    // the "Medium" type has a region reserved for it, but we haven't coded a handler for it.
    #[allow(dead_code)] // Large data caching isn't implemented, so of course, we don't ever create this type
    Large(KeyLargeData),
}
/// Small data is optimized for low overhead, and always represent a complete copy of the data.
pub(crate) struct KeySmallData {
    pub clean: bool,
    pub(crate) data: Vec::<u8>,
}
/// This can hold just a portion of a large key's data. For now, we now essentially manually
/// encode a sub-slice in parts, but, later on we could get more clever and start to cache
/// multiple disjoint portions of a large key's data...
#[allow(dead_code)] // large key caching is not yet implemented
pub(crate) struct KeyLargeData {
    pub clean: bool,
    pub(crate) start: u64,
    pub(crate) data: Vec::<u8>,
}

/// A storage pool for data that is strictly smaller than one VPAGE. These element are serialized
/// and stored inside the "small data pool" area.
pub(crate) struct KeySmallPool {
    // location of data within the Small memory region. Index is in units of SMALL_CAPACITY. (this should be encoded in the vector position)
    //pub(crate) index: u32,
    /// list of data actually stored within the pool - resolve against `keys` HashMap.
    pub(crate) contents: Vec::<String>,
    /// keeps track of the available space within the pool, avoiding an expensive lookup every time we want to query the available space
    pub(crate) avail: u16,
    /// if false, contents are modified and need syncing to disk
    pub(crate) clean: bool,
    /// if true, some elements of this pool were evicted to make room in RAM
    pub(crate) evicted: bool,
}
impl KeySmallPool {
    pub(crate) fn new() -> KeySmallPool {
        KeySmallPool {
            contents: Vec::<String>::new(),
            avail: SMALL_CAPACITY as u16,
            clean: false,
            evicted: false,
        }
    }
}
/// a bookkeeping structrue to put into a max-heap to figure out who has the most available space
#[derive(Eq)]
pub(crate) struct KeySmallPoolOrd {
    pub(crate) avail: u16,
    pub(crate) index: usize,
}
// only compare based on the amount of data used
impl Ord for KeySmallPoolOrd {
    fn cmp(&self, other: &Self) -> Ordering {
        self.avail.cmp(&other.avail)
    }
}
impl PartialOrd for KeySmallPoolOrd {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for KeySmallPoolOrd {
    fn eq(&self, other: &Self) -> bool {
        self.avail == other.avail
    }
}
