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


pub type BasisRootName = [u8; PDDB_MAX_BASIS_NAME_LEN];

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
#[repr(C)]
pub(crate) struct BasisEncryptor<'a> {
    root: &'a BasisRoot,
    dicts: &'a [DictPointer],
    cipher: Aes256GcmSiv,
    cur_vpage: usize,
    aad: Vec::<u8>,
    journal_rev: JournalType,
    entropy: Rc<RefCell<TrngPool>>,
}
impl<'a> BasisEncryptor<'a> {
    pub(crate) fn new(root: &'a BasisRoot, dicts: &'a [DictPointer], dna: u64, cipher: Aes256GcmSiv, rev: JournalType, entropy: Rc<RefCell<TrngPool>>) -> Self {
        let mut aad = Vec::<u8>::new();
        aad.extend_from_slice(&root.name);
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        aad.extend_from_slice(&dna.to_le_bytes());

        log::info!("aad: {:?}", aad);

        BasisEncryptor {
            root,
            dicts,
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
        if self.vaddr < self.basis_data.root.prealloc_open_end.as_usize() {
            let mut block = [0 as u8; VPAGE_SIZE + size_of::<JournalType>()];
            let block_iter = block.iter_mut();

            let journal_bytes = self.basis_data.journal_rev.to_le_bytes();
            /* rkyv serializer
            let (ser_pos, ser_basis) = self.basis_data.root.ser();
            let ser_bytes = ser_pos.to_le_bytes();
            let slice_iter =
            journal_bytes.iter() // journal rev
                .chain(ser_bytes.iter()
                .chain(ser_basis.as_ref().iter() // basis
                    // .chain(self.dicts.as_slice()  // dictionary
            )).skip(self.vaddr);
            */
            let slice_iter =
            journal_bytes.iter() // journal rev
                .chain(self.basis_data.root.as_ref().iter()
                    // .chain(self.dicts.as_slice()  // dictionary
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
/// In basis space, the BasisRoot is located at VPAGE #1 (VPAGE #0 is always invalid).
/// The first 4GiB is reserved for the Basis Root + Dictionary slice.
/// Key storage begin at 4GiB.
/// AAD associated with the BasisRoot consist of a bytewise concatenation of:
///   - Basis name
///   - version number (should match version inside; complicates downgrade attacks)
///   - FPGA's silicon DNA number (makes a naive raw-copy of the data to another device unusable;
///     but of course, the DNA ID can be forged minor efforts)
///
/// As a directory structure, the basis root is designed to be read into RAM in a contiguous block.
/// it'll typically be less than a page in length, but a pathological number of dictionaries can make it
/// much longer.
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
#[derive(rkyv::Serialize, rkyv::Deserialize, rkyv::Archive, PartialEq, Debug)]
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
    /// "open end" of the pre-allocated space for the Basis. All Basis data must exist in an extent that is
    /// less than this value. This can be grown and shrunk with allocation and compaction processes.
    pub(crate) prealloc_open_end: PageAlignedVa,
    /// virtual address pointer to the start of the dictionary record
    pub(crate) dict_ptr: Option<VirtAddr>,
    // dict_slice: [DictPointer; num_dictionaries],  // DictPointers + num_dictionaries above can be turned into a dict_slice
    ////// the following records are appended by the Serialization routine
    // pad: [u8],    // padding out to the next 4096-byte block less 16 bytes
    // p_tag: [u8; 16], // auth tag output of the AES-GCM-SIV
}
/* // this computation got supplanted by prealloc_open_end -- basis is always laid out linearly in RAM, so we just allocate a straight section of RAM based off of that.
impl BasisRoot {
    /// Compute the number of memory pages consumed by the BasisRoot structure itself.
    /// This is the size of BasisRoot, plus the dictionaries allocated within the Basis.
    /// It does mean that your memory usage scales directly with the number of dictionaries
    /// you put in the Basis, because there is no way to chain or defer the Basis structure
    /// if you get thousands of Dictionaries. Note that the intent is to have typcially no
    /// more than a couple dozen dictionaries; if you want to store a lot of different records,
    /// you can create thousands of Keys more efficiently, than you can dictionaries.
    pub(crate) fn len_vpages(&self) -> usize {
        let min_len = core::mem::size_of::<BasisRoot>()
            + ((self.num_dictionaries as usize) * core::mem::size_of::<DictPointer>());
        if min_len % VPAGE_SIZE == 0 {
            min_len / VPAGE_SIZE
        } else {
            min_len / VPAGE_SIZE + 1
        }
    }
    /// Number of bytes needed to pad between the length of the BasisRoot structure and the plaintext
    /// tag that will get appended to the end
    pub(crate) fn padding_count(&self) -> usize {
        self.len_vpages() * VPAGE_SIZE -
        (core::mem::size_of::<BasisRoot>()
         + ((self.num_dictionaries as usize) * core::mem::size_of::<DictPointer>())
        )
    }
}*/
use rkyv::ser::Serializer;
impl BasisRoot { // note: example wraps u8 in "Aligned"...
    pub(crate) fn ser(&self) -> (u32, Aligned<[u8; size_of::<rkyv::ser::serializers::BufferSerializer<BasisRoot>>()]>) {
        let mut ser = BufferSerializer::new(Aligned([0u8; size_of::<rkyv::ser::serializers::BufferSerializer<BasisRoot>>()]));
        let pos = match ser.serialize_value(self) {
            Ok(p) => p,
            Err(e) => {log::error!("serializer error: {:?}", e); panic!("serializer error");}
        };
        (pos as u32, ser.into_inner())
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


#[repr(C)]
pub(crate) struct DictPointer {
    name: [u8; PDDB_MAX_DICT_NAME_LEN],
    age: u32,  // increment every time the dictionary pointer is modified. Used to guide memory compaction.
    addr: u64, // the virtual address of the dictionary
}

/* this is supplanted by the FSCB
/// FreeSpace address space is in the virtual memory space of the containing Basis
#[derive(Copy, Clone)]
pub(crate) struct FreeSpace {
    start: u64,
    len: NonZeroU64,
}*/

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

