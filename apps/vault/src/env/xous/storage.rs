#![rustfmt::skip]
// Copyright2019-2021 Google LLC
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

use crate::env::UpgradeStorage;
use persistent_store::{Storage, StorageIndex, StorageResult};
use std::borrow::Cow;

pub struct XousStorage {
}

impl XousStorage {
    /// Dummy shim; actual write layer is implemented using the PDDB
    pub fn new() -> StorageResult<XousStorage> {
        Ok(XousStorage {  })
    }
}

impl Storage for XousStorage {
    fn word_size(&self) -> usize {
        1
    }

    fn page_size(&self) -> usize {
        4096
    }

    fn num_pages(&self) -> usize {
        xous::PDDB_LEN as usize / 4096
    }

    fn max_word_writes(&self) -> usize {
        100_000
    }

    fn max_page_erases(&self) -> usize {
        100_000
    }

    fn read_slice(&self, _index: StorageIndex, _length: usize) -> StorageResult<Cow<[u8]>> {
        unimplemented!()
    }

    fn write_slice(&mut self, _index: StorageIndex, _value: &[u8]) -> StorageResult<()> {
        unimplemented!()
    }

    fn erase_page(&mut self, _page: usize) -> StorageResult<()> {
        unimplemented!()
    }
}

pub struct XousUpgradeStorage {
}
impl UpgradeStorage for XousUpgradeStorage {
    fn write_bundle(&mut self, _offset: usize, _data: Vec<u8>) -> StorageResult<()> {
        unimplemented!()
    }
    fn bundle_identifier(&self) -> u32 {
        unimplemented!()
    }
    fn running_firmware_version(&self) -> u64 {
        unimplemented!()
    }
}