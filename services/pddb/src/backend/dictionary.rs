use crate::api::*;
use super::*;

use core::cell::RefCell;
use std::rc::Rc;
use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use std::convert::TryInto;
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use aes_gcm_siv::aead::{Aead, Payload};
use std::iter::IntoIterator;
use std::collections::HashMap;
use std::io::{Result, Error, ErrorKind};

/// RAM based copy of the dictionary structures on disk.
pub(crate) struct DictCacheEntry {
    /// Use this to compute the virtual address of the dictionary's location
    /// multiply this by DICT_VSIZE to get at the virtual address. This /could/ be a
    /// NonZeroU32 type as it should never be 0. Maybe that's a thing to fix later on.
    pub(crate) index: u32,
    /// A cache of the keys within the dictionary. If the key does not exist in
    /// the cache, one should consult the on-disk copy, assuming the record is clean.
    pub(crate) keys: HashMap::<String, KeyCacheEntry>,
    /// count of total keys in the dictionary -- may be equal to or larger than the number of elements in `keys`
    pub(crate) key_count: u32,
    /// set if synced to disk. should be cleared if the dict is modified, and/or if a subordinate key descriptor is modified.
    pub(crate) clean: bool,
    /// track modification count
    pub(crate) age: u32,
    /// copy of the flags entry on the Dict on-disk
    pub(crate) flags: u32,
}
impl DictCacheEntry {
    /// Update a key entry. If the key does not already exist, it will create a new one.
    ///
    /// `key_update` will write `data` starting at `offset`, and will grow the record if data
    /// is larger than the current allocation. If `truncate` is false, the existing data past the end of
    /// the `data` written is preserved; if `truncate` is true, the excess data past the end of the written
    /// data is removed.
    ///
    /// For small records, a `key_update` call would just want to replace the entire record, so it would have
    /// an `offset` of 0, `truncate` is true, and the data would be the new data. However, the `offset` and
    /// `truncate` records are particularly useful for updating very large file streams, which can't be
    /// held entirely in RAM.
    ///
    /// Note: it is up to the higher level Basis disambiguation logic to decide the cross-basis update policy: it
    /// could either be to update only the dictionary in the latest open basis, update all dictionaries, or update a
    /// specific dictionary in a named basis. In all of these cases, the Basis resolver will have had to find the
    /// correct DictCacheEntry and issue the `key_update` to it; for multiple updates, then multiple calls to
    /// multiple DictCacheEntry are required.
    pub fn key_update(&mut self, name: &str, data: &[u8], offset: usize, truncate: bool) -> Result <()> {
        Ok(())
    }

    /// If `paranoid` is true, this function calls `key_update` with 0's for the data. In either case, it
    /// deletes the key record from the dictionary.
    pub fn key_erase(&mut self, name: &str, paranoid: bool) {

    }
}
#[derive(Debug)]
/// On-disk representation of the dictionary header.
#[repr(C, align(8))]
pub(crate) struct Dictionary {
    /// Reserved for flags on the record entry
    pub(crate) flags: u32,
    /// Access count to the dicitionary
    pub(crate) age: u32,
    /// Number of keys in the dictionary
    pub(crate) num_keys: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    pub(crate) name: [u8; DICT_NAME_LEN],
}
impl Deref for Dictionary {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Dictionary as *const u8, core::mem::size_of::<Dictionary>())
                as &[u8]
        }
    }
}
impl DerefMut for Dictionary {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Dictionary as *mut u8, core::mem::size_of::<Dictionary>())
                as &mut [u8]
        }
    }
}

/// This structure "enforces" the 127-byte stride of dict/key vpage entries
#[derive(Copy, Clone)]
pub(crate) struct DictKeyEntry {
    pub(crate) data: [u8; DK_STRIDE],
}
impl Default for DictKeyEntry {
    fn default() -> DictKeyEntry {
        DictKeyEntry {
            data: [0; DK_STRIDE]
        }
    }
}

/// This structure helps to bookkeep which slices within a DictKey virtual page need to be updated
pub(crate) struct DictKeyVpage {
    pub(crate) elements: [Option<DictKeyEntry>; VPAGE_SIZE / DK_STRIDE],
}
impl<'a> Default for DictKeyVpage {
    fn default() -> DictKeyVpage {
        DictKeyVpage {
            elements: [None; VPAGE_SIZE / DK_STRIDE],
        }
    }
}
