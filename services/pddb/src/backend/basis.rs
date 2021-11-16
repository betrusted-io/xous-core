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

/// This is the RAM cached copy of a basis as maintained in the PDDB.
pub(crate) struct BasisCache {
    /// the name of this basis
    pub name: String,
    /// set if synched to what's on disk
    pub clean: bool,
    /// last sync time, in systicks, if any
    pub last_sync: Option<u64>,
    /// the basis root record, as read in from disk
    pub root: BasisRoot,
    /// dictionary array.
    pub dicts: HashMap::<String, DictCache>,
    /// the cipher for the basis
    pub cipher: Aes256GcmSiv,
    /// the AAD associated with this Basis
    pub aad: Vec::<u8>,
    /// modification count
    pub age: u32,
}

/// RAM based copy of the dictionary structures on disk.
pub(crate) struct DictCache {
    /// Use this to compute the virtual address of the dictionary's location
    index: u32,
    /// A cache of the keys within the dictionary. If the key does not exist in
    /// the cache, one should consult the on-disk copy, assuming the record is clean.
    keys: HashMap::<String, KeyDescriptor>,
    /// set if synced to disk. This might not actually be used, because I think we actually
    /// want a write-through policy for the key cache.
    clean: bool,
    /// track modification count
    age: u32,
}

/// On-disk representation of the dictionary header.
#[repr(C, align(8))]
pub(crate) struct Dictionary {
    /// Reserved for flags on the record entry
    flags: u32,
    /// Access count to the dicitionary
    age: u32,
    /// Number of keys in the dictionary
    num_keys: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    name: [u8; DICT_NAME_LEN],
}

/// On-disk representation of the Key. Note that the storage on disk is mis-aligned, so
/// any deserialization must essentially come with a copy step to line up the record.
#[repr(C, align(8))]
pub(crate) struct KeyDescriptor {
    /// virtual address of the key's start
    start: u64,
    /// length of the key
    len: u64,
    /// reserved for allocation bookkeeping
    reserved: u64,
    /// Reserved for flags on the record entry
    flags: u32,
    /// Access count to the key
    age: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    name: [u8; KEY_NAME_LEN],
}

/// used to identify cached/chunked data from a key
/// maybe we should detach the cache_page because this is meant to be a key, and cache_page is the payload.
pub(crate) struct KeyCacheEntry {
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