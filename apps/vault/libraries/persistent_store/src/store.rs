// Copyright 2019-2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Store implementation.
pub const OPENSK2_DICT: &'static str = "opensk";

#[cfg(feature = "std")]
pub use crate::model::{StoreModel, StoreOperation};
use crate::{usize_to_nat, Nat, Storage, StorageError};
#[cfg(feature = "std")]
pub use crate::{
    BufferStorage, StoreDriver, StoreDriverOff, StoreDriverOn, StoreInterruption, StoreInvariant,
};
use core::borrow::Borrow;
use std::io::{Write, Read};

/// Errors returned by store operations.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Invalid argument.
    ///
    /// The store is left unchanged. The operation will repeatedly fail until the argument is fixed.
    InvalidArgument,

    /// Not enough capacity.
    ///
    /// The store is left unchanged. The operation will repeatedly fail until capacity is freed.
    NoCapacity,

    /// Reached end of lifetime.
    ///
    /// The store is left unchanged. The operation will repeatedly fail until emergency lifetime is
    /// added.
    NoLifetime,

    /// A storage operation failed.
    ///
    /// The consequences depend on the storage failure. In particular, the operation may or may not
    /// have succeeded, and the storage may have become invalid. Before doing any other operation,
    /// the store should be [recovered](Store::recover). The operation may then be retried if
    /// idempotent.
    StorageError,

    /// Storage is invalid.
    ///
    /// The storage should be erased and the store [recovered](Store::recover). The store would be
    /// empty and have lost track of lifetime.
    InvalidStorage,
}

impl From<StorageError> for StoreError {
    fn from(error: StorageError) -> StoreError {
        match error {
            StorageError::CustomError => StoreError::StorageError,
            // The store always calls the storage correctly.
            StorageError::NotAligned | StorageError::OutOfBounds => unreachable!(),
        }
    }
}

/// Result of store operations.
pub type StoreResult<T> = Result<T, StoreError>;

/// Converts an Option into a StoreResult.
///
/// The None case is considered invalid and returns [`StoreError::InvalidStorage`].
fn or_invalid<T>(x: Option<T>) -> StoreResult<T> {
    x.ok_or(StoreError::InvalidStorage)
}

/// Progression ratio for store metrics.
///
/// This is used for the [`Store::capacity`] and [`Store::lifetime`] metrics. Those metrics are
/// measured in words.
///
/// # Invariant
///
/// - The used value does not exceed the total: `used` ≤ `total`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StoreRatio {
    /// How much of the metric is used.
    pub(crate) used: Nat,

    /// How much of the metric can be used at most.
    pub(crate) total: Nat,
}

impl StoreRatio {
    /// How much of the metric is used.
    pub fn used(self) -> usize {
        self.used as usize
    }

    /// How much of the metric can be used at most.
    pub fn total(self) -> usize {
        self.total as usize
    }

    /// How much of the metric is remaining.
    pub fn remaining(self) -> usize {
        (self.total - self.used) as usize
    }
}

/// Safe pointer to an entry.
///
/// A store handle stays valid at least until the next mutable operation. Store operations taking a
/// handle as argument always verify that the handle is still valid.
#[derive(Clone, Debug)]
pub struct StoreHandle {
    /// The key of the entry.
    key: usize,
}

impl StoreHandle {
    /// Returns the key of the entry.
    pub fn get_key(&self) -> usize {
        self.key
    }

    /// Returns the value length of the entry.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidArgument`] if the entry has been deleted or compacted.
    pub fn get_length<S: Storage>(&self, store: &Store<S>) -> StoreResult<usize> {
        store.get_length(self)
    }

    /// Returns the value of the entry.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidArgument`] if the entry has been deleted or compacted.
    pub fn get_value<S: Storage>(&self, store: &Store<S>) -> StoreResult<Vec<u8>> {
        store.get_value(self)
    }
}

/// Represents an update to the store as part of a transaction.
#[derive(Clone, Debug)]
pub enum StoreUpdate<ByteSlice: Borrow<[u8]>> {
    /// Inserts or replaces an entry in the store.
    Insert { key: usize, value: ByteSlice },

    /// Removes an entry from the store.
    Remove { key: usize },
}

impl<ByteSlice: Borrow<[u8]>> StoreUpdate<ByteSlice> {
    /// Returns the key affected by the update.
    pub fn key(&self) -> usize {
        match *self {
            StoreUpdate::Insert { key, .. } => key,
            StoreUpdate::Remove { key } => key,
        }
    }

    /// Returns the value written by the update.
    pub fn value(&self) -> Option<&[u8]> {
        match self {
            StoreUpdate::Insert { value, .. } => Some(value.borrow()),
            StoreUpdate::Remove { .. } => None,
        }
    }
}

pub type StoreIter<'a> = Box<dyn Iterator<Item = StoreResult<StoreHandle>> + 'a>;

/// Implements a store with a map interface over a storage.
pub struct Store<S: Storage> {
    storage: S,
    pddb: pddb::Pddb,
    entries: Option<Vec::<String>>,
}
impl<S: Storage + Clone> Clone for Store<S> {
    fn clone(&self) -> Self {
        Store {
            storage: self.storage.clone(),
            entries: self.pddb.list_keys(crate::store::OPENSK2_DICT, None).ok(),
            pddb: pddb::Pddb::new(),
        }
    }
}

impl<S: Storage> Store<S> {
    /// Resumes or initializes a store for a given storage.
    ///
    /// If the storage is completely erased, it is initialized. Otherwise, a possible interrupted
    /// operation is recovered by being either completed or rolled-back. In case of error, the
    /// storage ownership is returned.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidArgument`] if the storage is not
    /// [supported](Format::is_storage_supported).
    pub fn new(storage: S) -> Result<Store<S>, (StoreError, S)> {
        let pddb = pddb::Pddb::new();
        Ok(Store {
            storage,
            entries: pddb.list_keys(crate::store::OPENSK2_DICT, None).ok(),
            pddb,
        })
    }

    /// Extracts the storage.
    pub fn extract_storage(self) -> S {
        self.storage
    }

    /// Iterates over the entries.
    pub fn iter<'a>(&'a self) -> StoreResult<StoreIter<'a>> {
        Ok(Box::new(
            or_invalid(self.entries.as_ref())?
                .iter()
                .map(
                    move |key_name| {
                        Ok(
                            StoreHandle {
                                key: usize::from_str_radix(key_name, 10)
                                .map_err(|_| StoreError::InvalidStorage)?
                            }
                        )
                    }
                )
            )
        )
    }

    /// Returns the current and total capacity in words.
    ///
    /// The capacity represents the size of what is stored.
    pub fn capacity(&self) -> StoreResult<StoreRatio> {
        // The PDDB deliberately does not know its free space capacity,
        // so we return some bogus hard-coded numbers.
        Ok(StoreRatio { used: 16384, total: 1024 * 1024 * 96 })
    }

    /// Returns the current and total lifetime in words.
    ///
    /// The lifetime represents the age of the storage. The limit is an over-approximation by at
    /// most the maximum length of a value (the actual limit depends on the length of the prefix of
    /// the first physical page once all its erase cycles have been used).
    pub fn lifetime(&self) -> StoreResult<StoreRatio> {
        Ok(StoreRatio { used: 16384, total: 1024 * 1024 * 96 })
    }

    /// Applies a sequence of updates as a single transaction.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidArgument`] in the following circumstances:
    /// - There are [too many](Format::max_updates) updates.
    /// - The updates overlap, i.e. their keys are not disjoint.
    /// - The updates are invalid, e.g. key [out of bound](Format::max_key) or value [too
    ///   long](Format::max_value_len).
    pub fn transaction<ByteSlice: Borrow<[u8]>>(
        &mut self,
        updates: &[StoreUpdate<ByteSlice>],
    ) -> StoreResult<()> {
        let count = usize_to_nat(updates.len());
        if count == 0 {
            return Ok(());
        }

        for update in updates {
            match *update {
                StoreUpdate::Insert { key, ref value } => {
                    // if it exists, remove the key; if it doesn't exist, ignore the error
                    log::debug!("pre-delete key: {}:{}", crate::store::OPENSK2_DICT, key.to_string());
                    self.pddb.delete_key(
                        crate::store::OPENSK2_DICT,
                        &key.to_string(),
                        None
                    ).ok();
                    log::debug!("write key: {}:{}", crate::store::OPENSK2_DICT, key.to_string());
                    match self.pddb.get(
                        crate::store::OPENSK2_DICT,
                        &key.to_string(),
                        None,
                        true, true, None, // for some reason, we can't call .len() on ByteSlice??
                        None::<fn()>
                    ) {
                        Ok(mut record) => {
                            match record.write_all(value.borrow().into()) {
                                Ok(_) => {}
                                Err(e) => {
                                    log::error!("Error {:?} writing {}:{}", e, crate::store::OPENSK2_DICT, key.to_string());
                                    return Err(StoreError::StorageError);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Storage error in transaction() processing key {}: {:?}", key, e);
                            return Err(StoreError::StorageError);
                        }
                    }
                }
                StoreUpdate::Remove { key } => {
                    log::debug!("remove key: {}:{}", crate::store::OPENSK2_DICT, key.to_string());
                    match self.pddb.delete_key(
                        crate::store::OPENSK2_DICT,
                        &key.to_string(),
                        None
                    ) {
                        Ok(_) => {},
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::NotFound => {},
                            _ => {
                                log::warn!("Encountered error in StorageUpdate::Remove: {:?}", e);
                                return Err(StoreError::StorageError)
                            },
                        }
                    }
                }
            }
        }
        self.entries = self.pddb.list_keys(crate::store::OPENSK2_DICT, None).ok();
        Ok(())
    }

    /// Removes multiple entries as part of a single transaction.
    ///
    /// Entries with a key larger or equal to `min_key` are deleted.
    pub fn clear(&mut self, min_key: usize) -> StoreResult<()> {
        let keys = self.pddb.list_keys(crate::store::OPENSK2_DICT, None)
            .map_err(|_| StoreError::StorageError)?;
        for key in keys {
            if let Ok(key_as_usize) = usize::from_str_radix(&key, 10) {
                if key_as_usize >= min_key {
                    log::debug!("clear deleting: {}", key);
                    self.pddb.delete_key(crate::store::OPENSK2_DICT,
                        &key,
                        None
                    ).map_err(|_| StoreError::StorageError)?;
                }
            } else {
                log::error!("Internal coding error: all keys in {} should be numeric. Got: {}",
                    crate::store::OPENSK2_DICT,
                    &key
                );
                return Err(StoreError::InvalidArgument);
            }
        }
        self.entries = self.pddb.list_keys(crate::store::OPENSK2_DICT, None).ok();
        Ok(())
    }

    /// Compacts the store once if needed.
    ///
    /// If the immediate capacity is at least `length` words, then nothing is modified. Otherwise,
    /// one page is compacted.
    pub fn prepare(&mut self, _length: usize) -> Result<(), StoreError> {
        Ok(())
    }

    /// Recovers a possible interrupted operation.
    ///
    /// If the storage is completely erased, it is initialized.
    pub fn recover(&mut self) -> StoreResult<()> {
        Ok(())
    }

    /// Returns the value of an entry given its key.
    pub fn find(&self, key: usize) -> StoreResult<Option<Vec<u8>>> {
        log::debug!("find key: {}", key);
        match self.pddb.get(
            crate::store::OPENSK2_DICT,
            &key.to_string(),
            None,
            false, false, None, None::<fn()>
        ) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data).map_err(|_| StoreError::StorageError)?;
                Ok(Some(data))
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(StoreError::StorageError),
            }
        }
    }

    /// Returns a handle to an entry given its key.
    pub fn find_handle(&self, key: usize) -> StoreResult<Option<StoreHandle>> {
        log::debug!("listing all keys to find_handle {}", key);
        let keys = match self.pddb.list_keys(
            crate::store::OPENSK2_DICT,
            None
        ) {
            Ok(k) => k,
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    log::debug!("Dictionary does not exist, returning empty list");
                    Vec::new()
                },
                _ => return Err(StoreError::StorageError)
            }
        };
        log::debug!("keylist: {:?}", keys);
        if keys.contains(&key.to_string()) {
            log::debug!("find_handle found: {}", key);
            Ok(Some(StoreHandle {key}))
        } else {
            log::debug!("find_handle did not find: {}", key);
            Ok(None)
        }
    }

    /// Inserts an entry in the store.
    ///
    /// If an entry for the same key is already present, it is replaced.
    pub fn insert(&mut self, key: usize, value: &[u8]) -> StoreResult<()> {
        self.transaction(&[
            StoreUpdate::Insert {
                key,
                value
            }
        ])
    }

    /// Removes an entry given its key.
    ///
    /// This is not an error if there is no entry for this key.
    pub fn remove(&mut self, key: usize) -> StoreResult<()> {
        self.transaction(&[
            StoreUpdate::<&[u8]>::Remove { key }
        ])
    }

    /// Removes an entry given a handle.
    pub fn remove_handle(&mut self, handle: &StoreHandle) -> StoreResult<()> {
        self.transaction(&[
            StoreUpdate::<&[u8]>::Remove {
                key: handle.key
            }
        ])
    }

    /// Returns the maximum length in bytes of a value.
    pub fn max_value_length(&self) -> usize {
        // an arbitrarily large limit; large keys can theoretically go to the entire size of
        // the PDDB, but 10 MiB should be more than enough for FIDO2
        10 * 1024 * 1024
    }

    fn get_length(&self, handle: &StoreHandle) -> StoreResult<usize> {
        match self.pddb.get(
            crate::store::OPENSK2_DICT,
            &handle.key.to_string(),
            None,
            false, false, None, None::<fn()>
        ) {
            Ok(record) => {
                let attr = record.attributes().map_err(|_| StoreError::StorageError)?;
                Ok(attr.len)
            }
            _ => Err(StoreError::InvalidArgument) // key didn't exist
        }
    }

    fn get_value(&self, handle: &StoreHandle) -> StoreResult<Vec<u8>> {
        self.find(
            handle.key
        ).map(|val| val.ok_or(StoreError::InvalidArgument))?
    }

}

// Those functions are not meant for production.
#[cfg(feature = "std")]
impl Store<BufferStorage> {
    /// Returns the storage configuration.
    pub fn format(&self) -> &Format {
        &self.format
    }

    /// Accesses the storage.
    pub fn storage(&self) -> &BufferStorage {
        &self.storage
    }

    /// Accesses the storage mutably.
    pub fn storage_mut(&mut self) -> &mut BufferStorage {
        &mut self.storage
    }

    /// Returns the value of a possibly deleted entry.
    ///
    /// If the value has been partially compacted, only return the non-compacted part. Returns an
    /// empty value if it has been fully compacted.
    pub fn inspect_value(&self, handle: &StoreHandle) -> Vec<u8> {
        unimplemented!()
    }

    /// Applies an operation and returns the deleted entries.
    ///
    /// Note that the deleted entries are before any compaction, so they may point outside the
    /// window. This is more expressive than returning the deleted entries after compaction since
    /// compaction can be controlled independently.
    pub fn apply(&mut self, operation: &StoreOperation) -> (Vec<StoreHandle>, StoreResult<()>) {
        unimplemented!()
    }

    /// Initializes an erased storage as if it has been erased `cycle` times.
    pub fn init_with_cycle(storage: &mut BufferStorage, cycle: usize) {
        unimplemented!()
    }
}
