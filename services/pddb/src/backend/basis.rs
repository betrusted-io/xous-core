use crate::api::*;
use super::*;

use core::cell::RefCell;
use std::rc::Rc;
use core::num::NonZeroU64;
use core::ops::{Deref, DerefMut};
use core::{mem, slice};
use core::mem::size_of;
use std::convert::TryInto;
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use aes_gcm_siv::aead::{Aead, Payload};
use rkyv::Aligned;
use rkyv::ser::serializers::BufferSerializer;
use std::iter::IntoIterator;
use std::collections::HashMap;
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
/// fixed offsets starting at 0xFE_0000 and repeating every 0xFE_0000 intervals. A naive linear search
/// is used to scan for dictionaries, starting at the lowest address, scanning every 0xFE_0000, until
/// the number of valid dictionaries have been found that matches the valid dictionary count prescribed
/// in the Basis root. A dictionary can be effectively deleted by just marking its descriptor as invalid.
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
/// Key data storage itself pulls from allocation pools that optimize for large (>64k), medium (~4k) and
/// small (<256 byte) data lengths, using different allocation strategies to reduce fragmentation.
///
///
/// Basis Virtual Memory Layout
///
/// |   Start Address        |                                           |
/// |------------------------|-------------------------------------------|
/// | 0x0000_0000_0000_0000  |  Invalid -- VPAGE 0 reserved for Option<> |
/// | 0x0000_0000_0000_0FE0  |  Basis root page                          |
/// | 0x0000_0000_00FE_0000  |  Dictionary[0]                            |
/// | 0x0000_0000_01FC_0000  |  Dictionary[1]                            |
/// | 0x0000_003F_8000_0000  |  Dictionary[16383]                        |
/// | 0x0000_003F_80FE_0F00  |  Unused                                   |
/// | 0x0000_0040_0000_0000  |  Small data pool start  (256GiB)          |
/// | 0x0000_0080_0000_0000  |  Medium data pool start (512GiB)          |
/// | 0x0000_0100_0000_0000  |  Large data pool start  (~16mm TiB)       |
///
/// Note that each Basis has its own memory section, and you can have "many" orthogonal Basis without
/// a collision -- the AES keyspace is 128 bits, so you have a decent chance of no collisions
/// even with a few billion Basis concurrently existing in the filesystem.
///
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

/// The chosen "stride" of a dict/key entry. Drives a lot of key parameters in the database's characteristics.
/// This is chosen such that 32 of these entries fit evenly into a VPAGE.
pub(crate) const DK_STRIDE: usize = 127;

#[derive(PartialEq, Debug, Default)]
#[repr(C, align(8))]
pub(crate) struct BasisRoot {
    // everything below here is encrypted using AES-GCM-SIV
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

pub(crate) struct BasisCache {
    /// the cache entries themselves
    cache: Vec::<BasisCacheEntry>,
}
impl BasisCache {
    pub(crate) fn new() -> Self {
        BasisCache { cache: Vec::new() }
    }
    pub(crate) fn add_basis(&mut self, basis: BasisCacheEntry) {
        self.cache.push(basis);
    }
    // placeholder reminder: deleting a basis is a bit more complicated, as it requires
    // syncing its contents.

    fn select_basis(&mut self, basis_name: Option<&str>) -> Option<&mut BasisCacheEntry> {
        if self.cache.len() == 0 {
            log::error!("Can't select basis: PDDB is not mounted");
            return None
        }
        if let Some(n) = basis_name {
            self.cache.iter_mut().filter(|bc| bc.name == n).next()
        } else {
            self.cache.last_mut()
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
        if let Some(basis) = self.select_basis(basis_name) {
            basis.age = basis.age.saturating_add(1);
            if basis.dicts.get(&String::from(name)).is_some() {
                // quick exit if we see the dictionary is hot in the cache
                return Err(Error::new(ErrorKind::AlreadyExists, "Dictionary already exists"));
            } else {
                // if the dictionary doesn't exist in our cache it doesn't necessarily mean it
                // doesn't exist. Do a comprehensive search if our cache isn't complete.
                if basis.root.num_dictionaries as usize != basis.dicts.len() {
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

            // create the cache entry
            let mut dict_name = [0u8; DICT_NAME_LEN];
            for (src, dst) in name.bytes().into_iter().zip(dict_name.iter_mut()) {
                *dst = src;
            }
            let dict_cache = DictCacheEntry {
                index: dict_index,
                keys: HashMap::<String, KeyCacheEntry>::new(),
                clean: false,
                age: 0,
                flags: 0,
                key_count: 0,
            };
            basis.dicts.insert(String::from(name), dict_cache);
            // encrypt and write the dict entry to disk
            basis.dict_sync(hw, name)?;
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
    /// the basis root record, as read in from disk
    pub root: BasisRoot,
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
}
impl BasisCacheEntry {
    /*
    /// If `paranoid` is true, it recurses through each key and replaces its data with random junk.
    /// Otherwise, it does a "shallow" delete and just removes the directory entry, which is much
    /// more performant. Note that the intended "fast" way to secure-erase data is to store sensitive
    /// data in its own Basis, and then remove the Basis itself. This is much faster than picking
    /// through compounded data and re-writing partias sectors, and because of this, initially,
    /// the `paranoid` erase is `unimplemented`.
    pub(crate) fn dict_delete(&self, name: &str, basis_name: Option<&str>, paranoid: bool) -> Result<()> {
        if self.basis_cache.len() == 0 {
            return Err(Error::new(ErrorKind::NotConnected, "PDDB is not mounted"));
        }
        let maybe_basis = if let Some(n) = basis_name {
            self.basis_cache.iter_mut().filter(|&bc| bc.name == n).next()
        } else {
            self.basis_cache.last_mut()
        };
        if let Some(basis) = maybe_basis {
            if let Some((index, _dict)) = self.dict_deep_search(&mut basis, name) {
                let map = self.v2p_map.get_mut(&basis.name).expect("No v2p map despite extant BasisCacheEntry record. Shouldn't be possible...");
                if !paranoid {
                    // erase the header by writing over with random data. This makes the dictionary unsearchable, but if you
                    // have the key, you can of course do a hard-scan and try partially re-assemble the dictionary.
                    if let Some(pp) = map.get(&VirtAddr::new(index as u64 * DICT_VSIZE as u64).unwrap()) {
                        let mut random = [0u8; PAGE_SIZE];
                        self.entropy.borrow_mut().get_slice(&mut random);
                        self.patch_data(&random, pp.page_number() * PAGE_SIZE as u32);
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
        } else {
            Err(Error::new(ErrorKind::NotFound, "Requested basis not found"))
        }
    }
*/

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
        while try_entry <= DICT_MAXCOUNT && dict_count < self.root.num_dictionaries {
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
            let mut dict = Dictionary {
                flags: 0, age: 0, num_keys: 0, name: [0; DICT_NAME_LEN]
            };
            for (&src, dst) in data[size_of::<JournalType>()..].iter().zip(dict.deref_mut().iter_mut()) {
                *dst = src;
            }
            Some(dict)
        } else {
            None
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
                        data
                    } else {
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
                    // bump the journal rev. This means that revs start at "1", because the empty data array as passed has a 0 in it by default.
                    let newrev = JournalType::from_le_bytes(page[..size_of::<JournalType>()].try_into().unwrap()).saturating_add(1);
                    for (&src, dst) in newrev.to_le_bytes().iter().zip(page[..size_of::<JournalType>()].iter_mut()) {
                        *dst = src;
                    }
                    // generate nonce and write out
                    let nonce = hw.nonce_gen();
                    let payload = Payload {
                        msg: &page,
                        aad: &self.aad,
                    };
                    let ciphertext = self.cipher.encrypt(&nonce, payload).expect("failed to encrypt DictKeys");
                    hw.patch_data(&ciphertext, pp.page_number() * PAGE_SIZE as u32);

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
    /// set if synced to disk. should be cleared if the dict is modified, and/or if a subordinate key descriptor is modified.
    pub(crate) clean: bool,
    /// track modification count
    pub(crate) age: u32,
    /// copy of the flags entry on the Dict on-disk
    pub(crate) flags: u32,
}

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

/// On-disk representation of the Key. Note that the storage on disk is mis-aligned, so
/// any deserialization must essentially come with a copy step to line up the record.
#[repr(C, align(8))]
pub(crate) struct KeyDescriptor {
    /// virtual address of the key's start
    start: u64,
    /// length of the key's stored data
    len: u64,
    /// amount of space reserved for the key. Must be >= len.
    reserved: u64,
    /// Reserved for flags on the record entry
    flags: u32,
    /// Access count to the key
    age: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    name: [u8; KEY_NAME_LEN],
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

pub(crate) struct KeyCacheEntry {
    pub(crate) start: u64,
    pub(crate) len: u64,
    pub(crate) reserved: u64,
    pub(crate) flags: u32,
    pub(crate) age: u32,
    /// the current on-disk index of the KeyCacheEntry item, enumerated as "0" being the first DictKeyEntry
    /// slot past the first record which is the descriptor of the dictionary.
    pub(crate) descriptor_index: u32,
    /// indicates if it's clean
    pub(crate) clean: bool,
}
impl KeyCacheEntry {
    /// Given a base offset of the dictionary containing the key, compute the starting VirtAddr of the key itself.
    pub(crate) fn descriptor_vaddr(&self, dict_offset: VirtAddr) -> VirtAddr {
        VirtAddr::new(dict_offset.get() + ((self.descriptor_index as u64 + 1) * DK_STRIDE as u64)).unwrap()
    }
    /// Computes the modular position of the KeyDescriptor within a vpage.
    pub(crate) fn descriptor_modulus(&self) -> usize {
        (self.descriptor_index as usize + 1) % (VPAGE_SIZE / DK_STRIDE)
    }
    /// Computes the vpage offset as measured from the start of the dictionary storage region
    pub(crate) fn descriptor_vpage_num(&self) -> usize {
        (self.descriptor_index as usize + 1) / (VPAGE_SIZE / DK_STRIDE)
    }
}

/// used to identify cached/chunked data from a key
/// maybe we should detach the cache_page because this is meant to be a key, and cache_page is the payload.
pub(crate) struct KeyCacheEntryEntry {
    basis: String,
    dict: String,
    key: KeyDescriptor,
    /// offset of the cached data relative to the KeyDescriptor's start
    offset: u64,
    /// the actual cached data
    cache_page: [u8; VPAGE_SIZE],
    /// true if clean and synced with the disk
    clean: bool,
    /// general flags, tbd
    flags: u8,
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

// ****
// Beginning of serializers for the data structures in this file.
// ****

/// Newtype for BasisRootName so we can give it a default initializer.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub struct BasisRootName(pub [u8; BASIS_NAME_LEN]);
impl Default for BasisRootName {
    fn default() -> BasisRootName {
        BasisRootName([0; BASIS_NAME_LEN])
    }
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