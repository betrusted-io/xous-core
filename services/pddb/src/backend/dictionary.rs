use crate::api::*;
use super::*;

use core::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use std::convert::TryInto;
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use aes_gcm_siv::aead::{Aead, Payload};
use std::iter::IntoIterator;
use std::collections::{HashMap, BinaryHeap};
use std::io::{Result, Error, ErrorKind};
use bitfield::bitfield;

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct DictFlags(u32);
    impl Debug;
    pub valid, set_valid: 0;
}

/// stashed copy of a decrypted page. The copy here must always match
/// what's actually on disk; do not mutate it and expect it to sync with the disk.
/// Remember to invalidate this if the data are
/// This is stored with the journal number on top.
/// What the four possibilities of cache vs pp mean:
/// Some(cache) & Some(cache_pp) -> valid cache and pp
/// None(cache) & Some(cache_pp) -> the page was allocated; but never used, or was erased (it's free for you to use it); alternately, it was corrupted
/// Some(cache) & None(cache_pp) -> invalid, internal error
/// None(cache) & None(cache_pp) -> the basis mapping didn't exist: we've never requested this page before.
pub(crate) struct PlaintextCache {
    /// a page of data, stored with the Journal rev on top
    pub(crate) data: Option<Vec::<u8>>,
    /// the page the cache corresponds to
    pub(crate) tag: Option<PhysPage>,
}
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
    /// a cached copy of the next free key slot, expressed in units of DK_STRIDE, with 0 being the first valid key offset
    pub(crate) free_key_offset: Option<u32>,
    /// set if synced to disk. should be cleared if the dict is modified, and/or if a subordinate key descriptor is modified.
    pub(crate) clean: bool,
    /// track modification count
    pub(crate) age: u32,
    /// copy of the flags entry on the Dict on-disk
    pub(crate) flags: DictFlags,
    /// small pool data. index corresponds to portion on disk. This structure is built up as the dictionary is
    /// read in, and is the "master" for tracking purposes. We always fill this from index 0 and go up; if a KeySmallPool
    /// goes completely empty, the entry should still exist but indicate that it's got space. Thus if a key was found allocated
    /// to the Nth index position, but the previous N-1 positions are empty, the only way we could have gotten there was if we
    /// had allocated lots of small data, filled upo the pool to the Nth position, and then deleted all of that prior data.
    /// This situation could create pathologies in the memory usage overhead of the small_pool, which until we have a "defrag"
    /// operation for the small pool, we may just have to live with.
    pub(crate) small_pool: Vec<KeySmallPool>,
    /// free space of each small pool element. It's a collection of free space along with the Vec index of the small_pool.
    /// We don't keep the KeySmallPool itself in the small_pool_free directly because it's presumed to be more common
    /// that we want to index the data, than it is to need to ask the question of who has the most space free.
    /// This stays in lock-step with the small_pool data because we do a .pop() to get the target vector from the small_pool_free,
    /// then we modify the pool item, and then we .push() it back into the heap (or if it doesn't fit at all we allocate a new
    /// entry and return the original item plus the new one to the heap).
    pub(crate) small_pool_free: BinaryHeap<KeySmallPoolOrd>,
    /// copy of our AAD, for convenience
    pub(crate) aad: Vec::<u8>,
}
impl DictCacheEntry {
    pub fn new(dict: Dictionary, index: usize, aad: &Vec<u8>) -> DictCacheEntry {
        let mut my_aad = Vec::<u8>::new();
        for &b in aad.iter() {
            my_aad.push(b);
        }
        DictCacheEntry {
            index: index as u32,
            keys: HashMap::<String, KeyCacheEntry>::new(),
            key_count: dict.num_keys,
            free_key_offset: None,
            clean: true,
            age: dict.age,
            flags: dict.flags,
            small_pool: Vec::<KeySmallPool>::new(),
            small_pool_free: BinaryHeap::<KeySmallPoolOrd>::new(),
            aad: my_aad,
        }
    }
    /// Populates cache entries, reporting the maximum extent of large alloc data seen so far.
    /// Requires a descriptor for the hardware, and our virtual to physical page mapping.
    pub fn fill(&mut self, hw: &mut PddbOs, v2p_map: &HashMap::<VirtAddr, PhysPage>, cipher: &Aes256GcmSiv) -> VirtAddr {
        let dict_vaddr = VirtAddr::new(self.index as u64 * DICT_VSIZE).unwrap();
        let mut try_entry = 1;
        let mut key_count = 0;
        let mut alloc_top = VirtAddr::new(LARGE_POOL_START).unwrap();

        let mut index_cache = PlaintextCache { data: None, tag: None };
        let mut data_cache = PlaintextCache { data: None, tag: None };
        while try_entry < KEY_MAXCOUNT && key_count < self.key_count {
            // cache our decryption data -- there's about 32 entries per page, and the scan is largely linear/sequential, so this should
            // be a substantial savings in effort.
            // Determine the absolute virtual address of the requested entry. It's written a little weird because
            // DK_PER_VPAGE is 32, which optimizes cleanly and removes an expensive division step
            let req_vaddr = self.index as u64 * DICT_VSIZE + ((try_entry / DK_PER_VPAGE) as u64) * VPAGE_SIZE as u64;
            if let Some(pp) = v2p_map.get(&VirtAddr::new(req_vaddr).unwrap()) {
                let mut fill_needed = false;
                if let Some(tag) = index_cache.tag {
                    if tag.page_number() != pp.page_number() {
                        fill_needed = true;
                    }
                } else if index_cache.tag.is_none() {
                    fill_needed = true;
                }
                if fill_needed {
                    index_cache.data = hw.data_decrypt_page(&cipher, &self.aad, pp);
                    index_cache.tag = Some(*pp);
                }
            } else {
                index_cache.data = None;
                index_cache.tag = None;
            }

            if index_cache.data.is_none() || index_cache.tag.is_none() {
                // somehow we hit a page where nothing was allocated (perhaps it was previously deleted?), or less likely, the data was corrupted. Note the isuse, skip past it.
                if self.free_key_offset.is_none() { self.free_key_offset = Some(try_entry as u32) }
                log::warn!("Dictionary fill op encountered an unallocated page checking entry {} in the dictionary map. Marking it for re-use.", try_entry);
                try_entry += DK_PER_VPAGE;
            } else {
                let cache = index_cache.data.as_ref().expect("Cache should be full, it was already checked...");
                let pp = index_cache.tag.as_ref().expect("PP should be in existence, it was already checked...");
                let mut keydesc = KeyDescriptor::default();
                let start = size_of::<JournalType>() + (try_entry % DK_PER_VPAGE) * DK_STRIDE;
                for (&src, dst) in cache[start..start + DK_STRIDE].iter().zip(keydesc.deref_mut().iter_mut()) {
                    *dst = src;
                }
                if keydesc.flags.valid() {
                    let mut kcache = KeyCacheEntry {
                        start: keydesc.start,
                        len: keydesc.len,
                        reserved: keydesc.reserved,
                        flags: keydesc.flags,
                        age: keydesc.age,
                        descriptor_index: NonZeroU32::new(try_entry as u32).unwrap(),
                        clean: true,
                        data: None,
                    };
                    let kname = cstr_to_string(&keydesc.name);
                    if keydesc.start + keydesc.reserved > alloc_top.get() {
                        // if the key is within the large pool space, note its allocation for the basis overall
                        alloc_top = VirtAddr::new(keydesc.start + keydesc.reserved).unwrap();
                        // nothing else needs to be done -- we don't pre-cache large key data.
                    } else if keydesc.start < SMALL_POOL_END {
                        // if the key is within the small pool space, create a bookkeeping record for it, and pre-cache its data.
                        // generate the index within the small pool based on the address
                        assert!(keydesc.start >= SMALL_POOL_START, "Small pool key descriptor has an invalid range");
                        let rebase = keydesc.start - SMALL_POOL_START;
                        let derived_dict_index = rebase / SMALL_POOL_STRIDE;
                        assert!(derived_dict_index == (self.index - 1) as u64, "Small pool key was not stored in our dictionary's designated region");
                        let pool_index = (rebase - (derived_dict_index * SMALL_POOL_STRIDE)) as usize / VPAGE_SIZE as usize;
                        while self.small_pool.len() < pool_index {
                            // fill in the pool with blank entries. In general, we should have a low amount of blank entries, but
                            // one situation where we could get a leak is if we allocate a large amount of small data, and then delete
                            // all but the most recently allocated one, leaving an orphan at a high index, which is then subsequently
                            // treated as read-only so none of the subsequent write/update ops would have occassion to move it. This would
                            // need to be remedied with a "defrag" operation, but for now, we don't have that.
                            let ksp = KeySmallPool::new();
                            self.small_pool.push(ksp);
                        }
                        let ksp = &mut self.small_pool[pool_index];
                        ksp.contents.push(cstr_to_string(&keydesc.name));
                        assert!(keydesc.reserved >= keydesc.len, "Reserved amount is less than length, this is an error!");
                        assert!(keydesc.reserved <= VPAGE_SIZE as u64, "Reserved amount is not appropriate for the small pool. Logic error in prior PDDB operation!");
                        assert!((ksp.avail as u64) < keydesc.reserved, "Total amount allocated to a small pool chunk is incorrect; suspect logic error in prior PDDB operation!");
                        ksp.avail -= keydesc.reserved as u16;
                        // note: small_pool_free is updated only after all the entries have been read in

                        // now grab the *data* referred to by this key. Maybe this is a "bad" idea -- this can really eat up RAM fast to hold
                        // all the small pool data right away, but let's give it a try and see how it works. Later on we can always skip this.
                        // manage a separate small cache for data blocks, under the theory that small keys tend to be packed together
                        let data_vaddr = (keydesc.start / VPAGE_SIZE as u64) * VPAGE_SIZE as u64;
                        if let Some(pp) = v2p_map.get(&VirtAddr::new(data_vaddr).unwrap()) {
                            let mut fill_needed = false;
                            if let Some(tag) = data_cache.tag {
                                if tag.page_number() != pp.page_number() {
                                    fill_needed = true;
                                }
                            } else if data_cache.tag.is_none() {
                                fill_needed = true;
                            }
                            if fill_needed {
                                data_cache.data = hw.data_decrypt_page(&cipher, &self.aad, pp);
                                data_cache.tag = Some(*pp);
                            }
                            // the data should all be page-aligned, so we can "just" access by the modulus
                            if let Some(page) = data_cache.data.as_ref() {
                                let start_offset = size_of::<JournalType>() + (keydesc.start % VPAGE_SIZE as u64) as usize;
                                let data = page[start_offset..start_offset + keydesc.len as usize].to_vec();
                                kcache.data = Some(KeyCacheData::Small(
                                    KeySmallData {
                                        clean: true,
                                        data
                                    }
                                ));
                            } else {
                                log::error!("Key {}'s data region at pp: {:x?} va: {:x} is unreadable", kname, pp, keydesc.start);
                            }
                        } else {
                            log::error!("Key {} lacks a page table entry. Can't read its data...", kname);
                            data_cache.data = None;
                            data_cache.tag = None;
                        }
                    }
                    self.keys.insert(kname, kcache);
                    key_count += 1;
                } else {
                    if self.free_key_offset.is_none() { self.free_key_offset = Some(try_entry as u32) }
                }
                try_entry += 1;
            }
        }
        if self.free_key_offset.is_none() { self.free_key_offset = Some(try_entry as u32) }

        // now build the small_pool_free binary heap structure
        for (index, ksp) in self.small_pool.iter().enumerate() {
            self.small_pool_free.push(KeySmallPoolOrd{index, avail: ksp.avail})
        }

        alloc_top
    }
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
    /// Synchronize a given small pool key to disk
    pub(crate) fn key_small_sync(&self, hw: &PddbOs, smallkey: &mut KeySmallPool) {
        // 1. do a quick scan to see if any key entries are dirty. If all are clean, terminate fast
        // 2. iterate through all the elements of contents
        // 3. write the data to a Vec<u8>, while updating the KeyCacheEntry with the new pointers, and marking the entries dirty as necessary
        // 4. write the data to disk
    }
}
#[derive(Debug)]
/// On-disk representation of the dictionary header.
#[repr(C, align(8))]
pub(crate) struct Dictionary {
    /// Reserved for flags on the record entry
    pub(crate) flags: DictFlags,
    /// Access count to the dicitionary
    pub(crate) age: u32,
    /// Number of keys in the dictionary
    pub(crate) num_keys: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    pub(crate) name: [u8; DICT_NAME_LEN],
}
impl Default for Dictionary {
    fn default() -> Dictionary {
        let mut flags = DictFlags(0);
        flags.set_valid(true);
        Dictionary { flags, age: 0, num_keys: 0, name: [0; DICT_NAME_LEN] }
    }
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
