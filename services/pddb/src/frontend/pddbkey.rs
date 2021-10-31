//use std::io::{Error, ErrorKind, Result};
//use xous::MemoryRange;
//use std::io::SeekFrom;

//use crate::*;
use rkyv::{Archive, Deserialize, Serialize};
use xous_ipc::String;

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct PddbKeyRequest {
    pub(crate) dict: String::</*PDDB_MAX_DICT_NAME_LEN*/ 64>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) key: String::</*PDDB_MAX_KEY_NAME_LEN*/ 256>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) token: Option<[u32; 3]>,
}


/*
/// PddbKey is somewhat isomorphic to a File in Rust, in that it provides slices of [u8] that
/// can be `read()`, `write()` and `seek()`.
/// this is definitely a user-facing structure
pub struct PddbKey<'a> {
    // dictionary to search for the key within
    dict: PddbDict<'a>,
    // a copy of my name
    name: String,
    // called when the key changes (basis or is modified otherwise)
    key_changed_cb: Box<dyn FnMut() + 'a>,
    // mapped memory for the plaintext contexts, typically not all resident
    mem: MemoryRange,
}

impl<'a> PddbKey<'a> {
    pub fn set_callback(&mut self, key_changed_cb: impl FnMut() + 'a) {
        self.key_changed_cb = Box::new(key_changed_cb);
    }

    pub fn new() {
        let mem = xous::syscall::map_memory(
            None,
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).expect("couldn't map initial memory page");

    }
    /* these get moved to traits */
    /*
    // reads are transparent and "just happen"
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> { Ok(0) }
    // writes will call update() on the dictionary
    pub fn write(&mut self, buf: &[u8]) -> Result<usize> { Ok(0) }
    // provided for compatibility with Rust API
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64> { Ok(0) }
    */
}

*/