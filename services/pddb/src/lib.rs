#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};

use xous::MemoryRange;
use std::collections::HashMap;



////// probably things in this file should eventually be split ount into their own
////// frontend module file.

// this is an intenal structure for managing the overall PDDB
pub(crate) struct PddbManager {

}
impl PddbManager {
    // return a list of open bases
    fn list_basis() {}
}

// this is an internal struct for managing a basis
pub(crate) struct PddbBasis {

}
impl PddbBasis {
    // opening and closing basis will side-effect PddbDicts and PddbKeys
    fn open() {} // will result in a password box being triggered to open a basis
    fn close() {}
}

// this structure can be shared on the user side?
pub struct PddbDict<'a> {
    contents: HashMap<PddbKey<'a>, &'a [u8]>,
    callback: Box<dyn FnMut() + 'a>,
}
impl<'a> PddbDict<'a> {
    // opens a dictionary only if it exists
    pub fn open(dict_name: &str) -> Option<PddbDict> { None }
    // creates a dictionary only if it does not already exist
    pub fn create(dict_name: &str) -> Option<PddbDict> { None }

    // returns a key only if it exists
    pub fn get(&mut self, key_name: &str, key_changed_cb: impl FnMut() + 'a) -> Result<Option<PddbKey>> {
        self.callback = Box::new(key_changed_cb);
        Ok(None)
    }
    // updates an existing key's value. mainly used by write().
    pub fn update(&mut self, key: PddbKey) -> Result<Option<PddbKey>> { Ok(None) }
    // creates a key or overwrites it
    pub fn insert(&mut self, key: PddbKey) -> Option<PddbKey> { None } // may return the displaced key
    // deletes a key within the dictionary
    pub fn remove(&mut self, key: PddbKey) -> Result<()> { Ok(()) }
    // deletes the entire dictionary
    pub fn delete(&mut self) {}
}

