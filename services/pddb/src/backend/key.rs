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

/// On-disk representation of the Key. Note that the storage on disk is mis-aligned, so
/// any deserialization must essentially come with a copy step to line up the record.
#[repr(C, align(8))]
pub(crate) struct KeyDescriptor {
    /// virtual address of the key's start
    pub(crate) start: u64,
    /// length of the key's stored data
    pub(crate) len: u64,
    /// amount of space reserved for the key. Must be >= len.
    pub(crate) reserved: u64,
    /// Reserved for flags on the record entry
    pub(crate) flags: u32,
    /// Access count to the key
    pub(crate) age: u32,
    /// Name. Length should pad out the record to exactly 127 bytes.
    pub(crate) name: [u8; KEY_NAME_LEN],
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
    /// indicates if the cache entry is currently synchronized with what's on disk.
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