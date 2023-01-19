/// # The Organization of Basis Data
///
/// ## Overview
/// In basis space, the BasisRoot is located at VPAGE #1 (VPAGE #0 is always invalid).
/// A VPAGE is 0xFE0 (4,064) bytes long, which is equal to a PAGE of 4k minus 32 bytes of encryption+journal
/// overhead.
///
/// *NB: The factors of 4,064 are: 1, 2, 4, 8, 16, 32, 127, 254, 508, 1016, 2032, 4064*
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
/// ## Basis Virtual Memory Layout
///```Text
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
/// ```
///
/// Note that each Basis has its own memory section, and you can have "many" orthogonal Basis without
/// a collision -- the AES keyspace is 128 bits, so you have a decent chance of no collisions
/// even with a few billion Basis concurrently existing in the filesystem.
///
/// ## Memory Pools
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
/// ## The Alignment and Serialization Chronicles
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
/// ## Known Repr(C) footguns:
///  - When you start laying in 64-bit types, stuff has to be 64-bit aligned, or else you'll start to get
///    uninitialized padding data inserted, which can leak stack data in the serialization process.
///  - Don't use anything that's not native to C. In particular, for primitives that we want to be "Option"
///    wrapped, we're using a NonZeroU64 format. The compiler knows how to turn that into a 64-bit C-friendly
///    structure and serialize/deserialize that into the correct Rust structure. See
///    https://doc.rust-lang.org/nomicon/other-reprs.html for a citation on that.

use crate::api::*;
use super::*;

use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use std::convert::TryInto;
use aes_gcm_siv::{
    aead::KeyInit,
    Aes256GcmSiv,
};
use aes::Aes256;
use aes::cipher::generic_array::GenericArray;
use std::iter::IntoIterator;
use std::collections::{BinaryHeap, HashMap, HashSet, BTreeSet};
use std::io::{Result, Error, ErrorKind};
use std::cmp::Reverse;
use std::cmp::Ordering;
use core::num::NonZeroU32;

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
/// This is a size limit on the biggest file you can create. It's currently 32GiB. No, this is not
/// web scale, but it's big enough to hold a typical blu-ray movie as a single file. You can adjust
/// this constant up or down, and the trade-off is, you get more or less total number of large files
/// allocated over the life of the filesystem. We simply "increment a pointer" when a new large file
/// is added to create the next virtual memory spot for the large file. So at 32GiB, you can create
/// a lifetime total of about 200 million files (this includes files you've previously deleted, until
/// we create a mechanism for sweeping through the memory space and tracking de-allocations). Note that
/// a "large" file includes anything over 4kiB, so if you create a 5kiB file, it can potentially grow to
/// 32 GiB without bumping into the next large file. This is a very "lazy" way to deal with large files.
/// Given that the PDDB is designed for a 32-bit device with only 128MiB of memory and a read/write lifetime
/// of 100k cycles for the FLASH, 200 million file allocations is probably greater than the lifetime of
/// the device itself. If the PDDB migrates to a larger handphone-style application, I think it'll probably
/// still hold up OK with 200 million total large file allocations over the device lifetime and a limit
/// of 32GiB. That's about 73k files created per day for 10 years, or about 50 files per minute -- roughly
/// one new file per second for 10 years straight before the PDDB runs out of virtual memory space.
/// A web server creating a >4k temporary log file for every client that hit and then deleting it
/// would probably crush this limit in months. So don't use the PDDB to back a high volume web server.
/// But it's probably OK for a consumer electronics device with a typical lifetime of less than 10 years.
/// If you really think you want larger files and also more write life, you'd need to implement an in-memory
/// "free" file allocator, but honestly, this is not something I think we need to burn resources on for
/// the initial target of the PDDB (that is, a 100MiB device with 100k read/write endurance lifetime).
/// Anyways, the code is written so you can just slide this constant up or down and change the behavior
/// of the system; it's recommended you reformat when you do that but I /think/ it should actually be OK
/// if you made a change "on the fly".
///
/// Also note that in practice, a file size is limited to 4GiB on a 32-bit Precursor device anyways
/// because the usize type isn't big enough. Recompiling for a 64-bit target, however, should give
/// you access to the 32GiB file size limit.
pub(crate) const LARGE_FILE_MAX_SIZE: u64 = 0x0000_0008_0000_0000;

/// The chosen "stride" of a dict/key entry. Drives a lot of key parameters in the database's characteristics.
/// This is chosen such that 32 of these entries fit evenly into a VPAGE.
pub(crate) const DK_STRIDE: usize = 127;
//// DK_STRIDES per VPAGE
pub(crate) const DK_PER_VPAGE: usize = VPAGE_SIZE / DK_STRIDE; // should be 32 - use this for computing modulus on dictionary indices
/// size of a dictionary region in virtual memory
pub(crate) const DICT_VSIZE: u64 = 0xFE_0000;
/// maximum number of dictionaries in a system
pub(crate) const DICT_MAXCOUNT: usize = 16383;
/// default alloc hint, if none is given (needs to be non-zero)
/// this would be the typical "minimum space" reserved for a key
/// users are of course allowed to specify something smaller, but it should be non-zero
pub(crate) const DEFAULT_ALLOC_HINT: usize = 8;

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
        aad.extend_from_slice(&self.name.data[..self.name.len as usize]);
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
    /// ticktimer reference, for managing atimes
    pub(crate) tt: ticktimer_server::Ticktimer,
    /// data cache - stores the most recently decrypted pages of data
    data_cache: PlaintextCache,
}
impl BasisCache {
    pub(crate) fn new() -> Self {
        BasisCache {
            cache: Vec::new(),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            data_cache: PlaintextCache { data: None, tag: None },
        }
    }
    /// Returns a Vec which is a list of Bases to visit, in order of visitation, to create the union view.
    pub(crate) fn access_list(&self) -> Vec::<String> {
        let mut al = Vec::<String>::new();
        for entry in self.cache.iter().rev() {
            al.push(entry.name.to_string());
        }
        al
    }
    pub(crate) fn rekey(&self, hw: &mut PddbOs, op: PddbRekeyOp) -> PddbRekeyOp {
        hw.pddb_rekey(op, &self.cache)
    }
    fn select_basis(&self, basis_name: Option<&str>) -> Option<usize> {
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
    pub(crate) fn basis_count(&self) -> usize {self.cache.len()}

    /// Adds a dictionary with `name` to:
    ///    - if `basis_name` is None, the most recently opened basis
    ///    - if `basis_name` is Some, searches for the given basis and adds the dictionary to that.
    /// If the dictionary already exists, it returns an informative error.
    pub(crate) fn dict_add(&mut self, hw: &mut PddbOs, name: &str, basis_name: Option<&str>) -> Result<()> {
        if !hw.ensure_fast_space_alloc(2, &self.cache) {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to allocate dict"));
        }
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            basis.age = basis.age.saturating_add(1);

            if basis.ensure_dict_in_cache(hw, name) {
                return Err(Error::new(ErrorKind::AlreadyExists, "Dictionary already exists"));
            }
            basis.clean = false;
            // allocate a vpage offset for the dictionary
            let dict_index = basis.dict_get_free_offset(hw);
            let dict_offset = VirtAddr::new(dict_index as u64 * DICT_VSIZE).unwrap();
            log::debug!("dict_add at VA 0x{:x?}", dict_offset);
            let pp = basis.v2p_map.entry(dict_offset).or_insert_with(|| {
                let mut ap = hw.try_fast_space_alloc().expect("No free space to allocate dict");
                ap.set_valid(true);
                ap
            });
            log::debug!("dict_add at PA 0x {:x?}", pp);
            assert!(pp.valid(), "v2p returned an invalid page");

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
            let mut free_keys = BinaryHeap::<Reverse<FreeKeyRange>>::new();
            free_keys.push(Reverse(FreeKeyRange{start: 1, run: KEY_MAXCOUNT as u32 - 1}));
            let dict_cache = DictCacheEntry {
                index: NonZeroU32::new(dict_index).unwrap(),
                keys: HashMap::<String, KeyCacheEntry>::new(),
                clean: false,
                age: 0,
                free_keys,
                last_disk_key_index: 1, // we know we don't have to search past this in a new dictionary
                flags: init_flags,
                key_count: 0,
                small_pool: Vec::<KeySmallPool>::new(),
                small_pool_free: BinaryHeap::<KeySmallPoolOrd>::new(),
                aad: my_aad,
                created: std::time::Instant::now(),
            };
            log::debug!("adding dictionary {}", name);
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

    /// Returns a list of all the known dictionaries, across all the basis. A HashSet is returned
    /// because you can have the same-named dictionary in multiple basis, and what we're asking for
    /// is the union of all the dictionary names, without duplicates.
    pub(crate) fn dict_list(&mut self, hw: &mut PddbOs, basis_name: Option<&str>) -> HashSet::<String> {
        let mut dict_set = HashSet::<String>::new();
        if basis_name.is_some() {
            if let Some(basis_index) = self.select_basis(basis_name) {
                let basis = &mut self.cache[basis_index];
                basis.populate_caches(hw);
                for (key, dcache) in basis.dicts.iter() {
                    if dcache.flags.valid() {
                        dict_set.insert(String::from(key));
                    }
                }
            }
        } else {
            for basis in self.cache.iter_mut() {
                basis.populate_caches(hw);
                for (key, dcache) in basis.dicts.iter() {
                    if dcache.flags.valid() {
                        dict_set.insert(String::from(key));
                    }
                }
            }
        }
        dict_set
    }
    pub(crate) fn key_list(&mut self, hw: &mut PddbOs, dict: &str, basis_name: Option<&str>) -> Result<BTreeSet::<String>> {
        let mut merge_list = BTreeSet::<String>::new();
        let mut found_dict = false;
        if basis_name.is_some() {
            if let Some(basis_index) = self.select_basis(basis_name) {
                let basis = &mut self.cache[basis_index];
                basis.populate_caches(hw);
                if let Some(dcache) = basis.dicts.get_mut(dict) {
                    dcache.key_list(hw, &basis.v2p_map, &basis.cipher, &mut merge_list);
                    found_dict = true;
                }
            }
        } else {
            for basis in self.cache.iter_mut() {
                basis.populate_caches(hw);
                if let Some(dcache) = basis.dicts.get_mut(dict) {
                    dcache.key_list(hw, &basis.v2p_map, &basis.cipher, &mut merge_list);
                    found_dict = true;
                }
            }
        }
        if found_dict {
            Ok(merge_list)
        } else {
            return Err(Error::new(ErrorKind::NotFound, "dictionary not found"))
        }
    }

    /// This version of the call only removes one instance of a dictionary from the specified basis.
    /// Perhaps there also needs to be a `dict_remove_all` call which iterates through every basis
    /// makes sure the dictionary is removed from all the possible known basis. Anyways, that function
    /// would be a variant of this targeted version.
    pub(crate) fn dict_remove(&mut self,
        hw: &mut PddbOs, dict: &str, basis_name: Option<&str>, paranoid: bool
    ) -> Result<()> {
        if let Some(basis_index) = self.select_basis(basis_name) {
            log::debug!("deleting dict {}", dict);
            let basis = &mut self.cache[basis_index];

            basis.age = basis.age.saturating_add(1);
            basis.clean = false;
            basis.dict_delete(hw, dict, paranoid)?;
            basis.basis_sync(hw);
            basis.pt_sync(hw);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
        }
    }

    pub(crate) fn key_read(&mut self, hw: &mut PddbOs, dict: &str, key: &str, data: &mut [u8],
        offset: Option<usize>, basis_name:Option<&str>
    ) -> Result<usize> {
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            if !basis.ensure_dict_in_cache(hw, dict) {
                return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
            }
            if let Some(dict_entry) = basis.dicts.get_mut(dict) {
                if dict_entry.ensure_key_entry(hw, &mut basis.v2p_map, &basis.cipher, key) {
                    let mut needs_fill = false;
                    loop { // this weird loop works around an interior mutability issue with filling key data. It's awful, like a `goto`.
                        if needs_fill {
                            // fetch the data from disk
                            log::debug!("Small key evicted: filling {}:{}", dict, key);
                            // this call does a mutable borrow of dict_entry, which interferes with getting kcache below.
                            dict_entry.refill_small_key(hw, &basis.v2p_map, &basis.cipher, &mut self.data_cache, key);
                        }
                        let kcache = dict_entry.keys.get_mut(key).expect("Entry was assured, but then not there!");
                        kcache.set_atime(self.tt.elapsed_ms());
                        // the key exists, *and* there's sufficient space for the data
                        if kcache.start < SMALL_POOL_END {
                            // small pool fetch
                            if kcache.data.is_none() {
                                needs_fill = true;
                                // loop starts again at the top, but this time filling the key first.
                                continue;
                            }
                            // at this point, if we have a small key, we also have its data in cache.
                            if let KeyCacheData::Small(cache_data) = kcache.data.as_mut().expect("small pool should all have their data 'hot' if the index entry is also in cache") {
                                let mut bytes_read = 0;
                                if offset.unwrap_or(0) as u64 > kcache.len {
                                    return Err(Error::new(ErrorKind::UnexpectedEof, "offest requested is beyond the key length"));
                                }
                                for (&src, dst) in cache_data.data[offset.unwrap_or(0)..kcache.len as usize].iter().zip(data.iter_mut()) {
                                    *dst = src;
                                    bytes_read += 1;
                                }
                                if bytes_read != data.len() {
                                    log::debug!("Key shorter than read buffer {}:{} ({}/{})", dict, key, bytes_read, data.len());
                                }
                                return Ok(bytes_read)
                            } else {
                                panic!("Key allocated to small area but its cache data was not of the small type");
                            }
                        } else {
                            if offset.unwrap_or(0) as u64 > kcache.len {
                                return Err(Error::new(ErrorKind::UnexpectedEof, "offest requested is beyond the key length"));
                            }
                            if data.len() == 0 { // mostly because i don't want to have to think about this case in the later logic.
                                return Ok(0)
                            }
                            // large pool fetch
                            let mut abs_cursor = offset.unwrap_or(0) as u64;
                            let mut blocks_read = 0;
                            let mut bytes_read = 0;
                            loop {
                                let start_vpage_addr = ((kcache.start + abs_cursor) / VPAGE_SIZE as u64) * VPAGE_SIZE as u64;

                                if let Some(pp) = basis.v2p_map.get(&VirtAddr::new(start_vpage_addr).unwrap()) {
                                    let block_start_pos = (abs_cursor % VPAGE_SIZE as u64) as usize;
                                    assert!(pp.valid(), "v2p returned an invalid page");
                                    let pt_data = hw.data_decrypt_page(&basis.cipher, &basis.aad, pp).expect("Decryption auth error");
                                    if blocks_read != 0 {
                                        assert!(block_start_pos == 0, "algorithm error in handling offset data");
                                    }
                                    if blocks_read == 0 {
                                        log::debug!("reading {} abs: {}, block_start: {}, block: {}, data.len:{} kcache.len:{}",
                                            key, abs_cursor, block_start_pos, blocks_read, data.len(), kcache.len);
                                    } else {
                                        log::debug!("  reading {} abs: {}, block_start: {}, block: {}, remaining.data:{} kcache.len:{}",
                                            key, abs_cursor, block_start_pos, blocks_read, data.len() - bytes_read, kcache.len);
                                    }
                                    let data_offset = bytes_read;
                                    for (&src, dst) in
                                    pt_data[
                                        size_of::<JournalType>() // always this fixed offset per block
                                        + block_start_pos
                                        ..
                                    ].iter().zip(data[data_offset..].iter_mut()) {
                                        *dst = src;
                                        // it'd be computationally more efficient to figure out what this should be going into
                                        // every copy loop, but it's logically easier to think about in this form. Without this
                                        // check, a user could read past the allocated space for a block...
                                        if abs_cursor >= kcache.len {
                                            break;
                                        }
                                        abs_cursor += 1;
                                        bytes_read += 1;
                                    }
                                    blocks_read += 1;
                                } else {
                                    log::warn!("Not enough bytes available to read for key {}:{} ({}/{})", dict, key, abs_cursor, data.len());
                                    return Ok(bytes_read as usize)
                                }
                                if abs_cursor >= kcache.len {
                                    break;
                                }
                                if bytes_read >= data.len() {
                                    break;
                                }
                            }
                            return Ok(bytes_read as usize)
                        }
                        // note that from this point forward, either the "if" or the "else" branch returns;
                        // the end of this loop is never seen, unless a `continue` statement is hit within the loop.
                    }
                } else {
                    return Err(Error::new(ErrorKind::NotFound, "key not found"));
                }
            } else {
                return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
            }
        } else {
            Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
        }
    }

    pub(crate) fn key_remove(&mut self,
        hw: &mut PddbOs, dict: &str, key: &str, basis_name: Option<&str>, paranoid: bool
    ) -> Result<()> {
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            if !basis.ensure_dict_in_cache(hw, dict) {
                return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
            }
            if let Some(dict_entry) = basis.dicts.get_mut(dict) {
                basis.age = basis.age.saturating_add(1);
                basis.clean = false;
                if dict_entry.ensure_key_entry(hw, &mut basis.v2p_map, &basis.cipher, key) {
                    if !paranoid {
                        dict_entry.key_remove(hw, &mut basis.v2p_map, &basis.cipher, key, false);
                    } else {
                        // this implementation is still in progress
                        dict_entry.key_erase(key);
                    }
                    // sync the key pools to disk
                    dict_entry.sync_large_pool();
                    // encrypt and write the dict entry to disk
                    basis.dict_sync(hw, dict)?;
                    // sync the root basis structure as well, while we're at it...
                    basis.basis_sync(hw);
                    // finally, sync the page tables.
                    basis.pt_sync(hw);
                    return Ok(())
                } else {
                    return Err(Error::new(ErrorKind::NotFound, "key not found"));
                }
            } else {
                return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
            }
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
        let mut pages_needed = 2; // things go badly when no space is available so make sure there's always at least 1 spot
        let reserved = if data.len() + offset.unwrap_or(0) > alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT) {
            data.len() + offset.unwrap_or(0)
        } else {
            if alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT) == 0 { // disallow 0-sized alloc hints, round up to the default size if someone tries 0
                DEFAULT_ALLOC_HINT
            } else {
                alloc_hint.unwrap_or(DEFAULT_ALLOC_HINT)
            }
        };
        let reserved_pages = if reserved % VPAGE_SIZE == 0 {
            reserved / VPAGE_SIZE
        } else {
            (reserved / VPAGE_SIZE) + 1
        };
        if let Some(basis_index) = self.select_basis(basis_name) {
            let basis = &mut self.cache[basis_index];
            if !basis.ensure_dict_in_cache(hw, dict) {
                pages_needed += 1;
                pages_needed += reserved_pages;
            }
            if let Some(dict_entry) = basis.dicts.get(dict) {
                // see if we need to make a kcache entry
                if let Some(kcache) = dict_entry.keys.get(key) {
                    if kcache.flags.valid() {
                        // now check for data reservations
                        if let Some(key_index) = small_storage_index_from_key(&kcache, dict_entry.index) {
                            // it's probably going in the small pool.
                            // index exists, see if the page exists
                            if reserved == 0 { // something went wrong here
                                log::debug!("reserved {}", reserved);
                                log::debug!("{}:{} - [{}]{:x}+{}->{}", dict, key, kcache.descriptor_index, kcache.start, kcache.len, kcache.reserved);
                            }
                            if dict_entry.small_pool.len() > key_index {
                                log::trace!("resolved key index {}, small pool len: {}", key_index, dict_entry.small_pool.len());
                                // see if the pool's address exists in the page table
                                let pool_vaddr = VirtAddr::new(small_storage_base_vaddr_from_indices(dict_entry.index, key_index)).unwrap();
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
                        pages_needed += reserved_pages;
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
        log::debug!("reserving {} pages - {}", pages_needed, reserved_pages);
        if !hw.ensure_fast_space_alloc(pages_needed, &self.cache) {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to allocate dict"));
        }
        // now actually do the update
        if let Some(basis_index) = self.select_basis(basis_name) {
            let dict_found = (&mut self.cache[basis_index]).ensure_dict_in_cache(hw, dict);
            if !dict_found { // now that we're clear of the deep search, mutate the basis if we are sure it's not there
                self.dict_add(hw, dict, basis_name).expect("couldn't add dictionary");
            }
            // at this point, the dictionary should definitely be in cache
            // pre-flight & allocatefree space requirements
            if let Some(dict_entry) = self.cache[basis_index].dicts.get(dict) {
                hw.ensure_fast_space_alloc(dict_entry.alloc_estimate_small(), &self.cache);
                // large pools don't have caching implemented, so we don't have to check for free space for them
            }
            // refetch the basis here to avoid the re-borrow problem, now that all the potential dict cache mutations are done
            let basis = &mut self.cache[basis_index];

            // bumping this every key update affects performance *a lot* -- don't think this is worth it.
            // the bases should only "age" when dicts or keys are modified, not when any data in it is updated for any reason.
            // basis.age = basis.age.saturating_add(1);
            // basis.clean = false;

            // now do the sync
            if let Some(dict_entry) = basis.dicts.get_mut(dict) {
                let updated_ptr = dict_entry.key_update(hw, &mut basis.v2p_map,
                    &basis.cipher, key, data,
                    offset.unwrap_or(0),
                    alloc_hint,
                    truncate,
                    basis.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START))
                )?;
                basis.large_alloc_ptr = Some(updated_ptr);

                if !dict_entry.sync_small_pool(hw, &mut basis.v2p_map, &basis.cipher) {
                    return Err(Error::new(ErrorKind::OutOfMemory, "Ran out of memory syncing small pool"));
                }
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

    pub(crate) fn key_attributes(&mut self, hw: &mut PddbOs, dict: &str, key: &str, basis_name: Option<&str>) -> Result<KeyAttributes> {
        if basis_name.is_none() {
            for basis in self.cache.iter_mut().rev() {
                if !basis.ensure_dict_in_cache(hw, dict) {
                    continue;
                } else {
                    let dict_entry = basis.dicts.get_mut(dict).expect("Entry was assured, but not there!");
                    if dict_entry.ensure_key_entry(hw, &mut basis.v2p_map, &basis.cipher, key) {
                        let kcache = match dict_entry.keys.get_mut(key) {
                            Some(kc) => kc,
                            None => continue,
                        };
                        return Ok(KeyAttributes {
                            len: kcache.len as usize,
                            reserved: kcache.reserved as usize,
                            age: kcache.age as usize,
                            dict: dict.to_string(),
                            basis: (&basis.name).to_string(),
                            flags: kcache.flags,
                            index: kcache.descriptor_index,
                        })
                    } else {
                        // this is not a hard error, it just means that the key wasn't in this basis.
                        // that's alright, it could be in one of the other ones!
                        continue;
                    }
                }
            }
            Err(Error::new(ErrorKind::NotFound, "key not found"))
        } else {
            if let Some(basis_index) = self.select_basis(basis_name) {
                let basis = &mut self.cache[basis_index];
                if !basis.ensure_dict_in_cache(hw, dict) {
                    return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
                } else {
                    let dict_entry = basis.dicts.get_mut(dict).expect("Entry was assured, but not there!");
                    if dict_entry.ensure_key_entry(hw, &mut basis.v2p_map, &basis.cipher, key) {
                        let kcache = dict_entry.keys.get_mut(key).expect("Entry was assured, but then not there!");
                        Ok(KeyAttributes {
                            len: kcache.len as usize,
                            reserved: kcache.reserved as usize,
                            age: kcache.age as usize,
                            dict: dict.to_string(),
                            basis: (&basis.name).to_string(),
                            flags: kcache.flags,
                            index: kcache.descriptor_index,
                        })
                    } else {
                        return Err(Error::new(ErrorKind::NotFound, "key not found"));
                    }
                }
            } else {
                Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
            }
        }
    }

    pub(crate) fn dict_attributes(&mut self, hw: &mut PddbOs, dict: &str, basis_name: Option<&str>) -> Result<DictAttributes> {
        if basis_name.is_none() {
            for basis in self.cache.iter_mut().rev() {
                if !basis.ensure_dict_in_cache(hw, dict) {
                    continue;
                } else {
                    let dict_entry = basis.dicts.get_mut(dict).expect("Entry was assured, but not there!");
                    return Ok(dict_entry.to_dict_attributes(dict, &basis.name));
                }
            }
            return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
        } else {
            if let Some(basis_index) = self.select_basis(basis_name) {
                let basis = &mut self.cache[basis_index];
                if !basis.ensure_dict_in_cache(hw, dict) {
                    return Err(Error::new(ErrorKind::NotFound, "dictionary not found"));
                } else {
                    let dict_entry = basis.dicts.get_mut(dict).expect("Entry was assured, but not there!");
                    Ok(dict_entry.to_dict_attributes(dict, &basis.name))
                }
            } else {
                Err(Error::new(ErrorKind::NotFound, "Requested basis not found, or PDDB not mounted."))
            }
        }
    }

    /// this largely copies code from the pddb_mount() routine. Perhaps this should be modified a little bit to
    /// re-use that code. However, there are material differences in how the passwords are handled between
    /// these two methods, so the API calls are different. pddb_mount mounts the system basis with the intention
    /// of making it persistent, and assuming you're coming up from a blank slate. This routine makes no such
    /// assumptions and allows one to specify a persistence.
    pub(crate) fn basis_unlock(&mut self, hw: &mut PddbOs, name: &str, password: &str,
    policy: BasisRetentionPolicy) -> Option<BasisCacheEntry> {
        let basis_key =  hw.basis_derive_key(name, password);
        if let Some(basis_map) = hw.pt_scan_key(&basis_key.pt, &basis_key.data, name) {
            let aad = hw.data_aad(name);
            if let Some(root_page) = basis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                let vpage = match hw.data_decrypt_page_with_commit(&basis_key.data, &aad, root_page) {
                    Some(data) => data,
                    None => {log::error!("Could not find basis {} root", name); return None;},
                };
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
                let basis_name = std::str::from_utf8(&basis_root.name.data[..basis_root.name.len as usize]).expect("basis name is not valid utf-8");
                if basis_name != name {
                    log::error!("PDDB mount requested {}, but got {}; aborting.", name, basis_name);
                    return None;
                }
                log::debug!("Basis {} record found, generating cache entry", name);
                BasisCacheEntry::mount(hw, &basis_name, &basis_key, false, policy)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// this creates a basis entry in the PDDB, which should then be "mounted" to activate it.
    /// similar to how you might use fdisk to create a partition, but you still must call mount to access it.
    pub(crate) fn basis_create(&mut self, hw: &mut PddbOs, name: &str, password: &str) -> Result<()> {
        if !hw.fast_space_ensure_next_log() {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to create basis"));
        };
        if !hw.ensure_fast_space_alloc(1, &self.cache) {
            return Err(Error::new(ErrorKind::OutOfMemory, "No free space to create basis"));
        };

        let basis_key =  hw.basis_derive_key(name, password);

        if let Some(_basis_map) = hw.pt_scan_key(&basis_key.pt, &basis_key.data, name) {
            return Err(Error::new(ErrorKind::AlreadyExists, "Basis already exists"));
        }

        let mut basis_v2p_map = HashMap::<VirtAddr, PhysPage>::new();
        let basis_root = BasisRoot {
            magic: PDDB_MAGIC,
            version: PDDB_VERSION,
            name: BasisRootName::try_from_str(name).unwrap(),
            age: 0,
            num_dictionaries: 0
        };
        // allocate one page for the basis root
        if let Some(alloc) = hw.try_fast_space_alloc() {
            let mut rpte = alloc.clone();
            rpte.set_clean(true); // it's not clean _right now_ but it will be by the time this routine is done...
            rpte.set_valid(true);
            let va = VirtAddr::new((1 * VPAGE_SIZE) as u64).unwrap(); // page 1 is where the root goes, by definition
            log::debug!("adding basis {}: va {:x?} with pte {:?}", name, va, rpte);
            basis_v2p_map.insert(va, rpte);
        } else {
            return Err(Error::new(ErrorKind::Other, "Space reservation failed"));
        }
        let aad = basis_root.aad(hw.dna());
        let pp = basis_v2p_map.get(&VirtAddr::new(1 * VPAGE_SIZE as u64).unwrap()).unwrap();
        assert!(pp.valid(), "v2p returned an invalid page");
        let journal_bytes = (hw.trng_u32() % JOURNAL_RAND_RANGE).to_le_bytes();
        let slice_iter =
            journal_bytes.iter() // journal rev
            .chain(basis_root.as_ref().iter());
        let mut block = [0 as u8; KCOM_CT_LEN];
        for (&src, dst) in slice_iter.zip(block.iter_mut()) {
            *dst = src;
        }
        hw.data_encrypt_and_patch_page_with_commit(&basis_key.data, &aad, &mut block, &pp);

        let cipher =  Aes256::new(GenericArray::from_slice(&basis_key.pt));
        for (&virt, phys) in basis_v2p_map.iter_mut() {
            hw.pt_patch_mapping(virt, phys.page_number(), &cipher);
            // mark the entry as clean, as it has been sync'd to disk
            phys.set_clean(true);
        }

        Ok(())
    }

    pub(crate) fn basis_list(&self) -> Vec<String> {
        let mut ret = Vec::new();
        for bcache in &self.cache {
            ret.push(bcache.name.clone());
        }
        ret
    }
    pub(crate) fn basis_latest(&self) -> Option<&str> {
        if let Some(basis_index) = self.select_basis(None) {
            let basis = &self.cache[basis_index];
            Some(&basis.name)
        } else {
            None
        }
    }

    pub(crate) fn basis_add(&mut self, basis: BasisCacheEntry) {
        self.cache.push(basis);
    }

    pub(crate) fn basis_unmount(&mut self, hw: &mut PddbOs, basis_name: &str) -> Result<()> {
        if let Some(basis_index) = self.select_basis(Some(basis_name)) {
            let basis = &mut self.cache[basis_index];
            basis.sync(hw)?;
            self.cache.retain(|x| x.name != basis_name);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Basis not found"))
        }
    }

    /// note: you can "delete" a basis simply by forgetting its password, but this is more thorough.
    /// there might also need to be a variant to make which is a "change my password" function, but that is actually
    /// surprisingly hard.
    pub(crate) fn basis_delete(&mut self, hw: &mut PddbOs, basis_name: &str) -> Result<()> {
        if let Some(basis_index) = self.select_basis(Some(basis_name)) {
            let basis = &mut self.cache[basis_index];
            let mut temp: [u8; PAGE_SIZE] = [0; PAGE_SIZE];
            for page in basis.v2p_map.values_mut() {
                hw.trng_slice(&mut temp);
                hw.patch_data(&temp, page.page_number() * PAGE_SIZE as u32);
                log::trace!("fast_space_free basis delete {} before", page.journal());
                hw.fast_space_free(page);
            }
            basis.pt_sync(hw);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Basis not found"))
        }
    }

    pub(crate) fn sync(&mut self, hw: &mut PddbOs, basis_name: Option<&str>) -> Result<()> {
        if basis_name.is_some() {
            if let Some(basis_index) = self.select_basis(basis_name) {
                self.cache[basis_index].sync(hw)?
            }
        } else {
            for basis in self.cache.iter_mut() {
                log::debug!("syncing {}", basis.name);
                basis.sync(hw)?;
            }
        }
        Ok(())
    }

    pub(crate) fn suspend(&mut self, hw: &mut PddbOs) {
        self.sync(hw, None).expect("couldn't sync on suspend");
        let mut lock_list = Vec::<String>::new();
        for basis in self.cache.iter_mut() {
            match basis.policy {
                BasisRetentionPolicy::Persist => (),
                BasisRetentionPolicy::ClearAfterSleeps(sleeps) => {
                    basis.policy_state += 1;
                    if basis.policy_state >= sleeps {
                        lock_list.push(basis.name.clone());
                    }
                }
                /*
                BasisRetentionPolicy::TimeOutSecs(secs) => {
                    if secs < basis.policy_state {
                        lock_list.push(basis.name.clone());
                    }
                }*/
            }
        }
        for basis in lock_list {
            log::info!("unmounting basis on sleep: {}", &basis);
            self.basis_unmount(hw, &basis).ok();
        }
    }

    /// returns a relative measure of cache size. It is not absolutely accurate as
    /// overhead is not accounted for, but the actual data cached is relatively correct.
    pub(crate) fn cache_size(&mut self) -> usize {
        let mut total_size = 0;
        for basis in self.cache.iter() {
            for dict in basis.dicts.values() {
                for key in dict.keys.values() {
                    total_size += key.size();
                }
            }
        }
        total_size
    }
    /// attempts to prune `target_bytes` out of the cached data set
    pub(crate) fn cache_prune(&mut self, hw: &mut PddbOs, target_bytes: usize) -> usize {
        let mut pruned = 0;
        // this does it a "dumb" way, but at least it's sort of obvious how it works
        // 0. sync the basis and dictionaries to disk, so that removing cache entries are guaranteed not to be problematic.
        // 1. iterate through all the known keys, creating a sorted heap of fully-specified keys (basis/dict/key)
        //    sorted by access time
        // 2. evict keys with the oldest access time, until target_bytes is hit.
        self.sync(hw, None).unwrap();

        let mut candidates = BinaryHeap::new();
        for basis in self.cache.iter() {
            for (dict_name, dict) in basis.dicts.iter() {
                for (key_name, key) in dict.keys.iter() {
                    candidates.push(Reverse(
                        KeyAge {
                            atime: key.atime(),
                            _size: key.size(),
                            key: key_name.to_string(),
                            dict: dict_name.to_string(),
                            basis: basis.name.to_string(),
                        }
                    ));
                }
            }
        }

        loop {
            if let Some(Reverse(ka)) = candidates.pop() {
                for basis in self.cache.iter_mut() {
                    if basis.name == ka.basis {
                        match basis.dicts.get_mut(&ka.dict) {
                            Some(dentry) => {
                                let freed = dentry.evict_keycache_entry(&ka.key);
                                log::debug!("pruned {} bytes from atime {} / total {}", freed, ka.atime, pruned);
                                pruned += freed;
                                if pruned >= target_bytes {
                                    return pruned;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                break;
            }
        }
        pruned
    }
}
// Revise this to use references instead of allocations once we've refactored the interior mutability
// issues with the PDDB.
struct KeyAge {
    atime: u64,
    // currently unused but collected in case we want to use it in the future
    _size: usize,
    key: String,
    dict: String,
    basis: String,
}
impl Ord for KeyAge {
    fn cmp(&self, other: &Self) -> Ordering {
        self.atime.cmp(&other.atime)
    }
}
impl PartialOrd for KeyAge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for KeyAge {
    fn eq(&self, other: &Self) -> bool {
        self.atime == other.atime
    }
}
impl Eq for KeyAge {}

/// This is the RAM cached copy of a basis as maintained in the PDDB.
pub(crate) struct BasisCacheEntry {
    /// the name of this basis
    pub name: String,
    /// set if synched to what's on disk
    pub clean: bool,
    /// last sync time, in systicks, if any. Updated by any sync of basis, dict, or pt.
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
    /// derived cipher for encrypting PTEs -- cache it, so we can save the time cost of constructing the cipher key schedule
    pub cipher_ecb: Aes256,
    /// raw AES page table key -- needed because we have to do a low-level PT scan to generate FSCB, and sometimes the key comes from
    /// a copy cached here, or from one derived solely for the FSCB scan. There is no way to copy an Aes256 record, so, we include
    /// the raw key because we can copy that. :P so much for semantics.
    pub pt_key: GenericArray<u8, cipher::consts::U32>,
    /// raw AES data key -- needed because we have to use this to derive commitment keys for the basis root record, to work around AES-GCM-SIV salamanders
    pub key: GenericArray<u8, cipher::consts::U32>,
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
    /// retiention policy
    pub policy: BasisRetentionPolicy,
    // rention state
    pub policy_state: u32,
}
impl BasisCacheEntry {
    /// given a pointer to the hardware, name of the basis, and its cryptographic key, try to derive
    /// the basis. If `lazy` is true, it stops with the minimal amount of effort to respond to a query.
    /// If it `lazy` is false, it will populate the dictionary cache and key cache entries, as well as
    /// discover the location of the `large_alloc_ptr`.
    pub(crate) fn mount(hw: &mut PddbOs, name: &str,  key: &BasisKeys, lazy: bool, policy: BasisRetentionPolicy) -> Option<BasisCacheEntry> {
        if let Some(basis_map) = hw.pt_scan_key(&key.pt, &key.data, name) {
            let cipher = Aes256GcmSiv::new(&key.data.into());
            let aad = hw.data_aad(name);
            // get the first page, where the basis root is guaranteed to be
            if let Some(root_page) = basis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                let vpage = match hw.data_decrypt_page_with_commit(&key.data, &aad, root_page) {
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
                let basis_name = std::str::from_utf8(&basis_root.name.data[..basis_root.name.len as usize]).expect("basis name is not valid utf-8");
                if basis_name != String::from(name) {
                    log::error!("Discovered basis name does not match the requested name: {}; aborting mount operation.", basis_name);
                    return None;
                }
                let mut bcache = BasisCacheEntry {
                    name: basis_name.to_string(),
                    clean: true,
                    last_sync: Some(hw.timestamp_now()),
                    num_dicts: basis_root.num_dictionaries,
                    dicts: HashMap::<String, DictCacheEntry>::new(),
                    cipher,
                    cipher_ecb: Aes256::new(GenericArray::from_slice(&key.pt)),
                    pt_key: GenericArray::clone_from_slice(&key.pt),
                    key: GenericArray::clone_from_slice(&key.data),
                    aad,
                    age: basis_root.age,
                    free_dict_offset: None,
                    v2p_map: basis_map,
                    journal: u32::from_le_bytes(vpage[..size_of::<JournalType>()].try_into().unwrap()),
                    large_alloc_ptr: None,
                    policy,
                    policy_state: policy.derive_init_state(),
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

    /// do a deep scan of all the dictionaries and keys and attempt to populate all the caches
    pub(crate) fn populate_caches(&mut self, hw: &mut PddbOs) {
        // count number of valid dictionaries
        let mut num_valid = 0;
        for dict in self.dicts.values() {
            if dict.flags.valid() {
                num_valid += 1;
            }
        }
        // note to self: this should be true, because even if we didn't read all the dictionaries in from the basis,
        // and then say, deleted a bunch of dictionaries and then added a bunch more, the num_dicts field should
        // move up and down with the additions/deletions, and thus should relatively be larger than or equal to the num_valid
        // at all times. In other words, the difference of self.num_dicts vs num_valid should be precisely the number
        // of dictionary entries on disk that we haven't loaded into the cache.
        assert!(num_valid <= self.num_dicts, "Inconsistency in number of dictionaries in the basis");
        if num_valid == self.num_dicts { // no need to scan the index, just refresh the dictionaries themselves
            let mut largest_extent = 0;
            for dict in self.dicts.values_mut() {
                if dict.flags.valid() {
                    let extent = dict.fill(hw, &mut self.v2p_map, &self.cipher).get();
                    if extent > largest_extent {
                        largest_extent = extent;
                    }
                }
            }
            self.large_pool_update(largest_extent);
        } else { // scan the full index
            let mut try_entry = 1;
            let mut dict_count = 0;
            while try_entry <= DICT_MAXCOUNT && dict_count < self.num_dicts {
                let dict_vaddr = VirtAddr::new(try_entry as u64 * DICT_VSIZE).unwrap();
                if let Some(pp) = self.v2p_map.get(&dict_vaddr) {
                    assert!(pp.valid(), "v2p returned an invalid page");
                    if let Some(dict) = self.dict_decrypt(hw, &pp) {
                        if dict.flags.valid() {
                            let dict_name = std::str::from_utf8(&dict.name.data[..dict.name.len as usize]).expect("dict name is not valid utf-8").to_string();
                            let dict_present_and_valid = if let Some(d) = self.dicts.get(&dict_name) {
                                d.flags.valid()
                            } else {
                                false
                            };
                            if !dict_present_and_valid {
                                let mut dcache = DictCacheEntry::new(dict, try_entry, &self.aad);
                                let max_large_alloc = dcache.fill(hw, &self.v2p_map, &self.cipher);
                                self.dicts.insert(dict_name.to_string(), dcache);
                                self.large_pool_update(max_large_alloc.get());
                            } else {
                                let dcache = self.dicts.get_mut(&dict_name).expect("dict should be present, as we checked for it already...");
                                let extent = dcache.fill(hw, &mut self.v2p_map, &self.cipher).get();
                                self.large_pool_update(extent);
                            }
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
    }

    /// If `paranoid` is true, it recurses through each key and replaces its data with random junk.
    /// Otherwise, it does a "shallow" delete and just removes the directory entry, which is much
    /// more performant. Note that the intended "fast" way to secure-erase data is to store sensitive
    /// data in its own Basis, and then remove the Basis itself. This is much faster than picking
    /// through compounded data and re-writing partias sectors, and because of this, initially,
    /// the `paranoid` erase is `unimplemented`.
    pub(crate) fn dict_delete(&mut self, hw: &mut PddbOs, name: &str, paranoid: bool) -> Result<()> {
        if self.ensure_dict_in_cache(hw, name) {
            let dcache = self.dicts.get_mut(name).expect("entry was ensured, but somehow missing");
            log::trace!("dcache {} index {}, key_count {}", name, dcache.index, dcache.key_count);
            // ensure all the keys are in RAM
            dcache.fill(hw, &self.v2p_map, &self.cipher);

            // allocate a copy of the key list, to avoid interior mutability problems with the next remove step
            let mut key_list = Vec::<String>::new();
            for (key, entry) in dcache.keys.iter() {
                if entry.flags.valid() {
                    key_list.push(key.to_string());
                }
            }
            for key in key_list {
                log::debug!("removing {}:{}", name, key);
                // this will wipe any large pools if paranoid is set
                dcache.key_remove(hw, &mut self.v2p_map, &self.cipher, &key, paranoid);
            }
            // wipe & de-allocate any small pages
            for index in 0..dcache.small_pool.len() {
                let pool_vaddr = VirtAddr::new(small_storage_base_vaddr_from_indices(dcache.index, index)).unwrap();
                if let Some(pp) = self.v2p_map.get_mut(&pool_vaddr) {
                    assert!(pp.valid(), "v2p returned an invalid page");
                    { // always nuke old data
                        let mut random = [0u8; PAGE_SIZE];
                        hw.trng_slice(&mut random);
                        hw.patch_data(&random, pp.page_number() * PAGE_SIZE as u32);
                    }
                    log::trace!("fast_space_free small page delete {} before", pp.journal());
                    hw.fast_space_free(pp);
                    assert!(pp.valid() == false, "pp is still marked as valid!");
                }
            }
            /* for pp in self.v2p_map.values() {
                log::info!("v2p entry: {:x?}", pp);
            } */
            // erase the entire dictionary + key allocation area by writing over with random data. It's important to
            // do a comprehensive erase, because if the dictionary slot is re-used, the previously allocated key entries
            // decrypt correctly, are interpreted as valid keys, and thus cause consistency errors.
            let key_pages = 1 + (dcache.last_disk_key_index + 1) as usize / DK_PER_VPAGE;
            for page in 0..key_pages {
                // note: check this code against dict_indices_to_vaddr() -- it's recoded here because we go by page, not by index, but this needs to be consistent with that function
                let dk_vaddr = VirtAddr::new(dcache.index.get() as u64 * DICT_VSIZE as u64 + page as u64 * VPAGE_SIZE as u64).unwrap();
                if let Some(pp) = self.v2p_map.get_mut(&dk_vaddr) {
                    assert!(pp.valid(), "v2p returned an invalid page");
                    log::info!("erasing dk page 0x{:x}/0x{:x}", dk_vaddr, pp.page_number() as usize * PAGE_SIZE);
                    let mut random = [0u8; PAGE_SIZE];
                    hw.trng_slice(&mut random);
                    hw.patch_data(&random, pp.page_number() * PAGE_SIZE as u32);
                    log::trace!("fast_space_free dict_delete {} before", pp.journal());
                    hw.fast_space_free(pp);
                    assert!(pp.valid() == false, "pp is still marked as valid!");
                } else {
                    log::warn!("Inconsistent internal state: requested dictionary didn't have a mapping in the page table.");
                }
            }

            // mark data for re-use
            self.free_dict_offset = Some(dcache.index.get());
            dcache.flags.set_valid(false); // this shouldn't be necessary because we're removing the entry, but, it's "correct"
            self.num_dicts -= 1;
            // remove the cache entry
            self.dicts.remove(name);

            Ok(())
        } else {
            Err(Error::new(ErrorKind::NotFound, "Dictionary not found"))
        }
    }

    /// Called to compute the next free index. Normally this will get filled during a disk search for
    /// cache entries, but it could be None if a new dict entry was allocated and no other entries were loaded.
    pub(crate) fn dict_get_free_offset(&mut self, hw: &mut PddbOs) -> u32 {
        if let Some(offset) = self.free_dict_offset.take() {
            return offset;
        } else {
            let mut try_entry = 1;
            while try_entry <= DICT_MAXCOUNT {
                let dict_vaddr = VirtAddr::new(try_entry as u64 * DICT_VSIZE).unwrap();
                if let Some(pp) = self.v2p_map.get(&dict_vaddr) {
                    assert!(pp.valid(), "v2p returned an invalid page");
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
                assert!(pp.valid(), "v2p returned an invalid page");
                if let Some(dict) = self.dict_decrypt(hw, &pp) {
                    let dict_name = std::str::from_utf8(&dict.name.data[..dict.name.len as usize]).expect("dict name is not valid utf-8");
                    if (dict_name == name) && dict.flags.valid() {
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
        self.last_sync = Some(hw.timestamp_now());
        let mut kill_list = Vec::<VirtAddr>::new();
        // iterate once to delete old entries
        for (&virt, phys) in self.v2p_map.iter_mut() {
            if !phys.valid() {
                // erase the entry
                log::debug!("deleting pte va: {:x?} pa: {:x?}", virt, phys);
                kill_list.push(virt);
                hw.pt_erase(phys.page_number());
            }
        }
        // have to do this in a second phase due to interior mutability problems doing it inside the first iterator
        for kill in kill_list {
            if self.v2p_map.remove(&kill).is_none() {
                log::warn!("went to remove PTE from v2p map but it wasn't there: {:x}", kill);
            }
        }
        // iterate a second time to write new entries -- can't do this in a single loop because the
        // order of visitation is arbitrary and we can delete after writing an entry if we put these
        // two in the same loop!
        for (&virt, phys) in self.v2p_map.iter_mut() {
            if !phys.clean() {
                log::debug!("syncing dirty pte va: {:x?} pa: {:x?}", virt, phys);
                hw.pt_patch_mapping(virt, phys.page_number(), &self.cipher_ecb);
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
        self.last_sync = Some(hw.timestamp_now());
        if let Some(dict) = self.dicts.get_mut(&String::from(name)) {
            let dict_offset = VirtAddr::new(dict.index.get() as u64 * DICT_VSIZE).unwrap();
            if !dict.clean {
                let dict_name = DictName::try_from_str(name).or(Err(Error::new(ErrorKind::InvalidInput, "dictionary name invalid: invalid utf-8 or length")))?;
                let dict_disk = Dictionary {
                    flags: dict.flags,
                    age: dict.age,
                    num_keys: dict.key_count,
                    free_key_index: dict.last_disk_key_index,
                    name: dict_name,
                };
                log::debug!("syncing dict {} with {} keys", name, dict.key_count);
                // log::info!("raw: {:x?}", dict_disk.deref());
                // observation: all keys to be flushed to disk will be in the KeyCacheEntry. Some may be clean,
                // but definitely all the dirty ones are in there (if they aren't, where else would they be??)

                // this is the virtual page within the dictionary region that we're currently serializing
                let mut vpage_num = 0;
                let mut loopcheck= 0;
                let mut sync_count = 0;
                loop {
                    loopcheck += 1;
                    if loopcheck == 256 {
                        log::warn!("potential infinite loop detected sync dict {}", name);
                    }
                    // 1. resolve the virtual address to a target page
                    let cur_vpage = VirtAddr::new(dict_offset.get() + (vpage_num as u64 * VPAGE_SIZE as u64)).unwrap();
                    let pp = self.v2p_map.entry(cur_vpage).or_insert_with(|| {
                        let mut ap = hw.try_fast_space_alloc().expect("FastSpace empty");
                        ap.set_valid(true);
                        ap
                    });
                    //if name.contains("dict2") {
                    //    log::debug!("TRACING: {}/{} | {:x?}", name, vpage_num, pp);
                    //}
                    assert!(pp.valid(), "v2p returned an invalid page");

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
                        /*if key_name.contains("dict2|key6|len2347") {
                            log::warn!("TRACING: {}", key_name);
                            log::warn!("start: {:x}, clean: {}, flags: {:?}, desciptor: {:?}",
                                key.start, key.clean, key.flags, key.descriptor_index,
                            );
                        }*/
                        if !key.clean && key.flags.valid() {
                            if key.descriptor_vaddr(dict_offset) >= cur_vpage &&
                            key.descriptor_vaddr(dict_offset) < next_vpage {
                                log::debug!("merging in key {}", key_name);
                                // key is within the current page, add it to the target list
                                let mut dk_entry = DictKeyEntry::default();
                                let kn = KeyName::try_from_str(key_name).or(Err(Error::new(ErrorKind::InvalidInput, "key name invalid: invalid utf-8 or length")))?;
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
                                dk_vpage.elements[key.descriptor_index.get() as usize % DK_PER_VPAGE] = Some(dk_entry);
                                key.clean = true;
                            } else {
                                log::debug!("proposed key fell outside of our vpage: {} vpage{:x}/vaddr{:x}", key_name, cur_vpage.get(), key.descriptor_vaddr(dict_offset));
                            }
                        }
                    }

                    // 3. merge the vpage modifications into the disk
                    let mut page = if let Some(data) = hw.data_decrypt_page(&self.cipher, &self.aad, &pp) {
                        log::trace!("merging dictionary data into existing page");
                        data
                    } else {
                        log::trace!("existing data invalid, creating a new page");
                        // the existing data was invalid (this happens e.g. on the first time a dict is created). Just overwrite the whole page.
                        let mut d = vec![0u8; VPAGE_SIZE + size_of::<JournalType>()];
                        for (&src, dst) in (hw.trng_u32() % JOURNAL_RAND_RANGE).to_le_bytes().iter().zip(d[..size_of::<JournalType>()].iter_mut()) {
                            *dst = src;
                        }
                        d
                    };
                    for (index, stride) in page[size_of::<JournalType>()..].chunks_mut(DK_STRIDE).enumerate() {
                        if let Some(elem) = dk_vpage.elements[index] {
                            for (&src, dst) in elem.data.iter().zip(stride.iter_mut()) {
                                *dst = src;
                            }
                        }
                    }
                    // generate nonce and write out
                    log::debug!("patching pp {:x?} with aad {:x?}, data {:x?}", pp, self.aad, &page[..256]);
                    hw.data_encrypt_and_patch_page(&self.cipher, &self.aad, &mut page, &pp);

                    // 4. Check for dirty keys, if there are still some, update vpage_num to target them; otherwise
                    // exit the loop
                    let mut found_next = false;
                    for key in dict.keys.values() {
                        if !key.clean && key.flags.valid() {
                            found_next = true;
                            // note: we don't care *which* vpage we do next -- so we just break after finding the first one
                            vpage_num = key.descriptor_vpage_num();
                            break;
                        }
                    }
                    if !found_next {
                        log::debug!("breaking after syncing {} keys", sync_count);
                        break;
                    }
                    sync_count += 1;
                }
                log::debug!("done syncing dict");
                dict.clean = true;
            }
            Ok(())
        } else {
            log::error!("dict sync could not happen, dictionary name invalid!");
            Err(Error::new(ErrorKind::NotFound, "dict_sync called with an invalid dictionary name"))
        }
    }

    /// Runs through the dictionary listing in a basis and compacts them. Call when the
    /// the dictionary space becomes sufficiently fragmented that accesses are becoming
    /// inefficient.
    #[allow(dead_code)]
    pub(crate) fn dict_compact(&self, _basis_name: Option<&str>) -> Result<()> {
        unimplemented!();
    }

    /// Syncs *only* the basis header to disk.
    pub(crate) fn basis_sync(&mut self, hw: &mut PddbOs) {
        self.last_sync = Some(hw.timestamp_now());
        if !self.clean {
            self.age += 1;
            let basis_root = BasisRoot {
                magic: PDDB_MAGIC,
                version: PDDB_VERSION,
                name: BasisRootName::try_from_str(&self.name).unwrap(),
                age: self.age,
                num_dictionaries: self.num_dicts,
            };
            let pp = self.v2p_map.get(&VirtAddr::new(1 * VPAGE_SIZE as u64).unwrap())
                .expect("Internal consistency error: Basis exists, but its root map was not allocated!");
            assert!(pp.valid(), "basis page was invalid");
            log::debug!("{} before-sync journal: {}", self.name, self.journal);
            let journal_bytes = self.journal.to_le_bytes(); // journal gets bumped by the patching function now
            let slice_iter =
                journal_bytes.iter() // journal rev
                .chain(basis_root.as_ref().iter());
            let mut block = [0 as u8; KCOM_CT_LEN];
            for (&src, dst) in slice_iter.zip(block.iter_mut()) {
                *dst = src;
            }
            hw.data_encrypt_and_patch_page_with_commit(self.key.as_slice(), &self.aad, &mut block, &pp);
            // read back the incremented journal state
            self.journal = u32::from_le_bytes(block[..4].try_into().unwrap());
            log::debug!("{} after-sync journal: {}", self.name, self.journal);
            self.clean = true;
        }
    }

    /// This function ensures a dictionary is in the cache; if not, it will load its entry.
    pub(crate) fn ensure_dict_in_cache(&mut self, hw: &mut PddbOs, name: &str) -> bool {
        let mut dict_found = false;
        let dict_in_cache_and_valid =
            if let Some(dict) = self.dicts.get(name) {
                dict.flags.valid()
            } else {
                false
            };
        if !dict_in_cache_and_valid {
            log::debug!("dict: key not in cache {}", name);
            // if the dictionary doesn't exist in our cache it doesn't necessarily mean it
            // doesn't exist. Do a comprehensive search if our cache isn't complete.
            if let Some((index, dict_record)) = self.dict_deep_search(hw, name) {
                let dict_name = std::str::from_utf8(&dict_record.name.data[..dict_record.name.len as usize]).expect("dict name is not valid utf-8").to_string();
                let dcache = DictCacheEntry::new(dict_record, index as usize, &self.aad);
                self.dicts.insert(dict_name, dcache);
                dict_found = true;
            }
        } else {
            dict_found = true;
        }
        dict_found
    }

    pub(crate) fn sync(&mut self, hw: &mut PddbOs) -> Result<()> {
        // this is a bit awkward, but we have to make a copy of all the dictionary names
        // because otherwise we borrow self as immutable to enumerate the names, and then
        // we borrow it as mutable to do the sync. This deep-copy works around the issue.
        let mut dictnames = Vec::<String>::new();
        for (dict, entry) in self.dicts.iter() {
            if entry.flags.valid() {
                dictnames.push(dict.to_string());
            }
        }
        for dict in dictnames {
            match self.dict_sync(hw, &dict) {
                Ok(_) => {},
                Err(e) => {
                    log::error!("Error encountered syncing dict {}: {:?}", dict, e);
                    return Err(Error::new(ErrorKind::Other, e.to_string()));
                }
            }
        }
        self.basis_sync(hw);
        self.pt_sync(hw);
        Ok(())
    }

    // allocate a pointer data in the large pool, of length `amount`. "always" succeeds because...
    // there's 16 million terabytes of large pool to allocate before you run out?
    /*
    // this function is actually implemented inside the "key_update()" code - as a large key is allocated,
    // the pointer is bumped along within the update code. AFAIK, nobody else should be calling this, so, let's
    // comment it out, and if it becomes necessary for some reason let's take a good hard look at assumptions.
    // In particular, this function might become necessary if disk caching is implemented for large keys.
    pub(crate) fn large_pool_alloc(&mut self, amount: u64) -> u64 {
        let alloc_ptr = self.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START));
        self.large_alloc_ptr = Some(self.large_alloc_ptr.unwrap_or(PageAlignedVa::from(LARGE_POOL_START)) + PageAlignedVa::from(amount));
        return alloc_ptr.as_u64()
    }*/

}

// ****
// Beginning of serializers for the data structures in this file.
// ****

/// Newtype for BasisRootName so we can give it a default initializer.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub struct BasisRootName {
    pub len: u8,
    pub data: [u8; BASIS_NAME_LEN - 1],
}
impl BasisRootName {
    pub fn try_from_str(name: &str) -> Result<BasisRootName> {
        let mut alloc = [0u8; BASIS_NAME_LEN - 1];
        let bytes = name.as_bytes();
        if bytes.len() > (BASIS_NAME_LEN - 1) {
            Err(Error::new(ErrorKind::InvalidInput, "basis name is too long")) // FileNameTooLong is still nightly :-/
        } else {
            for (&src, dst) in bytes.iter().zip(alloc.iter_mut()) {
                *dst = src;
            }
            Ok(BasisRootName {
                len: bytes.len() as u8, // this as checked above to be short enough
                data: alloc,
            })
        }
    }
}
impl Default for BasisRootName {
    fn default() -> BasisRootName {
        BasisRootName{
            len: 0,
            data: [0; BASIS_NAME_LEN - 1]
        }
    }
}
