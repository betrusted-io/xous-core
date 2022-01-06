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
        let mgmt = PddbBasisRequest {
            name: [0u8; BASIS_NAME_LEN],
            code: PddbRequestCode::Uninit,
        };
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::LatestBasis.to_u32().unwrap()).expect("Couldn't execute ListBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore list structure");
        match ret.code {
            PddbRequestCode::NoErr => {
                Some(cstr_to_string(&ret.name))
            }
            PddbRequestCode::NotMounted => {
                None
            }
            _ => {
                log::error!("Invalid return from latest basis call");
                panic!("Invalid return from latest basis call");
            }
        }
    }
    pub fn create(&self, basis_name: &str) -> Result<()> {
        let mut mgmt = PddbBasisRequest {
            name: [0u8; BASIS_NAME_LEN],
            code: PddbRequestCode::Create,
        };
        for (&src, dst) in basis_name.as_bytes().iter().zip(mgmt.name.iter_mut()) {*dst = src}
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::CreateBasis.to_u32().unwrap()).expect("Couldn't execute CreateBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore mgmt structure");
        match ret.code {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::NoFreeSpace => Err(Error::new(ErrorKind::OutOfMemory, "No free space to create basis")),
            PddbRequestCode::InternalError => Err(Error::new(ErrorKind::Other, "Internal error creating basis")),
            _ => {
                log::error!("Invalid return code");
                panic!("Invalid return code");
            }
        }
    }
    pub fn open(&self, basis_name: &str) -> Result<()> {
        let mut mgmt = PddbBasisRequest {
            name: [0u8; BASIS_NAME_LEN],
            code: PddbRequestCode::Open,
        };
        for (&src, dst) in basis_name.as_bytes().iter().zip(mgmt.name.iter_mut()) {*dst = src}
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::OpenBasis.to_u32().unwrap()).expect("Couldn't execute OpenBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore mgmt structure");
        match ret.code {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::AccessDenied => Err(Error::new(ErrorKind::PermissionDenied, "Authentication error")),
            PddbRequestCode::InternalError => Err(Error::new(ErrorKind::Other, "Internal error creating basis")),
            _ => {
                log::error!("Invalid return code");
                panic!("Invalid return code");
            }
        }
    }
    pub fn close(&self, basis_name: &str) -> Result<()> {
        let mut mgmt = PddbBasisRequest {
            name: [0u8; BASIS_NAME_LEN],
            code: PddbRequestCode::Close,
        };
        for (&src, dst) in basis_name.as_bytes().iter().zip(mgmt.name.iter_mut()) {*dst = src}
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::CloseBasis.to_u32().unwrap()).expect("Couldn't execute CloseBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore mgmt structure");
        match ret.code {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Basis not found")),
            PddbRequestCode::InternalError => Err(Error::new(ErrorKind::Other, "Internal error closing basis")),
            _ => {
                log::error!("Invalid return code");
                panic!("Invalid return code");
            }
        }
    }
    pub fn delete(&self, basis_name: &str) -> Result<()> {
        let mut mgmt = PddbBasisRequest {
            name: [0u8; BASIS_NAME_LEN],
            code: PddbRequestCode::Delete,
        };
        for (&src, dst) in basis_name.as_bytes().iter().zip(mgmt.name.iter_mut()) {*dst = src}
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::DeleteBasis.to_u32().unwrap()).expect("Couldn't execute DeleteBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore mgmt structure");
        match ret.code {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Basis not found")),
            PddbRequestCode::InternalError => Err(Error::new(ErrorKind::Other, "Internal error deleting basis")),
            _ => {
                log::error!("Invalid return code");
                panic!("Invalid return code");
            }
        }
    }
}

pub struct Pddb<'a> {
    conn: CID,
    contents: HashMap<PddbKey<'a>, &'a [u8]>,
    callback: Option<Box<dyn FnMut() + 'a>>,
}
impl<'a> Pddb<'a> {
    // creates a dictionary only if it does not already exist
    pub fn create(dict_name: &str, basis_name: Option<&str>) -> Option<Self> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("can't connect to Pddb server");
        /*let mut request = PddbDictRequest {
            basis_specified: basis_name.is_some(),
        }*/

        None
    }

    /// returns a key only if it exists
    pub fn get(&mut self, dict_name: &str, key_name: &str, basis_name: Option<&str>, key_changed_cb: impl FnMut() + 'a) -> Result<Option<PddbKey>> {
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