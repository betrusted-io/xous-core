#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};
use xous::CID;
use xous_ipc::Buffer;

use std::collections::HashMap;

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

pub struct PddbBasisManager {
    conn: CID,
}
impl PddbBasisManager {
    pub fn new() -> Self {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("can't connect to Pddb server");
        PddbBasisManager {
            conn
        }
    }
    /// return a list of all open bases
    pub fn list_basis(&self) -> Vec::<String> {
        let list_alloc = PddbBasisList {
            list: [[0u8; BASIS_NAME_LEN]; 63],
            num: 0
        };
        let mut buf = Buffer::into_buf(list_alloc).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::ListBasis.to_u32().unwrap()).expect("Couldn't execute ListBasis opcode");
        let list = buf.to_original::<PddbBasisList, _>().expect("couldn't restore list structure");
        if list.num > list.list.len() as u32 {
            log::warn!("Number of open basis larger than our IPC structure. May need to refactor this API.");
        }
        let mut ret = Vec::<String>::new();
        for (index, name) in list.list.iter().enumerate() {
            if index as u32 == list.num {
                break;
            }
            ret.push(cstr_to_string(name));
        }
        ret
    }
    /// returns the latest basis that is opened -- this is where all new values are being sent by default
    /// if the PDDB is not mounted, returns None
    pub fn latest_basis(&self) -> Option<String> {
        // just re-use this structure -- because we have to clear the whole page anyways to share it
        let list_alloc = PddbBasisList {
            list: [[0u8; BASIS_NAME_LEN]; 63],
            num: 0
        };
        let mut buf = Buffer::into_buf(list_alloc).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::LatestBasis.to_u32().unwrap()).expect("Couldn't execute ListBasis opcode");
        let list = buf.to_original::<PddbBasisList, _>().expect("couldn't restore list structure");
        if list.num == 1 {
            Some(cstr_to_string(&list.list[0]))
        } else if list.num == 0 {
            None
        } else {
            log::error!("Either 1 or 0 items should be returned by the latest basis call");
            panic!("Either 1 or 0 items should be returned by the latest basis call");
        }
    }
    pub fn create(basis_name: &str) -> Result<()> {
        Ok(())
    }
    pub fn open(basis_name: &str) -> Result<()> {
        Ok(())
    }
    pub fn close(basis_name: &str) -> Result<()> {
        Ok(())
    }
}

// this structure can be shared on the user side?
pub struct Pddb<'a> {
    conn: CID,
    contents: HashMap<PddbKey<'a>, &'a [u8]>,
    callback: Option<Box<dyn FnMut() + 'a>>,
}
impl<'a> Pddb<'a> {
    // opens a dictionary only if it exists
    pub fn open(_dict_name: &str) -> Option<Self> { None }
    // creates a dictionary only if it does not already exist
    pub fn create(dict_name: &str) -> Option<Self> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("can't connect to Pddb server");

        None
    }

    /// returns a key only if it exists
    pub fn get(&mut self, _key_name: &str, key_changed_cb: impl FnMut() + 'a) -> Result<Option<PddbKey>> {
        self.callback = Some(Box::new(key_changed_cb));
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

pub(crate) fn cstr_to_string(cstr: &[u8]) -> String {
    let null_index = cstr.iter().position(|&c| c == 0).expect("couldn't find null terminator on c string");
    String::from_utf8(cstr[..null_index].to_vec()).expect("c string has invalid characters")
}