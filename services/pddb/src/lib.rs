#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};
use xous::CID;

use std::collections::HashMap;

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

// this structure can be shared on the user side?
pub struct Pddb<'a> {
    conn: CID,
    contents: HashMap<PddbKey<'a>, &'a [u8]>,
    callback: Box<dyn FnMut() + 'a>,
}
impl<'a> Pddb<'a> {
    /// return a list of all open bases
    pub fn list_basis() -> Vec::<String> {
        Vec::new()
    }
    /// returns the latest basis that is opened -- this is where all new values are being sent by default
    pub fn get_current_basis() -> String {
        String::new()
    }

    // opens a dictionary only if it exists
    pub fn open(_dict_name: &str) -> Option<Self> { None }
    // creates a dictionary only if it does not already exist
    pub fn create(dict_name: &str) -> Option<Self> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        None
    }

    /// returns a key only if it exists
    pub fn get(&mut self, _key_name: &str, key_changed_cb: impl FnMut() + 'a) -> Result<Option<PddbKey>> {
        self.callback = Box::new(key_changed_cb);
        Ok(None)
    }
    /// closes the key, de-allocating the OS memory to track it.
    pub fn close(&mut self, _key: PddbKey) -> Result<()> {
        Ok(())
    }
    // updates an existing key's value. mainly used by write().
    pub fn update(&mut self, _key: PddbKey) -> Result<Option<PddbKey>> { Ok(None) }
    // creates a key or overwrites it
    pub fn insert(&mut self, _key: PddbKey) -> Option<PddbKey> { None } // may return the displaced key
    // deletes a key within the dictionary
    pub fn remove(&mut self, _key: PddbKey) -> Result<()> { Ok(()) }
    // deletes the entire dictionary
    pub fn delete(&mut self) {}
}

impl<'a> Drop for Pddb<'a> {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}