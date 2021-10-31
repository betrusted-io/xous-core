#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use xous::{CID, send_message};
use xous_ipc::Buffer;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};
use std::path::Path;
use std::format;

pub const PDDB_MAX_DICT_NAME_LEN: usize = 64;
pub const PDDB_MAX_KEY_NAME_LEN: usize = 256;

pub struct PddbKey {
    conn: CID,
    dict: String,
    key: String,
    token: [u32; 3],
}
impl PddbKey {
    pub fn get<P: AsRef<Path>>(path: P) -> Result<PddbKey> {
        let xns = xous_names::XousNames::new().unwrap();
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("Can't connect to Pddb server");

        if !path.is_absolute() {
            return Err(Error::new(ErrorKind::InvalidInput, "All Xous keys must be fully specified relative to a dictionary"));
        }
        let mut dict = String::new();
        let mut key = String::new();
        if let Some(pathstr) = path.to_str() {
            if let Some(path_lstrip) = path.strip_prefix("/") {
                if let Some((dictstr, keystr)) = path_lstrip.split_once("/") {
                    if dictstr.len() < PDDB_MAX_DICT_NAME_LEN {
                        dict.push_str(dictstr);
                    } else {
                        return Err(Error::new(ErrorKind::InvalidInput, format!("Xous dictionary names must be shorter than {} bytes", PDDB_MAX_DICT_NAME_LEN)));
                    }
                    if keystr.len() < PDDB_MAX_KEY_NAME_LEN {
                        key.push_str(keystr);
                    } else {
                        return Err(Error::new(ErrorKind::InvalidInput, format!("Xous key names must be shorter than {} bytes", PDDB_MAX_DICT_NAME_LEN)));
                    }
                } else {
                    return Err(Error::new(ErrorKind::InvalidInput, "All Xous keys must be of the format /dict/key; the key may contain more /'s"));
                }
            } else {
                return Err(Error::new(ErrorKind::InvalidInput, "All Xous keys must be absolute and start with a /"));
            }
        } else {
            return Err(Error::new(ErrorKind::InvalidInput, "All Xous keys must be valid UTF-8"));
        }

        let request = PddbKeyRequest {
            dict: xous_ipc::String::<PDDB_MAX_DICT_NAME_LEN>::from_str(dict.as_str()),
            key: xous_ipc::String::<PDDB_MAX_KEY_NAME_LEN>::from_str(key.as_str()),
            token: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(conn, Opcode::KeyRequest.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbKeyRequest, _>().unwrap();
        if let Some(token) = response.token {
            Ok(PddbKey {
                conn,
                dict,
                key,
                token,
            })
        } else {
            Err(Error::new(ErrorKind::PermissionDenied, "Dict/Key access denied"))
        }
    }

    pub(crate) fn conn(&self) -> CID {
        self.conn
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for PddbKey {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}





use xous::MemoryRange;
use std::collections::HashMap;
use std::io::SeekFrom;

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
    contents: HashMap<PddbKey, &'a [u8]>,
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

