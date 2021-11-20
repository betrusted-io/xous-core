use crate::api::*;
use super::*;

use core::cell::RefCell;
use std::rc::Rc;
use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use std::convert::TryInto;
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Key};
use aes_gcm_siv::aead::{Aead, Payload, NewAead};
use std::iter::IntoIterator;
use std::collections::{BinaryHeap, HashMap};
use std::io::{Result, Error, ErrorKind};


/// In basis space, the BasisRoot is located at VPAGE #1 (VPAGE #0 is always invalid).
/// A VPAGE is 0xFE0 (4,064) bytes long, which is equal to a PAGE of 4k minus 32 bytes of encryption+journal
/// overhead.
///
/// NB: The factors of 4,064 are: 1, 2, 4, 8, 16, 32, 127, 254, 508, 1016, 2032, 4064
///
/// AAD associated with the BasisRoot consist of a bytewise concatenation of:
///   - Basis name
///   - version number (should match version inside; complicates downgrade attacks)
///   - FPGA's silicon DNA number (makes a naive raw-copy of the data to another device unusable;
///     but of course, the DNA ID can be forged minor efforts)
///
/// Usage Assumptions:
///   - Most mutability happens on the data keys themselves (keys are read/write/modify routinely).
///   - Dictionary modifications (key addition or removal) are about 20x less frequent than key mods.
///   - Basis modifications (creation/removal of dictionaries) is about 10x less frequent than dictionary mods.
///   - According to https://www.pdl.cmu.edu/PDL-FTP/HECStorage/Yifan_Final.pdf, 0.01% of files (1 in 10,000)
///     require a name over 100 bytes long; 0.1% require longer than 64 bytes. There longest filename identified
///     was 143 bytes long. Study surveys ~14M files on the LANL network.
///   - Same study says 99.9% of directories have under 1k files, 99.999% under 10k
///
/// The root basis structure takes up the first valid VPAGE in the virtual memory space.
/// It contains a count of the number of valid dictionaries in the Basis. Dictionaries are found at
/// fixed offsets starting at 0xFE_0000 and repeating every 0xFE_0000 intervals, with up to 16383 dictionaries
/// allowed. A naive linear search is used to scan for dictionaries, starting at the lowest address,
/// scanning every 0xFE_0000, until the number of valid dictionaries have been found that matches the valid
/// dictionary count prescribed in the Basis root. A dictionary can be effectively deleted by just marking its
/// descriptor as invalid.
///
/// A stride of 0xFE_0000 means that dictionary descriptors can be up to 4096 VPAGEs long. A dictionary
/// descriptor consists of a `DictDescriptor` header, some bookkeeping data, plus a count of the number
/// of keys in the dictionary. Following the header is a list of key descriptors. Similar to the DictDescriptors,
/// the key descriptors are stored at a stride of 127 (or 32 per VPAGE); they can be deleted by being marked
/// as invalid, and a linear scan is used to identify all the entries. A KeyDescriptor contains the name
/// of the key, flags, its age, and pointers to the key data in virtual memory space + its length.
/// This leads to a name length restriction of roughly 115 characters for keys and dictionaries, which is
/// about half of what most filesystems allow, but accommodates roughly 99.99% of the use cases.
///
/// Thus adding a new dictionary always consumes at least one 4k page, but you can have up to 15 keys
/// in that dictionary with no extra bookkeeping cost once the dictionary is added.
///
///
/// Basis Virtual Memory Layout
///
/// |   Start Address        |                                           |
/// |------------------------|-------------------------------------------|
/// | 0x0000_0000_0000_0000  |  Invalid -- VPAGE 0 reserved for Option<> |
/// | 0x0000_0000_0000_0FE0  |  Basis root page                          |
/// | 0x0000_0000_00FE_0000  |  Dictionary[0]                            |
/// |                    +0  |    - Dict header (127 bytes)              |
/// |                   +7F  |    - Maybe key entry (127 bytes)          |
/// |                   +FE  |    - Maybe key entry (127 bytes)          |
/// |              +FD_FF02  |    - Last key entry start (128k possible) |
/// | 0x0000_0000_01FC_0000  |  Dictionary[1]                            |
/// | 0x0000_003F_7F02_0000  |  Dictionary[16382]                        |
/// | 0x0000_003F_8000_0000  |  Small data pool start  (~256GiB)         |
/// |                        |    - Dict[0] pool = 16MiB (4k vpages)     |
/// |                        |      - SmallPool[0]                       |
/// |                  +FE0  |      - SmallPool[1]
/// | 0x0000_003F_80FE_0000  |    - Dict[1] pool = 16MiB                 |
/// | 0x0000_007E_FE04_0000  |    - Dict[16383] pool                     |
/// | 0x0000_007E_FF02_0000  |  Unused                                   |
/// | 0x0000_007F_0000_0000  |  Medium data pool start                   |
/// |                        |    - TBD                                  |
/// | 0x0000_FE00_0000_0000  |  Large data pool start  (~16mm TiB)       |
/// |                        |    - Demand-allocated, bump-pointer       |
/// |                        |      currently no defrag                  |
///
/// Note that each Basis has its own memory section, and you can have "many" orthogonal Basis without
/// a collision -- the AES keyspace is 128 bits, so you have a decent chance of no collisions
/// even with a few billion Basis concurrently existing in the filesystem.
///
/// Memory Pools
///
/// Key data is split into three categories of sizes: small, medium, and large; but the implementation
/// currently only deals with small and large keys. The thresholds are subject to tuning, but
/// roughly speaking, small data are keys <4k bytes; large keys are everything else.
///
/// Large keys are the simplest - each key starts at a VPAGE-aligned address, and allocates
/// up from there. Any unused amount is wasted, but with a ~32k threshold you'll have no worse
/// than 12.5% unused space, probably closer to ~7%-ish if all your data hovered around the threshold.
/// The allocation is a simple pointer that just keeps going up. De-allocated space is never defragmented,
/// and we just rely on the space being "huge" to save us.
///
/// Small keys are kept in VPAGE-sized pools of data, and compacted together in RAM. The initial, naive
/// implementation simply keeps all small keys in a HashMap in RAM, and when it comes time to sync them
/// to disk, they are sorted by update count, and written to disk in ascending order.
///
/// Medium keys have a TBD implementation, and are currently directed to the large pool for now.
///
/// The Alignment and Serialization Chronicles
///
/// We're using Repr(C) and alignment to 64-bits to create a consistent "FFI" layout; we use an unsafe cast
/// to [u8] as our method to serialize the structure, which means we could be subject to breakage if the Rust
/// compiler decides to change its Repr(C) FFI (it's not guaranteed, but I think at this point in the lifecycle
/// with simple primitive types it's hard to see it changing). This puts some requirements on the ordering of
/// fields in the struct below. Note that the serialization is all double-checked by the pddbdbg.py script.
///
/// In coming to the choice to use Repr(C), I experimented with rkyv and bincode. bincode relies on the serde
/// crate, which, as of Nov 2021, has troubles taking in const generics, and thus barfs on our fixed-sized
/// string allocations that are longer than 32 bytes. Version 2.0 of bincode /might/ do this better, but as
/// of the design of this crate, it's in "alpha" with no official release to crates.io, so we're avoiding it;
/// but for sure 1.3.3 of bincode (latest stable as of the design) cannot do the job, and there's a few other
/// users reporting the issue so I'm pretty sure it's not "user error" on my part.
///
/// rkyv handles const generics well, and it perhaps very reasonably shuffles around the order of structures
/// in the struct to improve the packing efficiency. However, this has the property that rkyv ser will never break
/// rkyv deser, but unfortunately you can't interoperate with anything that isn't rkyv (e.g., describing the data
/// layout to someone who wants to do a C implementation). There's also a risk that if we are forced to
/// upgrade rkyv later on we might break compatibility with what's stored on disk, although I'm pretty sure the
/// maintainer of rkyv tries to avoid that as much as possible.
///
/// Repr(C), while also not guaranteed to be stable, has pressure from the CFFI users at least to keep
/// things as stable as possible, and it is by definition inter-operable with C. Repr(C) is native to Rust,
/// with no additional dependencies to pull in, which helps reduce the code base size overall.
/// So, we're using a repr(C) with an align(8), and then carefully checking our structure organization and
/// elements to keep things "in spec" with what C can natively understand, in an effort to create a disk
/// storage structure that can persist through future versions of Rust and also other implementations in other
/// languages.
///
/// Known Repr(C) footguns:
///  - When you start laying in 64-bit types, stuff has to be 64-bit aligned, or else you'll start to get
///    uninitialized padding data inserted, which can leak stack data in the serialization process.
///  - Don't use anything that's not native to C. In particular, for primitives that we want to be "Option"
///    wrapped, we're using a NonZeroU64 format. The compiler knows how to turn that into a 64-bit C-friendly
///    structure and serialize/deserialize that into the correct Rust structure. See
///    https://doc.rust-lang.org/nomicon/other-reprs.html for a citation on that.

pub(crate) const SMALL_POOL_START: u64 = 0x0000_003F_8000_0000;
pub(crate) const SMALL_POOL_END: u64 = 0x0000_007E_FF02_0000;
pub(crate) const SMALL_POOL_STRIDE: u64 = 0xFE_0000;
/// we don't want this bigger than VPAGE_SIZE, because a key goal of the small pool is to
/// reduce # of writes to the disk of small data. While we could get some gain in memory efficiency
/// if we made this larger than a VPAGE_SIZE, we don't get much gain in terms of write reduction,
/// and it greatly complicates the implementation. So, SMALL_CAPACITY should be less than VPAGE_SIZE.
pub(crate) const SMALL_CAPACITY: usize = VPAGE_SIZE;
pub(crate) const LARGE_POOL_START: u64 = 0x0000_FE00_0000_0000;
pub(crate) const KEY_MAXCOUNT: usize = 131_071; // 2^17 - 1

/// The chosen "stride" of a dict/key entry. Drives a lot of key parameters in the database's characteristics.
/// This is chosen such that 32 of these entries fit evenly into a VPAGE.
pub(crate) const DK_STRIDE: usize = 127;
//// DK_STRIDES per VPAGE
pub(crate) const DK_PER_VPAGE: usize = VPAGE_SIZE / DK_STRIDE; // should be 32 - use this for computing modulus on dictionary indices
/// size of a dictionary region in virtual memory
pub(crate) const DICT_VSIZE: u64 = 0xFE_0000;
/// maximum number of dictionaries in a system
pub(crate) const DICT_MAXCOUNT: usize = 16383;

/// This is the format of the Basis as stored on disk
#[derive(PartialEq, Debug, Default)]
#[repr(C, align(8))]
pub(crate) struct BasisRoot {
    pub(crate) magic: [u8; 4],
    pub(crate) version: u32,
    /// increments every time the BasisRoot is modified. This field must saturate, not roll over.
    pub(crate) age: u32,
    /// number of dictionaries.
    pub(crate) num_dictionaries: u32,
    /* at this point, we are aligned to a 64-bit boundary. All data must stay aligned to this boundary from here out! */
    /// 64-byte name; aligns to 64-bits
    pub(crate) name: BasisRootName,
}
impl BasisRoot {
    pub(crate) fn aad(&self, dna: u64) -> Vec::<u8> {
        let mut aad = Vec::<u8>::new();
        aad.extend_from_slice(&self.name.0);
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        aad.extend_from_slice(&dna.to_le_bytes());
        aad
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

/// A list of open Basis that we can use to search and operate upon. Sort of the "root" data structure of the PDDB.
///
/// Note to self: it's tempting to integrate the "hw" parameter (the pointer to the PddbOs structure). However, this
/// results in interior mutability problems. I guess we could wrap it in a Rc or RefCell or something like that; but
/// the inconvenience of passing the hw structure around doesn't seem too bad so far...
pub(crate) struct BasisCache {
    /// the cache entries themselves
    cache: Vec::<BasisCacheEntry>,
}
impl BasisCache {
    pub(crate) fn new() -> Self {
        BasisCache { cache: Vec::new(), }
    }
    pub(crate) fn add_basis(&mut self, basis: BasisCacheEntry) {
        self.cache.push(basis);
    }
    // placeholder reminder: deleting a basis is a bit more complicated, as it requires
    // syncing its contents.

    fn select_basis(&mut self, basis_name: Option<&str>) -> Option<usize> {
        if self.cache.len() == 0 {
            log::error!("Can't select basis: PDDB is not mounted");
            return None
        }
        if let Some(n) = basis_name {
            self.cache.iter().position(|bc| bc.name == n)
        } else {
            Some(self.cache.len() - 1)
        }
    }

    /// Adds a dictionary with `name` to:
    ///    - if `basis_name` is None, the most recently opened basis
    ///    - if `basis_name` is Some, searches for the given basis and adds the dictionary to that.
    /// If the dictionary already exists, it returns an informative error.
    pub(crate) fn dict_add(&mut self, hw: &mut PddbOs, name: &str, basis_name: Option<&str>) -> Result<()> {
        if !hw.ensure_fast_space_alloc(1, &self.cache) {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to allocate dict"));
        }
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            basis.age = basis.age.saturating_add(1);
            if basis.dicts.get(&String::from(name)).is_some() {
                // quick exit if we see the dictionary is hot in the cache
                return Err(Error::new(ErrorKind::AlreadyExists, "Dictionary already exists"));
            } else {
                // if the dictionary doesn't exist in our cache it doesn't necessarily mean it
                // doesn't exist. Do a comprehensive search if our cache isn't complete.
                if basis.num_dicts as usize != basis.dicts.len() {
                    if basis.dict_deep_search(hw, name).is_some() {
                        return Err(Error::new(ErrorKind::AlreadyExists, "Dictionary already exists"));
                    }
                }
            }
            basis.clean = false;
            // allocate a vpage offset for the dictionary
            let dict_index = basis.dict_get_free_offset(hw);
            let dict_offset = VirtAddr::new(dict_index as u64 * DICT_VSIZE).unwrap();
            let pp = basis.v2p_map.entry(dict_offset).or_insert_with(||
                hw.try_fast_space_alloc().expect("No free space to allocate dict"));
            pp.set_valid(true);

            // create the cache entry
            let mut dict_name = [0u8; DICT_NAME_LEN];
            for (src, dst) in name.bytes().into_iter().zip(dict_name.iter_mut()) {
                *dst = src;
            }
            let mut my_aad = Vec::<u8>::new();
            for &b in basis.aad.iter() {
                my_aad.push(b);
            }
            let mut init_flags = DictFlags(0);
            init_flags.set_valid(true);
            let dict_cache = DictCacheEntry {
                index: dict_index,
                keys: HashMap::<String, KeyCacheEntry>::new(),
                clean: false,
                age: 0,
                flags: init_flags,
                key_count: 0,
                free_key_offset: Some(0),
                small_pool: Vec::<KeySmallPool>::new(),
                small_pool_free: BinaryHeap::<KeySmallPoolOrd>::new(),
                aad: my_aad,
            };
            log::info!("adding dictionary {}", name);
            basis.dicts.insert(String::from(name), dict_cache);
            basis.num_dicts += 1;
            // encrypt and write the dict entry to disk
            basis.dict_sync(hw, name)?;
            // sync the root basis structure as well, while we're at it...
            basis.basis_sync(hw);
            // finally, sync the page tables.
            basis.pt_sync(hw);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
        }
    }

    /// Updates a key in a dictionary; if it doesn't exist, creates it. User can specify a basis,
    /// or rely upon the auto-basis select algorithm.
    pub(crate) fn key_update(&mut self,
        hw: &mut PddbOs, dict: &str, key: &str, data: &[u8], offset: Option<usize>,
        alloc_hint: Option<usize>, basis_name: Option<&str>, truncate: bool) -> Result<()> {

        // we have to estimate how many pages are needed *before* we do anything, because we can't
        // mutate the page table to allocate data while we're accessing the page table. This huge gob of code
        // computes the pages needed. :-/
        let mut pages_needed = 0;
        let reserved = if data.len() + offset.unwrap_or(0) < alloc_hint.unwrap_or(0) {
            data.len() + offset.unwrap_or(0)
        } else {
            alloc_hint.unwrap_or(0)
        };
        let reserved_pages = if (reserved % VPAGE_SIZE == 0) {
            reserved / VPAGE_SIZE
        } else {
            (reserved / VPAGE_SIZE) + 1
        };
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            if !basis.dicts.contains_key(dict) {
                if let Some((index, dict_record)) = basis.dict_deep_search(hw, dict) {
                    if dict_record.flags.valid() {
                        let dict_name = String::from(cstr_to_string(&dict_record.name));
                        let dcache = DictCacheEntry::new(dict_record, index as usize, &basis.aad);
                        basis.dicts.insert(dict.to_string(), dcache);
                    }
                } else {
                    pages_needed += 1;
                    pages_needed += reserved_pages;
                }
            }
            if let Some(dict_entry) = basis.dicts.get(dict) {
                // see if we need to make a kcache entry
                if let Some(kcache) = dict_entry.keys.get(key) {
                    if kcache.descriptor_index.is_none() {
                        // index hasn't been allocated yet, if we don't have extra space in an already allocated page, we'll need a new one
                        if (DK_PER_VPAGE - dict_entry.key_count as usize % DK_PER_VPAGE) == 0 {
                            pages_needed += 1;
                        };
                    }
                    // now check for data reservations
                    if reserved < SMALL_CAPACITY {
                        // it's probably going in the small pool.
                        // index exists, see if the page exists
                        let key_index = (((kcache.start - SMALL_POOL_START as u64) - (dict_entry.index as u64 * SMALL_POOL_STRIDE as u64)) / SMALL_CAPACITY as u64) as usize;
                        if dict_entry.small_pool.len() > key_index {
                            log::info!("resoved key index {}, small pool len: {}", key_index, dict_entry.small_pool.len());
                            // see if the pool's address exists in the page table
                            let pool_vaddr = VirtAddr::new(dict_entry.index as u64 * SMALL_POOL_STRIDE + SMALL_POOL_START).unwrap();
                            if !basis.v2p_map.contains_key(&pool_vaddr) {
                                pages_needed += 1;
                            }
                        } else {
                            // we're definitely going to need another small pool page
                            pages_needed += 1;
                        }
                    } else {
                        // it's a large block. see if its address have been mapped
                        // large pool start addresses should always be vpage aligned
                        for vpage in (kcache.start..kcache.start + kcache.reserved).step_by(VPAGE_SIZE) {
                            if !basis.v2p_map.contains_key(&VirtAddr::new(vpage).unwrap()) {
                                pages_needed += 1;
                            }
                        }
                    }
                } else {
                    // there's no key cache entry. For simplicity, let's just assume this means none of the data
                    // has been allocated, and ask for it to be available.
                    //
                    // At least in v1 of the code, this is the case. Later on maybe you could
                    // evict a key key cache entry to trim memory usage; if we do this, we'll have to implement a
                    // routine that is like ensure_key_entry() but doesn't do any allocations - it just does the search
                    // for the key record and if it doesn't exist reports that we'll need one, but if the key does exist
                    // and simply wasn't loaded into cache, then don't ask for the reservation.
                    //
                    // But this should be a "last resort" move: preferably, if we have memory pressure, we should take
                    // the data entries in the key cache and turn them into None to reduce data pressure, rather than
                    // evicting the key indices themselves (because it causes bookkeeping problems like this). I guess
                    // if someone wanted to allocate thousand and thousands of keys, we'd eventually consume a megabyte of
                    // RAM, which is relatively precious, but....anyways. Maybe the answer is "don't do that" on a small memory
                    // machine and expect it to work?
                    pages_needed += reserved_pages;
                }
            }
        } else {
            return Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."));
        }
        // make the reservation
        if !hw.ensure_fast_space_alloc(pages_needed, &self.cache) {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to allocate dict"));
        }
        // now actually do the update
        if let Some(basis_index) = self.select_basis(basis_name) {
            let mut dict_found = false;
            { // resolve the basis here, potentially mutating things
                let basis = &mut self.cache[basis_index];
                basis.age = basis.age.saturating_add(1);
                basis.clean = false;
                if !basis.dicts.contains_key(dict) {
                    // if the dictionary doesn't exist in our cache it doesn't necessarily mean it
                    // doesn't exist. Do a comprehensive search if our cache isn't complete.
                    if let Some((index, dict_record)) = basis.dict_deep_search(hw, dict) {
                        let dict_name = String::from(cstr_to_string(&dict_record.name));
                        let dcache = DictCacheEntry::new(dict_record, index as usize, &basis.aad);
                        basis.dicts.insert(dict.to_string(), dcache);
                        dict_found = true;
                    }
                }
            }
            if !dict_found { // now that we're clear of the deep search, mutate the basis if we are sure it's not there
                self.dict_add(hw, dict, basis_name).expect("couldn't add dictionary");
            }
            // refetch the basis here to avoid the re-borrow problem, now that all the potential dict cache mutations are done
            let basis = &mut self.cache[basis_index];
            // at this point, the dictionary should definitely be in cache
            if let Some(dict_entry) = basis.dicts.get_mut(dict) {
                let updated_ptr = dict_entry.key_update(hw, &mut basis.v2p_map,
                    &basis.cipher, key, data,
                    offset.unwrap_or(0),
                    alloc_hint,
                    truncate,
                    basis.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START))
                ).expect("couldn't add key");
                basis.large_alloc_ptr = Some(updated_ptr);
                dict_entry.sync_small_pool(hw, &mut basis.v2p_map, &basis.cipher);
                // we don't have large pool caches yet, but this is a placeholder to remember to do "something" at this point,
                // once we do have them.
                dict_entry.sync_large_pool();

                // encrypt and write the dict entry to disk
                basis.dict_sync(hw, dict)?;
                // sync the root basis structure as well, while we're at it...
                basis.basis_sync(hw);
                // finally, sync the page tables.
                basis.pt_sync(hw);
            } else {
                return Err(Error::new(ErrorKind::NotFound, "Requested dictionary not found, or could not be allocated."));
            }

            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
        }
    }
}

/// This is the RAM cached copy of a basis as maintained in the PDDB.
pub(crate) struct BasisCacheEntry {
    /// the name of this basis
    pub name: String,
    /// set if synched to what's on disk
    pub clean: bool,
    /// last sync time, in systicks, if any
    pub last_sync: Option<u64>,
    /// Number of dictionaries. This should be greater than or equal to the number of elements in dicts.
    /// If dicts has less elements than num_dicts, it means there are still dicts on-disk that haven't been cached.
    pub num_dicts: u32,
    /// dictionary array.
    pub dicts: HashMap::<String, DictCacheEntry>,
    /// A cached copy of the absolute offset of the next free dictionary slot,
    /// expressed as a number that needs to be multiplied by DICT_VSIZE to arrive at a virtual address
    pub free_dict_offset: Option<u32>,
    /// the cipher for the basis
    pub cipher: Aes256GcmSiv,
    /// the AAD associated with this Basis
    pub aad: Vec::<u8>,
    /// modification count
    pub age: u32,
    /// virtual to physical page map. It's the reverse mapping of the physical page table on disk.
    pub v2p_map: HashMap<VirtAddr, PhysPage>,
    /// the last journal rev written to disk
    pub journal: u32,
    /// current allocation pointer for the "large" pool. This just keeps incrementing "forever".
    pub large_alloc_ptr: Option<PageAlignedVa>,
}
impl BasisCacheEntry {
    /// given a pointer to the hardware, name of the basis, and its cryptographic key, try to derive
    /// the basis. If `lazy` is true, it stops with the minimal amount of effort to respond to a query.
    /// If it `lazy` is false, it will populate the dictionary cache and key cache entries, as well as
    /// discover the location of the `large_alloc_ptr`.
    pub(crate) fn mount(hw: &mut PddbOs, name: &str, key: &[u8; AES_KEYSIZE], lazy: bool) -> Option<BasisCacheEntry> {
        if let Some(basis_map) = hw.pt_scan_key(key, name) {
            let cipher = Aes256GcmSiv::new(Key::from_slice(key));
            let aad = hw.data_aad(name);
            // get the first page, where the basis root is guaranteed to be
            if let Some(root_page) = basis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                let vpage = match hw.data_decrypt_page(&cipher, &aad, root_page) {
                    Some(data) => data,
                    None => {log::error!("System basis decryption did not authenticate. Unrecoverable error."); return None;},
                };
                // if the below assertion fails, you will need to re-code this to decrypt more than one VPAGE and stripe into a basis root struct
                assert!(size_of::<BasisRoot>() <= VPAGE_SIZE, "BasisRoot has grown past a single VPAGE, this routine needs to be re-coded to accommodate the extra bulk");
                let mut basis_root = BasisRoot::default();
                for (&src, dst) in vpage[size_of::<JournalType>()..].iter().zip(basis_root.deref_mut().iter_mut()) {
                    *dst = src;
                }
                if basis_root.magic != PDDB_MAGIC {
                    log::error!("Basis root did not deserialize correctly, unrecoverable error.");
                    return None;
                }
                if basis_root.version != PDDB_VERSION {
                    log::error!("PDDB version mismatch in system basis root. Unrecoverable error.");
                    return None;
                }
                let basis_name = cstr_to_string(&basis_root.name.0);
                if basis_name != String::from(name) {
                    log::error!("Discovered basis name does not match the requested name: {}; aborting mount operation.", basis_name);
                    return None;
                }
                let mut bcache = BasisCacheEntry {
                    name: basis_name.clone(),
                    clean: true,
                    last_sync: Some(hw.timestamp_now()),
                    num_dicts: basis_root.num_dictionaries,
                    dicts: HashMap::<String, DictCacheEntry>::new(),
                    cipher,
                    aad,
                    age: basis_root.age,
                    free_dict_offset: None,
                    v2p_map: basis_map,
                    journal: u32::from_le_bytes(vpage[..size_of::<JournalType>()].try_into().unwrap()),
                    large_alloc_ptr: None,
                };
                if !lazy {
                    bcache.populate_caches(hw);
                }
                log::info!("Basis {} found and reconstructed", name);
                return Some(bcache);
            } else {
                // i guess technically we could try a brute-force search for the page if it went missing, but meh.
                log::error!("Basis {} did not contain a root page -- unrecoverable error.", name);
                return None;
            }
        } else {
            log::error!("Basis {} has no page table entries -- maybe a bad password?", name);
            None
        }
    }
    /// called during the initial basis scan to track where the large allocation pointer end should be.
    /// basically try to find the maximal extent of already allocated data, and start allocating from there.
    pub(crate) fn large_pool_update(&mut self, maybe_end: u64) {
        if maybe_end > self.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START)).as_u64() {
            self.large_alloc_ptr = Some(PageAlignedVa::from(maybe_end));
        }
    }
    /// allocate a pointer data in the large pool, of length `amount`. "always" succeeds because...
    /// there's 16 million terabytes of large pool to allocate before you run out?
    pub(crate) fn large_pool_alloc(&mut self, amount: u64) -> u64 {
        let alloc_ptr = self.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START));
        self.large_alloc_ptr = Some(self.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START)) + PageAlignedVa::from(amount));
        return alloc_ptr.as_u64()
    }
    /// do a deep scan of all the dictionaries and keys and attempt to populate all the caches
    pub(crate) fn populate_caches(&mut self, hw: &mut PddbOs) {
        let mut try_entry = 1;
        let mut dict_count = 0;
        while try_entry <= DICT_MAXCOUNT && dict_count < self.num_dicts {
            let dict_vaddr = VirtAddr::new(try_entry as u64 * DICT_VSIZE).unwrap();
            if let Some(pp) = self.v2p_map.get(&dict_vaddr) {
                if let Some(dict) = self.dict_decrypt(hw, &pp) {
                    if dict.flags.valid() {
                        let dict_name = String::from(cstr_to_string(&dict.name));
                        let mut dcache = DictCacheEntry::new(dict, try_entry, &self.aad);
                        let max_large_alloc = dcache.fill(hw, &self.v2p_map, &self.cipher);
                        self.dicts.insert(dict_name, dcache);
                        self.large_pool_update(max_large_alloc.get());
                        dict_count += 1;
                    } else {
                        // this is an empty dictionary entry. we could stick a dictionary in here later on, take note if we haven't already computed that
                        if self.free_dict_offset.is_none() { self.free_dict_offset = Some(try_entry as u32); }
                    }
                } else {
                    if self.free_dict_offset.is_none() { self.free_dict_offset = Some(try_entry as u32); }
                }
            } else {
                if self.free_dict_offset.is_none() { self.free_dict_offset = Some(try_entry as u32); }
            }
            try_entry += 1;
        }
        if try_entry <= DICT_MAXCOUNT {
            if self.free_dict_offset.is_none() { self.free_dict_offset = Some(try_entry as u32); }
        }
    }

    /// If `paranoid` is true, it recurses through each key and replaces its data with random junk.
    /// Otherwise, it does a "shallow" delete and just removes the directory entry, which is much
    /// more performant. Note that the intended "fast" way to secure-erase data is to store sensitive
    /// data in its own Basis, and then remove the Basis itself. This is much faster than picking
    /// through compounded data and re-writing partias sectors, and because of this, initially,
    /// the `paranoid` erase is `unimplemented`.
    pub(crate) fn dict_delete(&mut self, hw: &mut PddbOs, name: &str, paranoid: bool) -> Result<()> {
        if let Some((index, _dict)) = self.dict_deep_search(hw, name) {
            if !paranoid {
                // erase the header by writing over with random data. This makes the dictionary unsearchable, but if you
                // have the key, you can of course do a hard-scan and try partially re-assemble the dictionary.
                if let Some(pp) = self.v2p_map.get(&VirtAddr::new(index as u64 * DICT_VSIZE as u64).unwrap()) {
                    let mut random = [0u8; PAGE_SIZE];
                    hw.trng_slice(&mut random);
                    hw.patch_data(&random, pp.page_number() * PAGE_SIZE as u32);
                } else {
                    log::warn!("Inconsistent internal state: requested dictionary didn't have a mapping in the page table.");
                }
            } else {
                unimplemented!("For now store sensitive data in its own Basis, and then delete the Basis.");
                // this is a bit of an arduous code path, it involves recursing through all the keys and nuking
                // them. Let's write this after we've actually _got keys_ (we're just figuring out how to add a dictionary
                // in the first place right now!).
            }

            // de-allocate all of the dictionary entries
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Dictionary not found"))
        }
    }

    pub(crate) fn dict_get_free_offset(&mut self, hw: &mut PddbOs) -> u32 {
        if let Some(offset) = self.free_dict_offset.take() {
            return offset;
        } else {
            let mut try_entry = 1;
            while try_entry <= DICT_MAXCOUNT {
                let dict_vaddr = VirtAddr::new(try_entry as u64 * DICT_VSIZE).unwrap();
                if let Some(pp) = self.v2p_map.get(&dict_vaddr) {
                    if self.dict_decrypt(hw, &pp).is_none() {
                        // mapping exists, but the data is invalid. It's a free entry.
                        return try_entry as u32
                    }
                } else {
                    // mapping doesn't exist yet, that's a free entry
                    return try_entry as u32
                }
                try_entry += 1;
            }
        }
        // maybe we should handle this better? but, I think this should be super-rare, as we allow 16384 dictionaries per Basis,
        // so if we got here I'm guessing it's more likely due to a coding error than an actually full Basis.
        panic!("Basis full, can't allocate dictionary");
    }

    /// Returns a tuple of "dictionary offset" , "dictionary"; the "dictionary offset" needs to be multiplied by DICT_VSIZE to arrive
    /// at a fully expanded virtual address.
    pub(crate) fn dict_deep_search(&mut self, hw: &mut PddbOs, name: &str) -> Option<(u32, Dictionary)> {
        let mut try_entry = 1;
        let mut dict_count = 0;
        while try_entry <= DICT_MAXCOUNT && dict_count < self.num_dicts {
            let dict_vaddr = VirtAddr::new(try_entry as u64 * DICT_VSIZE).unwrap();
            if let Some(pp) = self.v2p_map.get(&dict_vaddr) {
                if let Some(dict) = self.dict_decrypt(hw, &pp) {
                    if cstr_to_string(&dict.name) == name {
                        return Some((try_entry as u32, dict))
                    }
                    dict_count += 1;
                } else {
                    // this is an empty dictionary entry. we could stick a dictionary in here later on, take note if we haven't already computed that
                    if self.free_dict_offset.is_none() {
                        self.free_dict_offset = Some(try_entry as u32);
                    }
                }
            } else {
                // this is an empty dictionary entry. we could stick a dictionary in here later on, take note if we haven't already computed that
                if self.free_dict_offset.is_none() {
                    self.free_dict_offset = Some(try_entry as u32);
                }
            }
            try_entry += 1;
        }
        None
    }

    /// The `pp` must be the resolved physical page storing the top of the given
    /// dictionary index for this to work.
    pub(crate) fn dict_decrypt(&self, hw: &mut PddbOs, pp: &PhysPage) -> Option<Dictionary> {
        if let Some(data) = hw.data_decrypt_page(&self.cipher, &self.aad, &pp) {
            let mut dict = Dictionary::default();
            for (&src, dst) in data[size_of::<JournalType>()..].iter().zip(dict.deref_mut().iter_mut()) {
                *dst = src;
            }
            Some(dict)
        } else {
            None
        }
    }

    /// Looks for dirty entries in the page table, and flushes them to disk.
    pub(crate) fn pt_sync(&mut self, hw: &mut PddbOs) {
        for (&virt, phys) in self.v2p_map.iter_mut() {
            if !phys.clean() {
                log::info!("syncing dirty pte va: {:x?} pa: {:x?}", virt, phys);
                hw.pt_patch_mapping(virt, phys.page_number());
                phys.set_clean(true);
            }
        }
    }

    /// This will sync the named Dictionary header + dirty *key descriptors*. It does not
    /// sync the key data itself. Note this does not mark the surrounding basis structure as clean
    /// when it exits, even if there are no more dirty entries within.
    ///
    /// Significantly, this routine assumes that every dictionary entry has already had a v2p mapping
    /// allocated. You can try calling it without pre-allocating the entries, but if the FastSpace structure
    /// doesn't have enough space, the routine will return an error indicating we're out of memory.
    /// You could then try to allocate more FastSpace, and re-try the sync operation.
    pub(crate) fn dict_sync(&mut self, hw: &mut PddbOs, name: &str) -> Result<()> {
        if let Some(dict) = self.dicts.get_mut(&String::from(name)) {
            let dict_offset = VirtAddr::new(dict.index as u64 * DICT_VSIZE).unwrap();
            if !dict.clean {
                log::info!("syncing dictionary {}", name);
                let mut dict_name = [0u8; DICT_NAME_LEN];
                for (src, dst) in name.bytes().into_iter().zip(dict_name.iter_mut()) {
                    *dst = src;
                }
                let dict_disk = Dictionary {
                    flags: dict.flags,
                    age: dict.age,
                    num_keys: dict.key_count,
                    name: dict_name,
                };
                log::info!("syncing dict: {:?}", dict_disk);
                // log::info!("raw: {:x?}", dict_disk.deref());
                // observation: all keys to be flushed to disk will be in the KeyCacheEntry. Some may be clean,
                // but definitely all the dirty ones are in there (if they aren't, where else would they be??)

                // this is the virtual page within the dictionary region that we're currently serializing
                let mut vpage_num = 0;
                loop {
                    // 1. resolve the virtual address to a target page
                    let cur_vpage = VirtAddr::new(dict_offset.get() + (vpage_num as u64 * VPAGE_SIZE as u64)).unwrap();
                    if !self.v2p_map.contains_key(&cur_vpage) {
                        if let Some(pp) = hw.try_fast_space_alloc() {
                            self.v2p_map.insert(cur_vpage, pp);
                        } else {
                            return Err(Error::new(ErrorKind::OutOfMemory, "FastSpace empty"));
                        }
                    }
                    let pp = self.v2p_map.get(&cur_vpage).unwrap();

                    // 2(a). fill in the target vpage with data: header special case
                    let mut dk_vpage = DictKeyVpage::default();
                    // the dict always occupies the first entry of the first vpage in the dictionary region
                    if vpage_num == 0 {
                        let mut dk_entry = DictKeyEntry::default();
                        for (&src, dst) in dict_disk.deref().iter().zip(dk_entry.data.iter_mut()) {
                            *dst = src;
                        }
                        dk_vpage.elements[0] = Some(dk_entry);
                    }

                    // 2(b). fill in the target vpage with data: general key case
                    // Scan the DictCacheEntry.keys record for dirty keys within the current target vpage's range
                    // this is not a terribly efficient operation right now, because the DictCacheEntry is designed to
                    // be searched by name, but in this case, we want to check for an index range. There's a lot
                    // of things we could do to optimize this, depending on the memory/time trade-off we want to
                    // make, but for now, let's do it with a dumb O(N) scan through the KeyCacheEntry, running under
                    // the assumption that the KeyCacheEntry doesn't ever get to a very large N.
                    let next_vpage = VirtAddr::new(cur_vpage.get() + VPAGE_SIZE as u64).unwrap();
                    for (key_name, key) in dict.keys.iter_mut() {
                        log::info!("merging in key {}", key_name);
                        if !key.clean {
                            if key.descriptor_vaddr(dict_offset) >= cur_vpage &&
                            key.descriptor_vaddr(dict_offset) < next_vpage {
                                // key is within the current page, add it to the target list
                                let mut dk_entry = DictKeyEntry::default();
                                let mut kn = [0u8; KEY_NAME_LEN];
                                for (&src, dst) in key_name.as_bytes().iter().zip(kn.iter_mut()) {
                                    *dst = src;
                                }
                                let key_desc = KeyDescriptor {
                                    start: key.start,
                                    len: key.len,
                                    reserved: key.reserved,
                                    flags: key.flags,
                                    age: key.age,
                                    name: kn,
                                };
                                for (&src, dst) in key_desc.deref().iter().zip(dk_entry.data.iter_mut()) {
                                    *dst = src;
                                }
                                dk_vpage.elements[key.descriptor_modulus()] = Some(dk_entry);
                            }
                            key.clean = true;
                        }
                    }

                    // 3. merge the vpage modifications into the disk
                    let mut page = if let Some(data) = hw.data_decrypt_page(&self.cipher, &self.aad, &pp) {
                        log::info!("merging dictionary data into existing page");
                        data
                    } else {
                        log::info!("existing data invalid, creating a new page");
                        // the existing data was invalid (this happens e.g. on the first time a dict is created). Just overwrite the whole page.
                        vec![0u8; VPAGE_SIZE + size_of::<JournalType>()]
                    };
                    for (index, stride) in page[size_of::<JournalType>()..].chunks_mut(DK_STRIDE).enumerate() {
                        if let Some(elem) = dk_vpage.elements[index] {
                            for (&src, dst) in elem.data.iter().zip(stride.iter_mut()) {
                                *dst = src;
                            }
                        }
                    }
                    // generate nonce and write out
                    hw.data_encrypt_and_patch_page(&self.cipher, &self.aad, &mut page, &pp);

                    // 4. Check for dirty keys, if there are still some, update vpage_num to target them; otherwise
                    // exit the loop
                    let mut found_next = false;
                    for key in dict.keys.values() {
                        if !key.clean {
                            found_next = true;
                            // note: we don't care *which* vpage we do next -- so we just break after finding the first one
                            vpage_num = key.descriptor_vpage_num();
                            break;
                        }
                    }
                    if !found_next {
                        break;
                    }
                }
                dict.clean = true;
            }
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "dict_sync called with an invalid dictionary name"))
        }
    }

    /// Runs through the dictionary listing in a basis and compacts them. Call when the
    /// the dictionary space becomes sufficiently fragmented that accesses are becoming
    /// inefficient.
    pub(crate) fn dict_compact(&self, basis_name: Option<&str>) -> Result<()> {
        unimplemented!();
    }

    /// Syncs *only* the basis header to disk.
    pub(crate) fn basis_sync(&mut self, hw: &mut PddbOs) {
        if !self.clean {
            let basis_root = BasisRoot {
                magic: PDDB_MAGIC,
                version: PDDB_VERSION,
                name: BasisRootName::try_from_str(&self.name).unwrap(),
                age: self.age,
                num_dictionaries: self.num_dicts,
            };
            let aad = basis_root.aad(hw.dna());
            let pp = self.v2p_map.get(&VirtAddr::new(1 * VPAGE_SIZE as u64).unwrap())
                .expect("Internal consistency error: Basis exists, but its root map was not allocated!");
            let journal_bytes = self.journal.to_le_bytes(); // journal gets bumped by the patching function now
            let slice_iter =
                journal_bytes.iter() // journal rev
                .chain(basis_root.as_ref().iter());
            let mut block = [0 as u8; VPAGE_SIZE + size_of::<JournalType>()];
            for (&src, dst) in slice_iter.zip(block.iter_mut()) {
                *dst = src;
            }
            hw.data_encrypt_and_patch_page(&self.cipher, &self.aad, &mut block, &pp);
            self.clean = true;
        }
    }
}

// ****
// Beginning of serializers for the data structures in this file.
// ****

/// Newtype for BasisRootName so we can give it a default initializer.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub struct BasisRootName(pub [u8; BASIS_NAME_LEN]);
impl BasisRootName {
    pub fn try_from_str(name: &str) -> Result<BasisRootName> {
        let mut alloc = [0u8; BASIS_NAME_LEN];
        let bytes = name.as_bytes();
        if bytes.len() > BASIS_NAME_LEN {
            Err(Error::new(ErrorKind::InvalidInput, "basis name is too long")) // FileNameTooLong is still nightly :-/
        } else {
            for (&src, dst) in bytes.iter().zip(alloc.iter_mut()) {
                *dst = src;
            }
            Ok(BasisRootName(alloc))
        }
    }
}
impl Default for BasisRootName {
    fn default() -> BasisRootName {
        BasisRootName([0; BASIS_NAME_LEN])
    }
}

/* this is retired, but kept around because it's a cool idea that could be useful later.
   the code below is how the encryptor iterator would be called, followed by the iterator itself.
   The idea is to create a thing that can chain iterators and generate encrypted blocks. Could
   be useful for e.g. encrypting long keys and stuff...?

   The code was retired only because we decided to simplify the BasisRoot and it always fits in
   one page, so all this iterator is just baroque and frankly kind of pointless in that context.

        if let Some(syskey) = self.system_basis_key {
            let key = Key::from_slice(&syskey);
            let cipher = Aes256GcmSiv::new(key);
            let basis_encryptor = BasisEncryptor::new(
                &basis_root,
                self.dna,
                cipher,
                0,
                Rc::clone(&self.entropy),
            );
            for (&k, &v) in basis_v2p_map.iter() {
                log::info!("basis_v2p_map retrieved va: {:x?} pp: {:x?}", k, v);
            }
            for (vpage_no, ppage_data) in basis_encryptor.into_iter().enumerate() {
                let vaddr = VirtAddr::new( ((vpage_no + 1) * VPAGE_SIZE) as u64 ).unwrap();
                match basis_v2p_map.get(&vaddr) {
                    Some(pp) => {
                        self.patch_data(&ppage_data, pp.page_number() * PAGE_SIZE as u32);
                    }
                    None => {
                        log::error!("Previously allocated page was not found in our map!");
                        panic!("Inconsistent internal state");
                    }
                }
            }
        } else {
            log::error!("System key was not found, but it should be present!");
            panic!("Inconsistent internal state");
        }
/// Takes in the constituents of the Basis area, and encrypts them into
/// PAGE_SIZE blocks. Can be called as an iterator, or as a single-shot
/// for a given offset. Requires a cipher that is pre-keyed with the encryption
/// key, and the DNA code from the FPGA as a `u64`. This function generates
/// the AAD based off of the DNA code + version of PDDB + Basis Name.
///
/// The iteration step is in VPAGE units within the virtual space, but
/// it always returns a full PAGE_SIZE block. This object will handle
/// padding of the very last block so the encrypted data fills up a full
/// PAGE_SIZE; request for blocks beyond the length of the Basis pre-alloc
/// region will return None.
///
/// This routine is a bit heavyweight because we were originally going to
/// attach the dictionary data to the Basis Root but have since decided against that.
#[repr(C)]
pub(crate) struct BasisEncryptor<'a> {
    root: &'a BasisRoot,
    cipher: Aes256GcmSiv,
    cur_vpage: usize,
    aad: Vec::<u8>,
    journal_rev: JournalType,
    entropy: Rc<RefCell<TrngPool>>,
}
impl<'a> BasisEncryptor<'a> {
    pub(crate) fn new(root: &'a BasisRoot, dna: u64, cipher: Aes256GcmSiv, rev: JournalType, entropy: Rc<RefCell<TrngPool>>) -> Self {
        let mut aad = Vec::<u8>::new();
        aad.extend_from_slice(&root.name.0);
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        aad.extend_from_slice(&dna.to_le_bytes());

        log::info!("aad: {:?}", aad);

        BasisEncryptor {
            root,
            cur_vpage: 0,
            aad,
            cipher,
            journal_rev: rev,
            entropy,
        }
    }
}

pub(crate) struct BasisEncryptorIter<'a> {
    basis_data: BasisEncryptor<'a>,
    // the virtual address of the currently requested iteration
    vaddr: usize,
}
impl<'a> IntoIterator for BasisEncryptor<'a> {
    type Item=[u8; PAGE_SIZE];
    type IntoIter=BasisEncryptorIter<'a>;
    fn into_iter(self) -> BasisEncryptorIter<'a> {
        BasisEncryptorIter {
            basis_data: self,
            vaddr: 0,
        }
    }
}
impl<'a> Iterator for BasisEncryptorIter<'a> {
    type Item = [u8; PAGE_SIZE];

    fn next<'s>(&'s mut self) -> Option<Self::Item> {
        if self.vaddr < VPAGE_SIZE { // legacy from when we tried to have a multi-page basis area
            let mut block = [0 as u8; VPAGE_SIZE + size_of::<JournalType>()];
            let block_iter = block.iter_mut();

            let journal_bytes = self.basis_data.journal_rev.to_le_bytes();
            let slice_iter =
            journal_bytes.iter() // journal rev
                .chain(self.basis_data.root.as_ref().iter()
            ).skip(self.vaddr);

            // note that in the case that we've already serialized the journal, basis, and dictionary, this will produce nothing
            let mut written = 0;
            for(&src, dst) in slice_iter.zip(block_iter) {
                *dst = src;
                written += 1;
            }
            // which allows this to correctly pad out the rest of the prealloc region with 0's.
            while written < block.len() {
                block[written] = 0;
                written += 1;
            }

            let nonce_array = self.basis_data.entropy.borrow_mut().get_nonce();
            let nonce = Nonce::from_slice(&nonce_array);
            let ciphertext = self.basis_data.cipher.encrypt(
                &nonce,
                Payload {
                    aad: &self.basis_data.aad,
                    msg: &block,
                }
            ).unwrap();
            self.vaddr += VPAGE_SIZE;
            //log::info!("nonce: {} ct: {} total: {}", nonce_array.len(), ciphertext.deref().len(), nonce_array.len() + ciphertext.deref().len());
            Some([&nonce_array, ciphertext.deref()].concat().try_into().unwrap())
        } else {
            None
        }
    }
}
*/
/*
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    // turns out these test are invalid because the structures always end up being aligned
    fn test_dict_size() {
        print!("dict size: {}", core::mem::size_of::<Dictionary>());
        assert!(core::mem::size_of::<Dictionary>() == 127, "Dictionary is not an even multiple of the VPAGE size");
    }
    #[test]
    fn test_key_size() {
        print!("key size: {}", core::mem::size_of::<KeyDescriptor>());
        assert!(core::mem::size_of::<KeyDescriptor>() == 127, "Key descriptor is not an even multiple of the VPAGE size");
    }
}
*/