use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::io::{Error, ErrorKind, Result};
use std::num::NonZeroU32;

use aes_gcm_siv::Aes256GcmSiv;
use bitfield::bitfield;
#[cfg(feature = "perfcounter")]
use perflib::{PERFMETA_ENDBLOCK, PERFMETA_NONE, PERFMETA_STARTBLOCK};

use super::*;
#[cfg(feature = "perfcounter")]
use crate::FILE_ID_SERVICES_PDDB_SRC_DICTIONARY;
use crate::api::*;

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct DictFlags(u32);
    impl Debug;
    pub valid, set_valid: 0;
}

/// RAM based copy of the dictionary structures on disk. Most of the methods on this function operate on
/// keys within the Dictionary. Operations on the Dictionary itself originate from the containing Basis
/// structure.
pub(crate) struct DictCacheEntry {
    /// Use this to compute the virtual address of the dictionary's location
    /// multiply this by DICT_VSIZE to get at the virtual address. This /could/ be a
    /// NonZeroU32 type as it should never be 0. Maybe that's a thing to fix later on.
    pub(crate) index: NonZeroU32,
    /// A cache of the keys within the dictionary. If the key does not exist in
    /// the cache, one should consult the on-disk copy, assuming the record is clean.
    pub(crate) keys: HashMap<String, KeyCacheEntry>,
    /// count of total keys in the dictionary -- may be equal to or larger than the number of elements in
    /// `keys`
    pub(crate) key_count: u32,
    /// actual count of keys found -- this drifts from the actual key count due to write errors and
    /// ungraceful powerdowns
    pub(crate) found_key_count: u32,
    /// track the pool of free key indices. Wrapped in a refcell so we can work the index mechanism while
    /// updating the keys HashMap
    pub(crate) free_keys: BinaryHeap<Reverse<FreeKeyRange>>,
    /// hint for when to stop doing a brute-force search for the existence of a key in the disk records.
    /// This field is set to the max count on a new, naive record; and set only upon a sync() or a fill()
    /// call.
    pub(crate) last_disk_key_index: u32,
    /// set if synced to disk. should be cleared if the dict is modified, and/or if a subordinate key
    /// descriptor is modified.
    pub(crate) clean: bool,
    /// track modification count
    pub(crate) age: u32,
    /// copy of the flags entry on the Dict on-disk
    pub(crate) flags: DictFlags,
    /// small pool data. index corresponds to portion on disk. This structure is built up as the dictionary
    /// is read in, and is the "master" for tracking purposes. We always fill this from index 0 and go
    /// up; if a KeySmallPool goes completely empty, the entry should still exist but indicate that it's
    /// got space. Thus if a key was found allocated to the Nth index position, but the previous N-1
    /// positions are empty, the only way we could have gotten there was if we had allocated lots of
    /// small data, filled up the pool to the Nth position, and then deleted all of that prior data. This
    /// situation could create pathologies in the memory usage overhead of the small_pool, which until we
    /// have a "defrag" operation for the small pool, we may just have to live with.
    pub(crate) small_pool: Vec<KeySmallPool>,
    /// free space of each small pool element. It's a collection of free space along with the Vec index of
    /// the small_pool. We don't keep the KeySmallPool itself in the small_pool_free directly because
    /// it's presumed to be more common that we want to index the data, than it is to need to ask the
    /// question of who has the most space free. This stays in lock-step with the small_pool data because
    /// we do a .pop() to get the target vector from the small_pool_free, then we modify the pool item,
    /// and then we .push() it back into the heap (or if it doesn't fit at all we allocate a new
    /// entry and return the original item plus the new one to the heap).
    pub(crate) small_pool_free: BinaryHeap<KeySmallPoolOrd>,
    /// copy of our AAD, for convenience
    pub(crate) aad: Vec<u8>,
    /// ticktimer reference, for managing atimes
    pub(crate) created: std::time::Instant,
}
impl DictCacheEntry {
    pub fn new(dict: Dictionary, index: usize, aad: &Vec<u8>) -> DictCacheEntry {
        let mut my_aad = Vec::<u8>::new();
        for &b in aad.iter() {
            my_aad.push(b);
        }
        let mut free_keys = BinaryHeap::<Reverse<FreeKeyRange>>::new();
        free_keys.push(Reverse(FreeKeyRange {
            start: dict.free_key_index,
            run: KEY_MAXCOUNT as u32 - 1 - dict.free_key_index,
        }));
        DictCacheEntry {
            index: NonZeroU32::new(index as u32).unwrap(),
            keys: HashMap::<String, KeyCacheEntry>::new(),
            key_count: dict.num_keys,
            found_key_count: dict.num_keys,
            free_keys,
            last_disk_key_index: dict.free_key_index,
            clean: false,
            age: dict.age,
            flags: dict.flags,
            small_pool: Vec::<KeySmallPool>::new(),
            small_pool_free: BinaryHeap::<KeySmallPoolOrd>::new(),
            aad: my_aad,
            created: std::time::Instant::now(),
        }
    }

    /// Populates cache entries, reporting the maximum extent of large alloc data seen so far.
    /// Requires a descriptor for the hardware, and our virtual to physical page mapping.
    /// Does not overwrite existing cache entries, if they already exist -- only loads in ones that are
    /// missing. Todo: condense routines in common with ensure_key_entry() to make it easier to maintain.
    ///
    /// ASSUMES: all small/large pools have been sync'd prior to calling this
    pub fn fill(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        cleanup: bool,
    ) -> VirtAddr {
        let mut try_entry = 1;
        let mut key_count = 0;
        let mut alloc_top = VirtAddr::new(LARGE_POOL_START).unwrap();

        // prune invalid entries, then count how many we have
        self.keys.retain(|_name, entry| entry.flags.valid());
        let valid_keys = self.keys.len();

        assert!(
            valid_keys <= self.key_count as usize,
            "Inconsistency in key count. See note on basis/dict_count for logic on why this assert should be true..."
        );
        if valid_keys == self.key_count as usize {
            // we've got an entry for every key, so we can safely skip the deep index search
            let mut smallkeys_to_fill = Vec::<String>::new(); // work around inner mutability problem by copying the names of keys to fill
            for (name, key) in self.keys.iter() {
                // #[cfg(feature="perfcounter")]
                // hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, key_count,
                // std::line!());
                if key.flags.valid() {
                    if key.start + key.reserved > alloc_top.get() {
                        // if the key is within the large pool space, note its allocation for the basis
                        // overall
                        alloc_top = VirtAddr::new(key.start + key.reserved).unwrap();
                        // nothing else needs to be done -- we don't pre-cache large key data.
                    } else {
                        if key.data.is_none() {
                            smallkeys_to_fill.push(name.to_string());
                        }
                    }
                }
            }
            // the cache may not be very effectively used because the key order is random, but let's try
            // anyways.
            let mut data_cache = PlaintextCache { data: None, tag: None };
            for key_to_fill in smallkeys_to_fill.iter() {
                #[cfg(feature = "perfcounter")]
                hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, key_count, std::line!());
                self.try_fill_small_key(hw, v2p_map, cipher, &mut data_cache, key_to_fill);
            }
        } else {
            let mut index_cache = PlaintextCache { data: None, tag: None };
            let mut data_cache = PlaintextCache { data: None, tag: None };
            let mut errcnt = 0;
            while try_entry < KEY_MAXCOUNT && (cleanup || (key_count < self.key_count)) {
                #[cfg(feature = "perfcounter")]
                hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, key_count, std::line!());
                // cache our decryption data -- there's about 32 entries per page, and the scan is largely
                // linear/sequential, so this should be a substantial savings in effort.
                let req_vaddr = dict_indices_to_vaddr(self.index, try_entry);
                index_cache.fill(hw, v2p_map, cipher, &self.aad, VirtAddr::new(req_vaddr).unwrap());

                if index_cache.data.is_none() || index_cache.tag.is_none() {
                    // somehow we hit a page where nothing was allocated (perhaps it was previously deleted?),
                    // or less likely, the data was corrupted. Note the isuse, skip past it.
                    if (errcnt < 4) || (errcnt % 8192 == 0) {
                        log::warn!(
                            "Dictionary fill: encountered unallocated page at {} in the dictionary map. {}/{}",
                            try_entry,
                            key_count,
                            self.key_count
                        );
                    }
                    errcnt += 1;
                    try_entry += DK_PER_VPAGE;
                } else {
                    let cache_pp = index_cache
                        .tag
                        .as_ref()
                        .expect("PP should be in existence, it was already checked...");
                    let pp = v2p_map
                        .get(&VirtAddr::new(req_vaddr).unwrap())
                        .expect("dictionary PP should be in existence");
                    assert!(pp.valid(), "v2p returned an invalid page");
                    assert!(cache_pp.page_number() == pp.page_number(), "cache inconsistency error");
                    let cache =
                        index_cache.data.as_ref().expect("Cache should be full, it was already checked...");
                    let mut keydesc = KeyDescriptor::default();
                    let start = size_of::<JournalType>() + (try_entry % DK_PER_VPAGE) * DK_STRIDE;
                    for (&src, dst) in
                        cache[start..start + DK_STRIDE].iter().zip(keydesc.deref_mut().iter_mut())
                    {
                        *dst = src;
                    }
                    if keydesc.flags.valid() {
                        let kcache = KeyCacheEntry {
                            start: keydesc.start,
                            len: keydesc.len,
                            reserved: keydesc.reserved,
                            flags: keydesc.flags,
                            age: keydesc.age,
                            descriptor_index: NonZeroU32::new(try_entry as u32).unwrap(),
                            clean: true,
                            data: None,
                            atime: self.created.elapsed().as_millis() as u64,
                        };
                        let kname = std::str::from_utf8(&keydesc.name.data[..keydesc.name.len as usize])
                            .expect("key is not valid utf-8");
                        let key_exists_and_valid = if let Some(kcache) = self.keys.get(kname) {
                            kcache.flags.valid()
                        } else {
                            false
                        };
                        if !key_exists_and_valid {
                            self.keys.insert(kname.to_string(), kcache);
                            if keydesc.start + keydesc.reserved > alloc_top.get() {
                                // if the key is within the large pool space, note its allocation for the
                                // basis overall
                                alloc_top = VirtAddr::new(keydesc.start + keydesc.reserved).unwrap();
                                // nothing else needs to be done -- we don't pre-cache large key data.
                            } else {
                                // try to fill the small key cache entry details
                                self.try_fill_small_key(hw, v2p_map, cipher, &mut data_cache, &kname);
                            }
                        } else {
                            log::trace!("fill: entry already present {}", kname);
                        }
                        key_count += 1;
                    }
                    try_entry += 1;
                }
            }
            // note where the scan left off, so we don't have to brute-force it in the future
            self.last_disk_key_index = try_entry as u32;
            // note the actual number of keys found
            self.found_key_count = key_count;

            // now build the small_pool_free binary heap structure
            #[cfg(feature = "perfcounter")]
            hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, key_count, std::line!());
            self.rebuild_free_pool();
            #[cfg(feature = "perfcounter")]
            hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, key_count, std::line!());
        }
        alloc_top
    }

    /// merges the list of keys in this dict cache entry into a merge_list.
    /// The `merge_list` is used because keys are presented as a union across all open basis.
    pub(crate) fn key_list(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        merge_list: &mut BTreeSet<String>,
    ) {
        #[cfg(feature = "perfcounter")]
        hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_STARTBLOCK, 0, std::line!());
        // ensure that the key cache is filled
        if self.keys.len() < self.key_count as usize {
            self.fill(hw, v2p_map, cipher, false);
        }
        #[cfg(feature = "perfcounter")]
        hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_NONE, 0, std::line!());
        for (key, kcache) in self.keys.iter() {
            if kcache.flags.valid() {
                merge_list.insert(key.to_string());
            }
        }
        #[cfg(feature = "perfcounter")]
        hw.perf_entry(FILE_ID_SERVICES_PDDB_SRC_DICTIONARY, PERFMETA_ENDBLOCK, 0, std::line!());
    }

    /// Simply ensures we have the description of a key in cache. Only tries to load small key data.
    /// Required by meta-operations on the keys that operate only out of the cache.
    /// This shares a lot of code with the fill() routine -- we should condense the common routines
    /// to make this easier to maintain. Returns false if the disk was searched and no key was found; true
    /// if cache is hot or key was found on search.
    pub(crate) fn ensure_key_entry(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &mut HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        name_str: &str,
    ) -> bool {
        let mut data_cache = PlaintextCache { data: None, tag: None };
        // only fill if the key isn't in the cache, or if the data section has been evicted
        let needs_fill = if let Some(entry) = self.keys.get(name_str) {
            if entry.flags.valid() {
                if entry.data.is_none() {
                    self.try_fill_small_key(hw, v2p_map, cipher, &mut data_cache, name_str);
                    // it should be filled, so no further processing is needed
                    false
                } else {
                    // data is already there
                    false
                }
            } else {
                // invalid entry
                false
            }
        } else {
            // entry doesn't exist
            true
        };
        if needs_fill {
            log::debug!("searching for key {}", name_str);
            let mut try_entry = 1;
            let mut key_count = 0;
            let mut index_cache = PlaintextCache { data: None, tag: None };
            let mut warn_count = 0;
            while try_entry < KEY_MAXCOUNT
                && key_count < self.key_count
                && try_entry <= self.last_disk_key_index as usize
            {
                // cache our decryption data -- there's about 32 entries per page, and the scan is largely
                // linear/sequential, so this should be a substantial savings in effort.
                // Determine the absolute virtual address of the requested entry. It's written a little weird
                // because DK_PER_VPAGE is 32, which optimizes cleanly and removes an
                // expensive division step
                let req_vaddr = dict_indices_to_vaddr(self.index, try_entry);
                index_cache.fill(hw, v2p_map, cipher, &self.aad, VirtAddr::new(req_vaddr).unwrap());

                if index_cache.data.is_none() || index_cache.tag.is_none() {
                    // this case "should not happen" in practice, because the last_disk_key_index would either
                    // be correctly set as short by a dict_add(), or a mount() operation
                    // would have limited the extent of the search. if we are hitting
                    // this, that means the last_disk_key_index operator was not managed correctly.
                    if warn_count < 4 || (warn_count % 8192 == 0) {
                        log::warn!("expensive search op");
                    }
                    warn_count += 1;
                    try_entry += DK_PER_VPAGE;
                } else {
                    let cache =
                        index_cache.data.as_ref().expect("Cache should be full, it was already checked...");
                    let cache_pp = index_cache
                        .tag
                        .as_ref()
                        .expect("PP should be in existence, it was already checked...");
                    let pp = v2p_map
                        .get(&VirtAddr::new(req_vaddr).unwrap())
                        .expect("dictionary PP should be in existence");
                    assert!(pp.valid(), "v2p returned an invalid page");
                    assert!(cache_pp.page_number() == pp.page_number(), "cache inconsistency error");
                    let mut keydesc = KeyDescriptor::default();
                    let start = size_of::<JournalType>() + (try_entry % DK_PER_VPAGE) * DK_STRIDE;
                    // there is 1 byte of extra padding that causes the slices of keydesc.deref_mut() to be
                    // 128 bytes long instead of 127 bytes...
                    keydesc.deref_mut()[..DK_STRIDE].copy_from_slice(&cache[start..start + DK_STRIDE]);
                    let kname = std::str::from_utf8(&keydesc.name.data[..keydesc.name.len as usize])
                        .expect("key is not valid utf-8");
                    if keydesc.flags.valid() {
                        if kname == name_str {
                            log::debug!("found {} at entry {}/{}", name_str, try_entry, start);
                            let kcache = KeyCacheEntry {
                                start: keydesc.start,
                                len: keydesc.len,
                                reserved: keydesc.reserved,
                                flags: keydesc.flags,
                                age: keydesc.age,
                                descriptor_index: NonZeroU32::new(try_entry as u32).unwrap(),
                                clean: true,
                                data: None,
                                atime: self.created.elapsed().as_millis() as u64,
                            };
                            self.keys.insert(kname.to_string(), kcache);
                            self.try_fill_small_key(hw, v2p_map, cipher, &mut data_cache, &kname);
                            return true;
                        }
                        key_count += 1;
                    }
                    try_entry += 1;
                }
            }
            false
        } else {
            // the key is in the cache, but is it valid?
            if self.keys.get(name_str).expect("inconsistent state").flags.valid() {
                true
            } else {
                // not valid -- it's an erased key, but waiting to be synced to disk. Return that the key
                // wasn't found.
                false
            }
        }
    }

    fn try_fill_small_key(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        data_cache: &mut PlaintextCache,
        key_name: &str,
    ) {
        let mut filled_index: Option<usize> = None;
        if let Some(kcache) = self.keys.get_mut(key_name) {
            if !kcache.flags.valid() {
                // nothing to fill, the key entry isn't valid
                return;
            }
            if let Some(pool_index) = small_storage_index_from_key(&kcache, self.index) {
                // if the key is within the small pool space, create a bookkeeping record for it, and
                // pre-cache its data. generate the index within the small pool based on the
                // address
                while self.small_pool.len() < pool_index + 1 {
                    // fill in the pool with blank entries. In general, we should have a low amount of blank
                    // entries, but one situation where we could get a leak is if we
                    // allocate a large amount of small data, and then delete all but the
                    // most recently allocated one, leaving an orphan at a high index, which is then
                    // subsequently treated as read-only so none of the subsequent
                    // write/update ops would have occassion to move it. This would
                    // need to be remedied with a "defrag" operation, but for now, we don't have that.
                    let ksp = KeySmallPool::new();
                    self.small_pool.push(ksp);
                }
                let ksp = &mut self.small_pool[pool_index];
                if !ksp.clean || ksp.evicted {
                    if !ksp.contents.contains(&key_name.to_string()) {
                        log::trace!("creating ksp entry for {}", key_name);
                        ksp.contents.push(key_name.to_string());
                        assert!(
                            kcache.reserved >= kcache.len,
                            "Reserved amount is less than length, this is an error!"
                        );
                        assert!(
                            kcache.reserved <= VPAGE_SIZE as u64,
                            "Reserved amount is not appropriate for the small pool. Logic error in prior PDDB operation!"
                        );
                        log::trace!("avail: {} reserved: {}", ksp.avail, kcache.reserved);
                        assert!(
                            (ksp.avail as u64) >= kcache.reserved,
                            "Total amount allocated to a small pool chunk is incorrect; suspect logic error in prior PDDB operation!"
                        );
                        ksp.avail -= kcache.reserved as u16;
                        // note: small_pool_free is updated only after all the entries have been read in
                    } else {
                        // if the entry was previously created, don't update the metadata.
                        // This might not be the best place for this note, but, as a reminder to myself,
                        // the small pool code is written assuming you scan through all the keys and fill it
                        // into RAM at least once. Once you've done that, you can evict cached data to free up
                        // some space, but it's sort of dumb that you have to do that
                        // in the first place. This is going to lead to a crisis once
                        // the small pool data exceeds the available RAM -- which isn't too far from now.
                        // This will be tracked in issue #109.
                        log::trace!("refilling data only for {}", key_name);
                    }

                    // now grab the *data* referred to by this key. Maybe this is a "bad" idea -- this can
                    // really eat up RAM fast to hold all the small pool data right away,
                    // but let's give it a try and see how it works. Later on we can always skip this.
                    // manage a separate small cache for data blocks, under the theory that small keys tend to
                    // be packed together
                    let data_vaddr = small_storage_base_vaddr_from_indices(self.index, pool_index);
                    data_cache.fill(hw, v2p_map, cipher, &self.aad, VirtAddr::new(data_vaddr).unwrap());
                    if let Some(page) = data_cache.data.as_ref() {
                        let start_offset =
                            size_of::<JournalType>() + (kcache.start % VPAGE_SIZE as u64) as usize;
                        let mut data = page[start_offset..start_offset + kcache.len as usize].to_vec();
                        data.reserve_exact((kcache.reserved - kcache.len) as usize);
                        kcache.data = Some(KeyCacheData::Small(KeySmallData { clean: true, data }));
                        filled_index = Some(pool_index);
                    } else {
                        log::error!(
                            "Key {}'s data region at pp: {:x?} va: {:x} is unreadable",
                            key_name,
                            data_cache.tag,
                            kcache.start
                        );
                    }
                } else {
                    log::debug!("pool is clean for key {} / index {}, skipping fill", key_name, pool_index);
                }
            }
        } else {
            log::error!(
                "try_fill_small_key() can only be called after the key cache entry has been allocated"
            );
            panic!("consistency error");
        }
        if let Some(index) = filled_index {
            // check to see if this fill can clear the eviction status of the entire pool
            let ksp = &mut self.small_pool[index];
            let mut all_present = true;
            for key in ksp.contents.iter() {
                if let Some(kcache) = self.keys.get(key) {
                    if kcache.flags.valid() {
                        if kcache.data.is_none() {
                            all_present = false;
                            break;
                        }
                    }
                }
            }
            if all_present {
                ksp.evicted = false;
            }
        }
    }

    /// To be called only to re-fill key entries whose data have been evicted by a prune.
    pub(crate) fn refill_small_key(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        data_cache: &mut PlaintextCache,
        key_name: &str,
    ) {
        if let Some(kcache) = self.keys.get_mut(key_name) {
            if kcache.flags.valid() {
                // invalid keys were previously deleted, and have nothing to fill
                if let Some(pool_index) = small_storage_index_from_key(&kcache, self.index) {
                    let ksp = &mut self.small_pool[pool_index];
                    assert!(
                        ksp.contents.contains(&key_name.to_string()),
                        "refill called on a non-existent key. This is an illegal state."
                    );
                    // now grab the *data* referred to by this key. Maybe this is a "bad" idea -- this can
                    // really eat up RAM fast to hold all the small pool data right away,
                    // but let's give it a try and see how it works. Later on we can always skip this.
                    // manage a separate small cache for data blocks, under the theory that small keys tend to
                    // be packed together
                    let data_vaddr = small_storage_base_vaddr_from_indices(self.index, pool_index);
                    data_cache.fill(hw, v2p_map, cipher, &self.aad, VirtAddr::new(data_vaddr).unwrap());
                    if let Some(page) = data_cache.data.as_ref() {
                        let start_offset =
                            size_of::<JournalType>() + (kcache.start % VPAGE_SIZE as u64) as usize;
                        let mut data = page[start_offset..start_offset + kcache.len as usize].to_vec();
                        data.reserve_exact((kcache.reserved - kcache.len) as usize);
                        kcache.data = Some(KeyCacheData::Small(KeySmallData { clean: true, data }));
                    } else {
                        log::error!(
                            "Key {}'s data region at pp: {:x?} va: {:x} is unreadable",
                            key_name,
                            data_cache.tag,
                            kcache.start
                        );
                    }
                }
            }
        } else {
            log::error!("refill_small_key() can only be called after the key cache entry has been allocated");
            panic!("consistency error");
        }
    }

    /// Update a key entry. If the key does not already exist, it will create a new one.
    ///
    /// Assume: the caller has called ensure_fast_space_alloc() to make sure there is sufficient space for the
    /// inserted key before calling.
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
    /// Note: it is up to the higher level Basis disambiguation logic to decide the cross-basis update policy:
    /// it could either be to update only the dictionary in the latest open basis, update all
    /// dictionaries, or update a specific dictionary in a named basis. In all of these cases, the Basis
    /// resolver will have had to find the correct DictCacheEntry and issue the `key_update` to it; for
    /// multiple updates, then multiple calls to multiple DictCacheEntry are required.
    pub fn key_update(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &mut HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        name: &str,
        data: &[u8],
        offset: usize,
        alloc_hint: Option<usize>,
        truncate: bool,
        large_alloc_ptr: PageAlignedVa,
    ) -> Result<PageAlignedVa> {
        self.age = self.age.saturating_add(1);
        self.clean = false;
        if self.ensure_key_entry(hw, v2p_map, cipher, name) {
            let kcache = self.keys.get_mut(name).expect("Entry was assured, but then not there!");
            kcache.set_atime(self.created.elapsed().as_millis() as u64);
            kcache.clean = false;
            // the update isn't going to fit in the reserved space, remove it, and re-insert it with an
            // entirely new entry.
            if kcache.reserved < (data.len() + offset) as u64 {
                if kcache.start < SMALL_POOL_END {
                    // this started life as a small key. the algorithm is to remove and retry upon extend.
                    // allocate a new vector that contains the *entire* data contents (not just the updated
                    // portion), and re-insert it
                    let mut update_data = Vec::<u8>::with_capacity(data.len() + offset);
                    if let Some(KeyCacheData::Small(old_data)) = &kcache.data {
                        for &b in &old_data.data {
                            update_data.push(b);
                        }
                    } else {
                        log::error!("expected a small key, but the data type was incorrect or not in cache");
                        panic!("expected a small key, but the data type was incorrect or not in cache");
                    }
                    // extend the vector out to the expected length, so the indexed slice in the next step
                    // doesn't panic
                    while update_data.len() < offset + data.len() {
                        update_data.push(0);
                    }
                    for (&src, dst) in data.iter().zip(update_data[offset..].iter_mut()) {
                        *dst = src
                    }
                    log::debug!("update/extend: removing {}", name);
                    // now remove the old key entirely
                    self.key_remove(hw, v2p_map, cipher, name, false);
                    self.sync_small_pool(hw, v2p_map, cipher);
                    if update_data.len() > 4 {
                        // just make sure that this log call doesn't fail on an index violation...
                        log::debug!(
                            "update/extend: re-adding {} with data len {}: {:x?}...",
                            name,
                            update_data.len(),
                            &update_data[..4]
                        );
                    }
                    // and re-add it with the extended data; if it's no longer a small key after this, it'll
                    // be handled inside this call.
                    return self.key_update(
                        hw,
                        v2p_map,
                        cipher,
                        name,
                        &update_data,
                        0,
                        alloc_hint,
                        truncate,
                        large_alloc_ptr,
                    );
                } else {
                    // large data sets will need more physical pages to be allocated for the new file length.
                    // It's a hard error if the requested size goes beyond the
                    // pre-allocated virtual memory space limit.
                    if (offset + data.len()) as u64 > LARGE_FILE_MAX_SIZE {
                        log::error!(
                            "Requested file update would exceed our file size limit. Asked: {}, limit: {}",
                            offset + data.len(),
                            LARGE_FILE_MAX_SIZE
                        );
                        panic!("Updated file size is greater than the large file size limit.");
                    }
                    assert!(
                        (kcache.start + kcache.reserved) % VPAGE_SIZE as u64 == 0,
                        "large space allocation rules were violated"
                    );
                    let new_reservation_abs_addr =
                        PageAlignedVa::from(kcache.start + offset as u64 + data.len() as u64).as_u64();
                    for vpage_addr in
                        ((kcache.start + kcache.reserved)..new_reservation_abs_addr).step_by(VPAGE_SIZE)
                    {
                        // ensures that a physical page entry exists for every new virtual address required by
                        // the extended key
                        v2p_map.entry(VirtAddr::new(vpage_addr).unwrap()).or_insert_with(|| {
                            let mut ap = hw
                                .try_fast_space_alloc()
                                .expect("No free space to allocate additional large key storage");
                            ap.set_valid(true);
                            ap
                        });
                    }
                    kcache.reserved = new_reservation_abs_addr - kcache.start; // convert absolute address to an actual length
                    kcache.clean = false;
                    // now retry the call, with the new physical page reservations in place
                    return self.key_update(
                        hw,
                        v2p_map,
                        cipher,
                        name,
                        data,
                        offset,
                        alloc_hint,
                        truncate,
                        large_alloc_ptr,
                    );
                }
            }
            // the key exists, *and* there's sufficient space for the data
            if kcache.start < SMALL_POOL_END {
                log::debug!("doing data update of {}", name);
                kcache.age = kcache.age.saturating_add(1);
                kcache.clean = false;
                // it's a small key; note that we didn't consult the *size* of the key to determine its pool
                // type: small-sized keys can still end up in the "large" space if the small
                // pool allocation is exhausted.
                match kcache.data.as_mut() {
                    Some(KeyCacheData::Small(cache_data)) => {
                        cache_data.clean = false;
                        // grow the data cache to accommodate the necessary length; this should be efficient
                        // because we reserved space when the vector was allocated
                        while cache_data.data.len() < data.len() + offset {
                            cache_data.data.push(0);
                        }
                        for (&src, dst) in data.iter().zip(cache_data.data[offset..].iter_mut()) {
                            *dst = src;
                        }
                    }
                    Some(KeyCacheData::Large(_)) => {
                        panic!("Key allocated to small area but its cache data was not of the small type");
                    }
                    None => {
                        // cache data was evicted, fill it
                        assert!(kcache.flags.valid(), "Entry to be filled should be valid");
                        let pool_index =
                            small_storage_index_from_key(&kcache, self.index).expect("pool should be valid");
                        let ksp = &mut self.small_pool[pool_index];
                        if !ksp.contents.contains(&name.to_string()) {
                            log::warn!(
                                "illegal state: key_update fill key pool contents: {:?}, missing {}",
                                ksp.contents,
                                name
                            );
                            panic!("Entry to fill isn't in the expected pool");
                        }
                        let data_vaddr = small_storage_base_vaddr_from_indices(self.index, pool_index);
                        let mut data_cache = PlaintextCache { data: None, tag: None };
                        data_cache.fill(hw, v2p_map, cipher, &self.aad, VirtAddr::new(data_vaddr).unwrap());
                        if let Some(page) = data_cache.data.as_ref() {
                            let start_offset =
                                size_of::<JournalType>() + (kcache.start % VPAGE_SIZE as u64) as usize;
                            let mut data = page[start_offset..start_offset + kcache.len as usize].to_vec();
                            data.reserve_exact((kcache.reserved - kcache.len) as usize);
                            kcache.data = Some(KeyCacheData::Small(KeySmallData { clean: true, data }));
                        } else {
                            let error = format!(
                                "Key {}'s data region at pp: {:x?} va: {:x} is unreadable",
                                name, data_cache.tag, kcache.start
                            );
                            panic!("{}", error);
                        }
                    }
                }
                // check if we grew the length
                if kcache.len < (data.len() + offset) as u64 {
                    kcache.len = (data.len() + offset) as u64;
                } else if truncate {
                    kcache.len = (data.len() + offset) as u64;
                }
                // mark the storage pool entry as dirty, too.
                let pool_index = small_storage_index_from_key(&kcache, self.index).expect("index missing");
                self.small_pool[pool_index].clean = false;
                // note: there is no need to update small_pool_free because the reserved size did not change.
            } else {
                // it's a large key
                if let Some(_kcd) = &kcache.data {
                    unimplemented!("caching is not yet implemented for large data sets");
                } else {
                    kcache.age = kcache.age.saturating_add(1);
                    kcache.clean = false;
                    /* // this was for debugging a patching bug -- OK to remove
                    if data.len() == 4 {
                        use std::convert::TryInto;
                        log::info!("patching checksum: {:x} at offset {}", u32::from_le_bytes(data.try_into().unwrap()), offset);
                    }*/
                    // 1. handle unaligned start offsets
                    let mut written: usize = 0;
                    if ((kcache.start + offset as u64 + written as u64) % VPAGE_SIZE as u64) != 0 {
                        let start_vpage_addr =
                            ((kcache.start + offset as u64) / VPAGE_SIZE as u64) * VPAGE_SIZE as u64;
                        let pp = v2p_map
                            .get(&VirtAddr::new(start_vpage_addr).unwrap())
                            .expect("large key data allocation missing");
                        assert!(pp.valid(), "v2p returned an invalid page");
                        let mut pt_data = match hw.data_decrypt_page(&cipher, &self.aad, pp) {
                            Some(data) => data,
                            None => {
                                // this case is triggered by the following circumstance:
                                //  - we reserved data that includes this current page
                                //  - up until now, we've only written data into the previous page (so this
                                //    page is not initialized -- it's garbage)
                                //  - we just issued an update that causes the data to touch this page for the
                                //    first time
                                // in response to this, we allocate a fresh page of 0's.
                                log::debug!(
                                    "Reserved and uninitialized page encountered updating large block: {} {:x}..{}->{}; update @{}..{}",
                                    name,
                                    kcache.start,
                                    kcache.len,
                                    kcache.reserved,
                                    offset,
                                    data.len()
                                );
                                let mut d = vec![0u8; VPAGE_SIZE + size_of::<JournalType>()];
                                for (&src, dst) in (hw.trng_u32() % JOURNAL_RAND_RANGE)
                                    .to_le_bytes()
                                    .iter()
                                    .zip(d[..size_of::<JournalType>()].iter_mut())
                                {
                                    *dst = src;
                                }
                                d
                            }
                        };
                        if offset > 0 {
                            log::trace!(
                                "patching offset {}, total length {}, data length {}",
                                offset % VPAGE_SIZE,
                                kcache.len,
                                data.len()
                            );
                        }
                        for (&src, dst) in data[written..]
                            .iter()
                            .zip(pt_data[size_of::<JournalType>() + (offset % VPAGE_SIZE)..].iter_mut())
                        {
                            *dst = src;
                            written += 1;
                        }
                        if written < data.len() {
                            assert!(
                                (kcache.start + offset as u64 + written as u64) % VPAGE_SIZE as u64 == 0,
                                "alignment algorithm failed"
                            );
                        }
                        hw.data_encrypt_and_patch_page(cipher, &self.aad, &mut pt_data, &pp);
                    }
                    // 2. do the rest
                    while written < data.len() {
                        let vpage_addr = ((kcache.start + written as u64 + offset as u64)
                            / VPAGE_SIZE as u64)
                            * VPAGE_SIZE as u64;
                        let pp = v2p_map
                            .get(&VirtAddr::new(vpage_addr).unwrap())
                            .expect("large key data allocation missing");
                        assert!(pp.valid(), "v2p returned an invalid page");
                        if data.len() - written >= VPAGE_SIZE {
                            // overwrite whole pages without decryption
                            let mut block = [0u8; VPAGE_SIZE + size_of::<JournalType>()];
                            for (&src, dst) in (hw.trng_u32() % JOURNAL_RAND_RANGE)
                                .to_le_bytes()
                                .iter()
                                .zip(block[..size_of::<JournalType>()].iter_mut())
                            {
                                *dst = src;
                            }
                            for (&src, dst) in
                                data[written..].iter().zip(block[size_of::<JournalType>()..].iter_mut())
                            {
                                *dst = src;
                                written += 1;
                            }
                            hw.data_encrypt_and_patch_page(cipher, &self.aad, &mut block, pp);
                        } else {
                            // handle partial trailing pages
                            if let Some(pt_data) = hw.data_decrypt_page(&cipher, &self.aad, pp).as_mut() {
                                for (&src, dst) in
                                    data[written..].iter().zip(pt_data[size_of::<JournalType>()..].iter_mut())
                                {
                                    *dst = src;
                                    written += 1;
                                }
                                hw.data_encrypt_and_patch_page(cipher, &self.aad, pt_data, pp);
                            } else {
                                // page didn't exist, initialize it with 0's and merge the tail end.
                                let mut pt_data = [0u8; VPAGE_SIZE + size_of::<JournalType>()];
                                for (&src, dst) in (hw.trng_u32() % JOURNAL_RAND_RANGE)
                                    .to_le_bytes()
                                    .iter()
                                    .zip(pt_data[..size_of::<JournalType>()].iter_mut())
                                {
                                    *dst = src;
                                }
                                for (&src, dst) in
                                    data[written..].iter().zip(pt_data[size_of::<JournalType>()..].iter_mut())
                                {
                                    *dst = src;
                                    written += 1;
                                }
                                hw.data_encrypt_and_patch_page(cipher, &self.aad, &mut pt_data, pp);
                            }
                        }
                    }
                    log::trace!("data written: {}, data requested to write: {}", written, data.len());
                    assert!(
                        written == data.len(),
                        "algorithm problem -- didn't write all the data we thought we would"
                    );
                    // 3. truncate or extend
                    // check if we grew the length; extend the length by exactly enough if so.
                    if kcache.len < (data.len() + offset) as u64 {
                        kcache.len = (data.len() + offset) as u64;
                    } else if truncate {
                        // discard all whole pages after written+offset, and reset the reserved field to the
                        // smaller size.
                        log::trace!("PageAligned VA components: {}, {}", written, offset);
                        let vpage_end_offset = PageAlignedVa::from((written + offset) as u64);
                        if (vpage_end_offset.as_u64() - kcache.start) > kcache.reserved {
                            for vpage in (vpage_end_offset.as_u64()..kcache.start + kcache.reserved)
                                .step_by(VPAGE_SIZE)
                            {
                                if let Some(pp) = v2p_map.get_mut(&VirtAddr::new(vpage).unwrap()) {
                                    assert!(pp.valid(), "v2p returned an invalid page");
                                    log::trace!("fast_space_free key_update {} before", pp.journal());
                                    hw.fast_space_free(pp);
                                    assert!(pp.valid() == false, "pp is still marked as valid!");
                                }
                            }
                            kcache.reserved = vpage_end_offset.as_u64() - kcache.start;
                            kcache.clean = false;
                            kcache.len = (data.len() + offset) as u64;
                        }
                    }
                }
            }
        } else {
            // key does not exist (or was previously erased) -- create one or replace the erased one.
            // try to fit the key in the small pool first
            if ((data.len() + offset) < SMALL_CAPACITY)
                && (alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT) < SMALL_CAPACITY)
            {
                log::debug!("creating small key {}", name);
                // handle the case that we're a brand new dictionary and no small keys have ever been stored
                // before.
                if self.small_pool.len() == 0 {
                    self.small_pool.push(KeySmallPool::new());
                    self.rebuild_free_pool();
                }
                let pool_candidate =
                    self.small_pool_free.pop().expect("Free pool was allocated & rebuilt, but still empty.");
                let mut reservation = if alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT) > data.len() + offset {
                    alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT)
                } else {
                    data.len() + offset
                };
                if reservation == 0 {
                    // this case happens if someone tries to just create an empty key entry without giving an
                    // alloc hint
                    reservation = 1; // at least make a reservation for 1 byte of data
                }
                let index = if pool_candidate.avail as usize >= reservation {
                    // it fits in the current candidate slot, use this as the index
                    let ksp = &mut self.small_pool[pool_candidate.index];
                    ksp.contents.push(name.to_string());
                    ksp.avail -= reservation as u16;
                    ksp.clean = false;
                    log::debug!("ksp.clean = false {}", name);
                    self.small_pool_free
                        .push(KeySmallPoolOrd { avail: ksp.avail, index: pool_candidate.index });
                    pool_candidate.index
                } else {
                    self.small_pool_free.push(pool_candidate);
                    // allocate a new small pool slot
                    let mut ksp = KeySmallPool::new();
                    ksp.contents.push(name.to_string());
                    ksp.avail -= reservation as u16;
                    ksp.clean = false;
                    log::debug!("ksp.clean = false {}", name);
                    // update the free pool with the current candidate
                    // we don't subtract 1 from len because we're about to push the ksp onto the end of the
                    // small_pool, consuming it
                    self.small_pool_free
                        .push(KeySmallPoolOrd { avail: ksp.avail, index: self.small_pool.len() });
                    self.small_pool.push(ksp);
                    // the actual location is at len-1 now because we have done the push
                    self.small_pool.len() - 1
                };
                let mut kf = KeyFlags(0);
                kf.set_valid(true);
                kf.set_unresolved(true);
                let mut alloc_data = Vec::<u8>::new();
                for _ in 0..offset {
                    alloc_data.push(0);
                }
                for &b in data {
                    alloc_data.push(b);
                }
                let descriptor_index = if let Some(di) = self.get_free_key_index() {
                    di
                } else {
                    return Err(Error::new(ErrorKind::OutOfMemory, "Ran out of key indices in dictionary"));
                };
                /* log::info!("storing in index {:?}", descriptor_index);
                for fk in self.free_keys.iter() {
                    log::info!("fk start: {} run: {}", fk.0.start, fk.0.run);
                } */
                let kcache = KeyCacheEntry {
                    start: small_storage_base_vaddr_from_indices(self.index, index),
                    len: (data.len() + offset) as u64,
                    reserved: reservation as u64,
                    flags: kf,
                    age: 0,
                    descriptor_index,
                    clean: false,
                    data: Some(KeyCacheData::Small(KeySmallData { clean: false, data: alloc_data })),
                    atime: self.created.elapsed().as_millis() as u64,
                };
                self.keys.insert(name.to_string(), kcache);
                self.key_count += 1;
            } else {
                log::debug!("creating large key: {}", data.len());
                // it didn't fit in the small pool, stick it in the big pool.
                let reservation =
                    PageAlignedVa::from(if alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT) > data.len() + offset {
                        alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT)
                    } else {
                        data.len() + offset
                    });
                let mut kf = KeyFlags(0);
                kf.set_valid(true);
                let descriptor_index = if let Some(di) = self.get_free_key_index() {
                    di
                } else {
                    return Err(Error::new(ErrorKind::OutOfMemory, "Ran out of key indices in dictionary"));
                };
                let kcache = KeyCacheEntry {
                    start: large_alloc_ptr.as_u64(),
                    len: (data.len() + offset) as u64,
                    reserved: reservation.as_u64(),
                    flags: kf,
                    age: 0,
                    descriptor_index,
                    clean: false,
                    data: None, // no caching implemented yet for large keys
                    atime: self.created.elapsed().as_millis() as u64,
                };
                self.keys.insert(name.to_string(), kcache);
                self.key_count += 1;
                // 1. allocate all the pages up to the reservation limit
                for vpage_addr in (large_alloc_ptr.as_u64()..large_alloc_ptr.as_u64() + reservation.as_u64())
                    .step_by(VPAGE_SIZE)
                {
                    let pp = hw.try_fast_space_alloc().ok_or(Error::new(
                        ErrorKind::OutOfMemory,
                        "couldn't allocate memory for large key",
                    ))?;
                    assert!(pp.valid(), "didn't receive a valid page in large space alloc");
                    log::debug!("pp alloc v{:x}->p{:x?}", vpage_addr, pp);
                    v2p_map.insert(VirtAddr::new(vpage_addr).unwrap(), pp);
                }
                // 2. Recurse. Now, the key should exist, and it should go through the "write the data out"
                //    section of the algorithm.
                // note that the alloc pointer at this point sets the ultimate limit for the large file size
                // (32GiB as of writing).
                return self.key_update(
                    hw,
                    v2p_map,
                    cipher,
                    name,
                    data,
                    offset,
                    alloc_hint,
                    truncate,
                    large_alloc_ptr + PageAlignedVa::from(LARGE_FILE_MAX_SIZE),
                );
            }
        }
        Ok(large_alloc_ptr)
    }

    #[allow(dead_code)]
    pub fn key_contains(&self, name: &str) -> bool { self.keys.contains_key(&String::from(name)) }

    fn rebuild_free_pool(&mut self) {
        self.small_pool_free.clear();
        for (index, ksp) in self.small_pool.iter().enumerate() {
            self.small_pool_free.push(KeySmallPoolOrd { index, avail: ksp.avail })
        }
    }

    /// Used to remove a key from the dictionary. If you call it with a non-existent key,
    /// the routine has no effect, and does not report an error. Small keys are not immediately
    /// overwritten in paranoid mode, but large keys are.
    pub fn key_remove(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &mut HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        name_str: &str,
        paranoid: bool,
    ) {
        log::debug!("removing key {}", name_str);
        if paranoid {
            // large records are paranoid-erased, by default, because of the pool-reuse problem.
            unimplemented!(
                "Paranoid erase for small records not yet implemented. Calling sync after an update, however, effectively does a paranoid erase."
            );
        }
        // this call will check the disk to see if there's key data that's not in cache.
        if self.ensure_key_entry(hw, v2p_map, cipher, name_str) {
            let name = String::from(name_str);
            let mut need_rebuild = false;
            let mut need_free_key: Option<u32> = None;
            if let Some(kcache) = self.keys.get_mut(&name) {
                if !kcache.flags.valid() {
                    log::debug!("ensure of invalid key: {}", name_str);
                    assert!(kcache.clean == false, "keys that are invalidated should be marked as not clean");
                    // key was previously deleted, already in cache, but not flushed. Nothing to do here.
                    return;
                }
                self.age = self.age.saturating_add(1);
                self.clean = false;

                kcache.age = kcache.age.saturating_add(1);
                kcache.flags.set_valid(false);
                kcache.clean = false;

                if let Some(small_index) = small_storage_index_from_key(kcache, self.index) {
                    // handle the small pool case
                    let ksp = &mut self.small_pool[small_index];
                    let err_static = format!(
                        "Small pool did not contain the element we expected: {}, len: {}",
                        &name, kcache.len
                    );
                    log::debug!("ksp swap_remove({}) flags: {:?}", name, kcache.flags);
                    ksp.contents
                        .swap_remove(ksp.contents.iter().position(|s| *s == name).expect(&err_static));
                    assert!(kcache.reserved <= SMALL_CAPACITY as u64, "error in small key entry size");
                    ksp.avail += kcache.reserved as u16;
                    assert!(ksp.avail <= SMALL_CAPACITY as u16, "bookkeeping error in small pool capacity");
                    ksp.clean = false; // this will also effectively cause the record to be deleted on disk once the small pool data is synchronized
                    need_rebuild = true;
                } else {
                    // handle the large pool case
                    // mark the entry as invalid and dirty; virtual space is one huge memory leak...
                    // ...but we remove the virtual pages from the page pool, effectively reclaiming the
                    // physical space.
                    for vpage in kcache.large_pool_vpages() {
                        if let Some(pp) = v2p_map.get_mut(&vpage) {
                            assert!(pp.valid(), "v2p returned an invalid page");
                            {
                                // this slows things down but it prevents data from leaking back into other
                                // basis data structures when the sector is re-allocated
                                // in other words, i think it's probably very unsafe to not always
                                // secure-erase large data as it's de-allocated.
                                let mut noise = [0u8; PAGE_SIZE];
                                hw.trng_slice(&mut noise);
                                hw.patch_data(&noise, pp.page_number() * PAGE_SIZE as u32);
                            }
                            log::trace!("fast_space_free key_remove {} before", pp.journal());
                            hw.fast_space_free(pp);
                            assert!(pp.valid() == false, "pp is still marked as valid!");
                        }
                    }
                }
                need_free_key = Some(kcache.descriptor_index.get());
            }
            // free up the key index in the dictionary, if necessary
            if let Some(key_to_free) = need_free_key {
                log::debug!("freeing key: {}", key_to_free);
                self.put_free_key_index(key_to_free);
                self.key_count -= 1;
                log::debug!("key_count: {}", self.key_count);
            }
            if need_rebuild {
                // no stable "retain" api, so we have to clear the heap and rebuild it https://github.com/rust-lang/rust/issues/71503
                self.rebuild_free_pool();
            }

            // we don't remove the cache entry, because it hasn't been synchronized to disk.
            // at this point:
            //   - in-memory representation will return an entry, but with its valid flag set to false.
            //   - disk still contains a key entry that claims we have a valid key
            // a call to sync is necessary to completely flush things, but, we don't sync every time we remove
            // because it's inefficient.
        } else {
            log::debug!("key_remove() key does not exist: {}", name_str);
        }
        // if there's no key....we're done!
    }

    /// used to remove a key from the dictionary, syncing 0's to the disk in the key's place
    /// sort of less relevant now that the large keys have a paranoid mode; probably this routine should
    /// actually be a higher-level function that catches the paranoid request and does an "update" of 0's
    /// to the key then does a disk sync and then calls remove
    pub fn key_erase(&mut self, _name: &str) {
        unimplemented!();
    }

    /// estimates the amount of space needed to sync the dict cache. Pass this to ensure_fast_space_alloc()
    /// before calling a sync. estimate can be inaccurate under pathological allocation conditions.
    pub(crate) fn alloc_estimate_small(&self) -> usize {
        let mut data_estimate = 0;
        let mut index_estimate = 0;
        for ksp in &self.small_pool {
            if !ksp.clean {
                for keyname in &ksp.contents {
                    let kce = self.keys.get(keyname).expect("data allocated but no index entry");
                    if kce.flags.unresolved() && kce.flags.valid() {
                        data_estimate += SMALL_CAPACITY - ksp.avail as usize;
                        index_estimate += 1;
                    }
                }
            }
        }
        let index_avail = DK_PER_VPAGE - self.keys.len() % DK_PER_VPAGE;
        let index_req = if index_estimate > index_avail {
            ((index_estimate - index_avail) / DK_PER_VPAGE) + 1
        } else {
            0
        };
        (data_estimate / VPAGE_SIZE) + 1 + index_req
    }

    /// Synchronize a given small pool key to disk. Make sure there is adequate space in the fastspace
    /// pool by using self.alloc_estimate_small + hw.ensure_fast_space_alloc. Following this call,
    /// one should call `dict_sync` and `pt_sync` as soon as possible to keep everything consistent.
    ///
    /// Observation: given the dictionary index and the small key pool index, we know exactly
    /// the virtual address of the data pool.
    pub(crate) fn sync_small_pool(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &mut HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
    ) -> bool {
        let mut data_cache = PlaintextCache { data: None, tag: None };
        for (index, entry) in self.small_pool.iter_mut().enumerate() {
            if !entry.clean {
                let pool_vaddr =
                    VirtAddr::new(small_storage_base_vaddr_from_indices(self.index, index)).unwrap();
                if !hw.fast_space_has_pages(1) {
                    return false;
                }
                let pp = v2p_map
                    .entry(pool_vaddr)
                    .or_insert_with(|| {
                        let mut ap =
                            hw.try_fast_space_alloc().expect("No free space to allocate small key storage");
                        ap.set_valid(true);
                        ap
                    })
                    .clone();
                assert!(pp.valid(), "v2p returned an invalid page");

                // WARNING - we don't read back the journal number before loading data into the page!
                // we /could/ do that, but it incurs an expensive full-page decryption when we plan to nuke
                // all the data. I'm a little worried the implementation as-is is going to be
                // too slow, so let's try the abbreviated method and see how it fares. This
                // incurs a risk that we lose data if we have a power outage or panic just after
                // the page is erased but before all the PTEs and pointers are synced.
                //
                // If it turns out this is an issue, here's how you'd fix it:
                //   1. decrypt the old page (if it exists) and extract the journal rev
                //   2. de-allocate the old phys page, returning it to the fastspace pool; it'll likely not be
                //      returned on the next step
                //   3. allocate a new page
                //   4. write data to the new page (which increments the old journal rev)
                //   5. sync the page tables
                // This implementation just skips to step 3.
                let mut page = [0u8; VPAGE_SIZE + size_of::<JournalType>()];
                for (&src, dst) in (hw.trng_u32() % JOURNAL_RAND_RANGE)
                    .to_le_bytes()
                    .iter()
                    .zip(page[..size_of::<JournalType>()].iter_mut())
                {
                    *dst = src;
                }
                let mut pool_offset = 0;
                // visit the entries in arbitrary order, but back them in optimally tightly packed
                for key_name in &entry.contents {
                    let kcache = self.keys.get_mut(key_name).expect("data record without index");
                    if kcache.flags.valid() {
                        // only sync valid keys, not ones that were marked for deletion but not yet synced.
                        log::debug!(
                            "sync {}/{}:0x{:x}..{}->{}",
                            key_name,
                            index,
                            kcache.start,
                            kcache.len,
                            kcache.reserved
                        );
                        let old_start = kcache.start;
                        kcache.start = pool_vaddr.get() + pool_offset as u64;
                        kcache.age = kcache.age.saturating_add(1);
                        kcache.clean = false;
                        kcache.flags.set_unresolved(false);
                        kcache.flags.set_valid(true);
                        match kcache.data.as_mut() {
                            Some(KeyCacheData::Small(data)) => {
                                data.clean = true;
                                page[size_of::<JournalType>() + pool_offset
                                    ..size_of::<JournalType>() + pool_offset + kcache.len as usize]
                                    .copy_from_slice(&data.data[..kcache.len as usize]);
                            }
                            None => {
                                // the entry was pruned and must be loaded
                                // fill() will only do the expensive decryption operation once per page, and
                                // refer to the cached value on other calls
                                log::debug!("Small key sync filling {}", key_name);
                                data_cache.fill(
                                    hw,
                                    v2p_map,
                                    cipher,
                                    &self.aad,
                                    VirtAddr::new(pool_vaddr.get()).unwrap(),
                                );
                                if let Some(old_page) = data_cache.data.as_ref() {
                                    let start_offset =
                                        size_of::<JournalType>() + (old_start % VPAGE_SIZE as u64) as usize;
                                    // the delta of len-reserved bytes is assumed zeroed by the original space
                                    // allocator; we just copy that
                                    // here. However, if this assumption is broken, we could have leakage
                                    // between keys in the reserved area!!!
                                    page[size_of::<JournalType>() + pool_offset
                                        ..size_of::<JournalType>() + pool_offset + kcache.len as usize]
                                        .copy_from_slice(
                                            &old_page[start_offset..start_offset + kcache.len as usize],
                                        );
                                    // now take the previous data and shove it back in our cache
                                    let data =
                                        old_page[start_offset..start_offset + kcache.len as usize].to_vec();
                                    kcache.data =
                                        Some(KeyCacheData::Small(KeySmallData { clean: true, data }));
                                } else {
                                    log::error!(
                                        "Key {}'s data region at pp: {:x?} va: {:x} is unreadable",
                                        key_name,
                                        data_cache.tag,
                                        kcache.start
                                    );
                                }
                            }
                            _ => {
                                // the type returned was large, which is patently incorrect
                                panic!("Incorrect data cache type for small key entry.");
                            }
                        }
                        pool_offset += kcache.reserved as usize;
                    }
                }
                // now commit the sector to disk
                hw.data_encrypt_and_patch_page(cipher, &self.aad, &mut page, &pp);
                entry.clean = true;
                entry.evicted = false;
                log::debug!("Key pool[{}] is now clean, and not evicted: {:?}", index, entry.contents);
            } else {
                log::debug!("Key pool[{}] was clean, no need to sync: {:?}", index, entry.contents);
            }
        }
        // we now have a bunch of dirty kcache entries. You should call `dict_sync` shortly after this to
        // synchronize those entries to disk.
        true
    }

    /// No data cache to flush yet...large pool caches not implemented!
    pub(crate) fn sync_large_pool(&self) {}

    /// Finds the next available slot to store the key metadata (not the data itself). It also
    /// does bookkeeping to bound brute-force searches for keys within the dictionary's index space.
    pub(crate) fn get_free_key_index(&mut self) -> Option<NonZeroU32> {
        if let Some(free_key) = self.free_keys.pop() {
            let index = free_key.0.start;
            if free_key.0.run > 0 {
                self.free_keys.push(Reverse(FreeKeyRange { start: index + 1, run: free_key.0.run - 1 }))
            }
            if index >= self.last_disk_key_index {
                // if the new index is outside the currently known set, raise the search extent for the
                // brute-force search
                self.last_disk_key_index = index + 1;
            }
            NonZeroU32::new(index as u32)
        } else {
            log::warn!("Ran out of dict index space");
            None
        }
    }

    /// Returns a key's metadata storage to the index pool.
    pub(crate) fn put_free_key_index(&mut self, index: u32) {
        let free_keys = std::mem::replace(&mut self.free_keys, BinaryHeap::<Reverse<FreeKeyRange>>::new());
        let mut free_key_vec = free_keys.into_sorted_vec();
        // I know what you're going to say! It'd be more efficient to have a custom Ord implementation!
        // I tried! It got really confusing! Some calls were coming out sorted and others weren't. Something
        // subtle was wrong in the Ord implementation. While this is less efficient, it at least
        // works, and I can reason through it. You're welcome to fix it. Just be sure to pass the unit
        // tests.
        free_key_vec.reverse();
        // this is a bit weird because we have three cases:
        // - the new key is more than 1 away from any element, in which case we insert it as a singleton
        //   (start = index, run = 0)
        // - the new key is adjacent to exactly once element, in which case we put it either on the top or
        //   bottom (merge into existing record)
        // - the new key is adjacent to exactly two elements, in which case we merge the new key and other two
        //   elements together, add its length to the new overall run
        let mut skip = false;
        let mut placed = false;
        log::trace!("inserting free key {}", index);
        for i in 0..free_key_vec.len() {
            if skip {
                // this happens when we merged into the /next/ record, and we reduced the total number of
                // items by one
                skip = false;
                continue;
            }
            if !placed {
                match free_key_vec[i].0.arg_compared_to_self(index) {
                    FreeKeyCases::LessThan => {
                        self.free_keys.push(Reverse(FreeKeyRange { start: index as u32, run: 0 }));
                        placed = true;
                        self.free_keys.push(free_key_vec[i]);
                    }
                    FreeKeyCases::LeftAdjacent => {
                        self.free_keys.push(Reverse(FreeKeyRange {
                            start: index as u32,
                            run: free_key_vec[i].0.run + 1,
                        }));
                        placed = true;
                    }
                    FreeKeyCases::Within => {
                        log::error!("Double-free error in free_keys()");
                        log::info!("free_key_vec[i].0: {:?}", free_key_vec[i].0);
                        log::info!("index: {}", index);
                        panic!(
                            "Double-free error in free_keys(). free_key_vec[i].0: {:?}, index: {}, free_key_vec.len(): {}",
                            free_key_vec[i].0,
                            index,
                            free_key_vec.len()
                        );
                    }
                    FreeKeyCases::RightAdjacent => {
                        // see if we should merge to the right
                        if i + 1 < free_key_vec.len() {
                            log::trace!(
                                "middle insert {}: {:?}, {:?}",
                                index,
                                free_key_vec[i],
                                free_key_vec[i + 1]
                            );
                            if free_key_vec[i + 1].0.arg_compared_to_self(index as u32)
                                == FreeKeyCases::LeftAdjacent
                            {
                                self.free_keys.push(Reverse(FreeKeyRange {
                                    start: free_key_vec[i].0.start,
                                    run: free_key_vec[i].0.run + free_key_vec[i + 1].0.run + 2,
                                }));
                                skip = true;
                                placed = true;
                                continue;
                            }
                        }
                        self.free_keys.push(Reverse(FreeKeyRange {
                            start: free_key_vec[i].0.start,
                            run: free_key_vec[i].0.run + 1,
                        }));
                        placed = true;
                    }
                    FreeKeyCases::GreaterThan => {
                        self.free_keys.push(free_key_vec[i]);
                    }
                }
            } else {
                self.free_keys.push(free_key_vec[i]);
            }
        }
    }

    /// a cache entry doesn't actually know it's own name -- it's the key associated with the cache entry
    /// so you must provide it to create the full record. It also doesn't know the name of its containing
    /// basis.
    pub(crate) fn to_dict_attributes(&self, name: &str, basis_name: &str) -> DictAttributes {
        DictAttributes {
            flags: self.flags,
            age: self.age,
            num_keys: self.key_count,
            free_key_index: self.last_disk_key_index,
            name: name.to_string(),
            clean: self.clean,
            small_key_count: self.small_pool.len(),
            basis: basis_name.to_string(),
        }
    }

    /// removes a key cache entry from a dictionary cache, returning the amount of space liberated with
    /// the eviction. 0 means the key was either 0-sized, or it could not be evicted (possibly
    /// because it was dirty; you need to call sync before evicting anything from the cache)
    pub(crate) fn evict_keycache_entry(&mut self, key: &str) -> usize {
        if let Some(kcache) = self.keys.get_mut(key) {
            // if the cache entry is dirty, abort
            if kcache.flags.valid() && !kcache.clean {
                return 0;
            }
            if let Some(KeyCacheData::Small(_)) = kcache.data {
                // can only prune small records
                let pruned = kcache.size();
                kcache.data.take(); // this effectively frees up the key cache data
                // mark the key's pool as unclean, so it is processed for filling
                let pool_index = small_storage_index_from_key(&kcache, self.index).expect("index missing");
                self.small_pool[pool_index].evicted = true;
                log::debug!("pruned {} bytes from key {} / evicted ksp index: {}", pruned, key, pool_index);
                pruned
            } else {
                0
            }
        } else {
            0
        }
    }
}

/// converts a dictionary index -- which is a 1-offset number of dictionaries -- plus a key
/// metadata index (not the key number; but, the enumerated set of potential key slots, also
/// 1-offset), and creates a virtual address for the location of this combination
/// It's written a little weird because DK_PER_VPAGE is 32, which optimizes cleanly and removes
/// an expensive division step.
fn dict_indices_to_vaddr(dict_index: NonZeroU32, key_meta_index: usize) -> u64 {
    assert!(key_meta_index != 0, "key metadata index is 1-offset");
    dict_index.get() as u64 * DICT_VSIZE + ((key_meta_index / DK_PER_VPAGE) as u64) * VPAGE_SIZE as u64
}
/// Derives the index of a Small Pool storage block given the key cache entry and the dictionary index.
/// The index maps into the small_pool array, which itself maps 1:1 onto blocks inside the small pool
/// memory space.
pub(crate) fn small_storage_index_from_key(kcache: &KeyCacheEntry, dict_index: NonZeroU32) -> Option<usize> {
    let storage_base = (dict_index.get() - 1) as u64 * SMALL_POOL_STRIDE + SMALL_POOL_START;
    let storage_end = storage_base + SMALL_POOL_STRIDE;
    if (kcache.start >= storage_base) && ((kcache.start + kcache.reserved) < storage_end) {
        let index_base = kcache.start - storage_base;
        Some(index_base as usize / SMALL_CAPACITY)
    } else {
        None
    }
}
/// derive the virtual address of a small storage block from the dictionary and the index of the small storage
/// pool
pub(crate) fn small_storage_base_vaddr_from_indices(dict_index: NonZeroU32, base_index: usize) -> u64 {
    SMALL_POOL_START + (dict_index.get() - 1) as u64 * DICT_VSIZE + base_index as u64 * SMALL_CAPACITY as u64
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
#[repr(C, align(8))]
pub struct DictName {
    pub len: u8,
    pub data: [u8; DICT_NAME_LEN - 1],
}
impl DictName {
    pub fn try_from_str(name: &str) -> Result<DictName> {
        let mut alloc = [0u8; DICT_NAME_LEN - 1];
        let bytes = name.as_bytes();
        if bytes.len() > (DICT_NAME_LEN - 1) {
            Err(Error::new(ErrorKind::InvalidInput, "dict name is too long"))
        } else {
            for (&src, dst) in bytes.iter().zip(alloc.iter_mut()) {
                *dst = src;
            }
            Ok(DictName {
                len: bytes.len() as u8, // this as checked above to be short enough
                data: alloc,
            })
        }
    }
}
impl Default for DictName {
    fn default() -> DictName { DictName { len: 0, data: [0; DICT_NAME_LEN - 1] } }
}

#[derive(Debug)]
/// On-disk representation of the dictionary header. This structure is mainly for archival/unarchival
/// purposes. To "functionalize" a stored disk entry, it needs to be deserialized into a DictionaryCacheEntry.
#[repr(C, align(8))]
pub(crate) struct Dictionary {
    /// Reserved for flags on the record entry
    pub(crate) flags: DictFlags,
    /// Access count to the dicitionary
    pub(crate) age: u32,
    /// Number of keys in the dictionary
    pub(crate) num_keys: u32,
    /// Free index starting space. While this is a derived parameter, its value is recorded to avoid
    /// an expensive, long search operation during the creation of a dictionary cache record. 0 is an invalid
    /// index, as this is where the header goes. Maybe this should be a NonZeroU32.
    pub(crate) free_key_index: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    pub(crate) name: DictName,
}
impl Default for Dictionary {
    fn default() -> Dictionary {
        let mut flags = DictFlags(0);
        flags.set_valid(true);
        Dictionary { flags, age: 0, num_keys: 0, free_key_index: 1, name: DictName::default() }
    }
}
impl Deref for Dictionary {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const Dictionary as *const u8,
                core::mem::size_of::<Dictionary>(),
            ) as &[u8]
        }
    }
}
impl DerefMut for Dictionary {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self as *mut Dictionary as *mut u8,
                core::mem::size_of::<Dictionary>(),
            ) as &mut [u8]
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct DictAttributes {
    pub(crate) flags: DictFlags,
    pub(crate) age: u32,
    pub(crate) num_keys: u32,
    pub(crate) free_key_index: u32,
    pub(crate) name: String,
    pub(crate) clean: bool,
    pub(crate) small_key_count: usize,
    pub(crate) basis: String,
}

/// This structure "enforces" the 127-byte stride of dict/key vpage entries
#[derive(Copy, Clone)]
pub(crate) struct DictKeyEntry {
    pub(crate) data: [u8; DK_STRIDE],
}
impl Default for DictKeyEntry {
    fn default() -> DictKeyEntry { DictKeyEntry { data: [0; DK_STRIDE] } }
}

/// This structure helps to bookkeep which slices within a DictKey virtual page need to be updated
pub(crate) struct DictKeyVpage {
    pub(crate) elements: [Option<DictKeyEntry>; VPAGE_SIZE / DK_STRIDE],
}
impl<'a> Default for DictKeyVpage {
    fn default() -> DictKeyVpage { DictKeyVpage { elements: [None; VPAGE_SIZE / DK_STRIDE] } }
}

#[derive(PartialEq, Eq, Debug)]
pub(crate) enum FreeKeyCases {
    LeftAdjacent,
    RightAdjacent,
    Within,
    LessThan,
    GreaterThan,
}
#[derive(Eq, Copy, Clone, Debug)]
pub(crate) struct FreeKeyRange {
    /// This index should be free. Smallest value is 1; 0-index is for the dict header. Maybe this should be
    /// a NonZeroU32?
    pub(crate) start: u32,
    /// Additional free keys after the start one. Run = 0 means just the start key is free, and the
    /// next one should be used. Run = 2 means {start, start+1} are free, etc.
    pub(crate) run: u32,
}
impl FreeKeyRange {
    /// returns the result of "index compared to self", so,
    /// if the index is smaller than me, the result is LessThan.
    pub(crate) fn arg_compared_to_self(&self, index: u32) -> FreeKeyCases {
        if self.start > 1 && (index < (self.start - 1)) {
            FreeKeyCases::LessThan
        } else if self.start > 0 && (index == self.start - 1) {
            FreeKeyCases::LeftAdjacent
        } else if (index >= self.start) && (index <= (self.start + self.run)) {
            FreeKeyCases::Within
        } else if index == (self.start + self.run + 1) {
            FreeKeyCases::RightAdjacent
        } else {
            FreeKeyCases::GreaterThan
        }
    }
}
impl Ord for FreeKeyRange {
    fn cmp(&self, other: &Self) -> Ordering { self.start.cmp(&other.start) }
}
impl PartialOrd for FreeKeyRange {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(&other)) }
}
impl PartialEq for FreeKeyRange {
    fn eq(&self, other: &Self) -> bool { self.start == other.start }
}

/// stashed copy of a decrypted page. The copy here must always match
/// what's actually on disk; do not mutate it and expect it to sync with the disk.
/// Remember to invalidate this if the data are
/// This is stored with the journal number on top.
/// What the four possibilities of cache vs pp mean:
/// Some(cache) & Some(cache_pp) -> valid cache and pp
/// None(cache) & Some(cache_pp) -> the page was allocated; but never used, or was erased (it's free for you
/// to use it); alternately, it was corrupted Some(cache) & None(cache_pp) -> invalid, internal error
/// None(cache) & None(cache_pp) -> the basis mapping didn't exist: we've never requested this page before.
pub(crate) struct PlaintextCache {
    /// a page of data, stored with the Journal rev on top
    pub(crate) data: Option<Vec<u8>>,
    /// the page the cache corresponds to
    pub(crate) tag: Option<PhysPage>,
}
impl PlaintextCache {
    pub fn fill(
        &mut self,
        hw: &mut PddbOs,
        v2p_map: &HashMap<VirtAddr, PhysPage>,
        cipher: &Aes256GcmSiv,
        aad: &[u8],
        req_vaddr: VirtAddr,
    ) {
        if let Some(pp) = v2p_map.get(&req_vaddr) {
            assert!(pp.valid(), "v2p returned an invalid page");
            let mut fill_needed = false;
            if let Some(tag) = self.tag {
                if tag.page_number() != pp.page_number() {
                    fill_needed = true;
                }
            } else if self.tag.is_none() {
                fill_needed = true;
            }
            if fill_needed {
                self.data = hw.data_decrypt_page(&cipher, &aad, pp);
                self.tag = Some(*pp);
            }
        } else {
            self.data = None;
            self.tag = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clone_bheap<T: Clone + Ord + Copy>(heap: &mut BinaryHeap<Reverse<T>>) -> BinaryHeap<Reverse<T>> {
        let heap_copy = std::mem::replace(heap, BinaryHeap::<Reverse<T>>::new());
        let mut heap_clone = BinaryHeap::<Reverse<T>>::new();
        let heap_vec = heap_copy.into_sorted_vec();
        for &i in heap_vec.iter() {
            heap_clone.push(i.clone());
            heap.push(i);
        }
        heap_clone
    }
    #[test]
    fn test_free_key_index() {
        let mut d = DictCacheEntry::new(Dictionary::default(), 1, &Vec::<u8>::new());
        let init_max = KEY_MAXCOUNT as u32 - 1 - 1;
        // 1..131070
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        assert!(f.len() == 1);
        assert!(f[0].0.start == 1);
        assert!(f[0].0.run == init_max);
        for i in 0..10 {
            assert!(d.get_free_key_index().unwrap().get() == i + 1);
        }
        // 11...131060
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        assert!(f.len() == 1);
        assert!(f[0].0.start == 11);
        assert!(f[0].0.run == init_max - 10);

        d.put_free_key_index(4);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 4: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 4);
        assert!(f[0].0.run == 0);
        assert!(f[1].0.start == 11);
        assert!(f[1].0.run == init_max - 10);

        d.put_free_key_index(9);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 9: {:?}", f);
        assert!(f.len() == 3);
        assert!(f[0].0.start == 4);
        assert!(f[0].0.run == 0);
        assert!(f[1].0.start == 9);
        assert!(f[1].0.run == 0);
        assert!(f[2].0.start == 11);
        assert!(f[2].0.run == init_max - 10);

        d.put_free_key_index(10);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 10: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 4);
        assert!(f[0].0.run == 0);
        assert!(f[1].0.start == 9);
        assert!(f[1].0.run == init_max - 8);

        d.put_free_key_index(3);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 3: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 3);
        assert!(f[0].0.run == 1);
        assert!(f[1].0.start == 9);
        assert!(f[1].0.run == init_max - 8);

        d.put_free_key_index(5);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 5: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 3);
        assert!(f[0].0.run == 2);
        assert!(f[1].0.start == 9);
        assert!(f[1].0.run == init_max - 8);

        d.put_free_key_index(8);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("insert 8: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 3);
        assert!(f[0].0.run == 2);
        assert!(f[1].0.start == 8);
        assert!(f[1].0.run == init_max - 7);

        let k = d.get_free_key_index().unwrap().get();
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("get [3]: {:?}", f);
        assert!(k == 3);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 4);
        assert!(f[0].0.run == 1);
        assert!(f[1].0.start == 8);
        assert!(f[1].0.run == init_max - 7);

        assert!(d.get_free_key_index().unwrap().get() == 4);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("get [4]: {:?}", f);
        assert!(f.len() == 2);
        assert!(f[0].0.start == 5);
        assert!(f[0].0.run == 0);
        assert!(f[1].0.start == 8);
        assert!(f[1].0.run == init_max - 7);

        assert!(d.get_free_key_index().unwrap().get() == 5);
        let c = clone_bheap(&mut d.free_keys);
        let mut f = c.into_sorted_vec();
        f.reverse();
        println!("get [5]: {:?}", f);
        assert!(f.len() == 1);
        assert!(f[0].0.start == 8);
        assert!(f[0].0.run == init_max - 7);
    }
}
