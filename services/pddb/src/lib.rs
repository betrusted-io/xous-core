#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use num_traits::*;
use std::io::{Result, Error, ErrorKind};
use xous::{CID, SID, msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use core::sync::atomic::{AtomicU32, Ordering};
pub(crate) static REFCOUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static POLLER_REFCOUNT: AtomicU32 = AtomicU32::new(0);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum CbOp {
    Change,
    Quit
}

pub struct PddbMountPoller {
    conn: CID
}
impl PddbMountPoller {
    pub fn new() -> Self {
        POLLER_REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB_POLLER).expect("can't connect to Pddb mount poller server");
        PddbMountPoller { conn }
    }
    /// This call is guaranteed to *never* block and return the instantaneous state of the PDDB, even if the server itself
    /// is currently busy processing other requests. This has to be done with a separate server from the main one, because
    /// the main server will block during the mount operations, as it owns the PDDB data objects and cannot concurrently process
    /// the mount check task while manipulating them. Instead, this routine queries a separate thread that shares an AtomicBool
    /// with the main thread that reports the mount state.
    pub fn is_mounted_nonblocking(&self) -> bool {
        match send_message(self.conn,
            Message::new_blocking_scalar(PollOp::Poll.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't poll mount poller") {
            xous::Result::Scalar1(is_mounted) => {
                if is_mounted == 0 {
                    false
                } else {
                    true
                }
            }
            _ => false
        }
    }
}
impl Drop for PddbMountPoller {
    fn drop(&mut self) {
        if POLLER_REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}


/// The intention is that one Pddb management object is made per process, and this serves
/// as the gateway for parcelling out PddbKey objects, which are the equivalent of a File
/// in a convention system that implements read/write operations.
///
/// The Pddb management object also handles meta-issues such as basis creation, unlock/lock,
/// and callbacks in case data changes.
pub struct Pddb {
    conn: CID,
    /// a SID that we can directly share with the PDDB server for the purpose of handling key change callbacks
    cb: Option<(SID, JoinHandle::<()>)>,
    /// Handle key change updates. The general intent is that the closure implements a
    /// `send` of a message to the server to deal with a key change appropriately,
    /// but no mutable data is allowed within the closure itself due to safety problems.
    /// Thus, the closure might encode something like whether the message is blocking or nonblocking;
    /// the CID of the message; and the opcode and arguments, as static variables. An implementation
    /// with many keys might, for example, keep a lookup table of indices to keys to track which
    /// ones need clearing if you're working at a very fine granularity, but more generally,
    /// the application behavior might be something like a refresh of the data from storage
    /// in the case of a basis change. Basis changes are thought to be rare; so, big changes
    /// like this are probably OK.
    keys: Arc<Mutex<HashMap<ApiToken, Box<dyn Fn() + 'static + Send> >>>,
    trng: trng::Trng,
}
impl Pddb {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("can't connect to Pddb server");
        let keys: Arc<Mutex<HashMap<ApiToken, Box<dyn Fn() + 'static + Send> >>> = Arc::new(Mutex::new(HashMap::new()));
        Pddb {
            conn,
            cb: None,
            keys,
            trng: trng::Trng::new(&xns).unwrap(),
        }
    }
    fn ensure_async_responder(&mut self) {
        if self.cb.is_none() {
            let sid = xous::create_server().unwrap();
            let handle = thread::spawn({
                let keys = Arc::clone(&self.keys);
                let sid = sid.clone();
                move || {
                    loop {
                        let msg = xous::receive_message(sid).unwrap();
                        match FromPrimitive::from_usize(msg.body.id()) {
                            Some(CbOp::Change) => msg_scalar_unpack!(msg, t0, t1, t2, _, {
                                let token: ApiToken = [t0 as u32, t1 as u32, t2 as u32];
                                if let Some(cb) = keys.lock().unwrap().get(&token) {
                                    cb();
                                } else {
                                    log::warn!("Key changed but no callback was hooked to receive it");
                                }
                            }),
                            Some(CbOp::Quit) => { // blocking scalar
                                xous::return_scalar(msg.sender, 0).unwrap();
                                break;
                            },
                            _ =>log::warn!("Got unknown opcode: {:?}", msg),
                        }
                    }
                    xous::destroy_server(sid).unwrap();
                }
            });
            self.cb = Some((sid, handle));
        }
    }
    /// This blocks until the PDDB is mounted by the end user. If `None` is specified for the poll_interval_ms,
    /// A random interval between 1 and 2 seconds is chosen for the poll wait time. Randomizing the waiting time
    /// helps to level out the task scheduler in the case that many threads are waiting on the PDDB simultaneously.
    ///
    /// This is typically the API call one would use to hold execution of a service until the PDDB is mounted.
    pub fn is_mounted_blocking(&self) {
        let ret = send_message(self.conn, Message::new_blocking_scalar(
            Opcode::IsMounted.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute IsMounted query");
        match ret {
            xous::Result::Scalar1(_code) => {
                ()
            },
            _ => panic!("Internal error"),
        }
    }
    pub fn try_mount(&self) -> bool {
        let ret = send_message(self.conn, Message::new_blocking_scalar(
            Opcode::TryMount.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute IsMounted query");
        match ret {
            xous::Result::Scalar1(code) => {
                if code == 0 {false} else {true}
            },
            _ => panic!("Internal error"),
        }
    }
    /// return a list of all open bases
    pub fn list_basis(&self) -> Vec::<String> {
        let list_alloc = PddbBasisList {
            list: [xous_ipc::String::<BASIS_NAME_LEN>::default(); 63],
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
            ret.push(name.as_str().expect("name is not valid utf-8").to_string());
        }
        ret
    }
    /// returns the latest basis that is opened -- this is where all new values are being sent by default
    /// if the PDDB is not mounted, returns None
    pub fn latest_basis(&self) -> Option<String> {
        let mgmt = PddbBasisRequest {
            name: xous_ipc::String::<BASIS_NAME_LEN>::new(),
            code: PddbRequestCode::Uninit,
            policy: None,
        };
        let mut buf = Buffer::into_buf(mgmt).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::LatestBasis.to_u32().unwrap()).expect("Couldn't execute ListBasis opcode");
        let ret = buf.to_original::<PddbBasisRequest, _>().expect("couldn't restore list structure");
        match ret.code {
            PddbRequestCode::NoErr => {
                Some(ret.name.as_str().expect("name wasn't valid utf-8").to_string())
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
    pub fn create_basis(&self, basis_name: &str) -> Result<()> {
        if basis_name.len() > BASIS_NAME_LEN - 1 {
            return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
        }
        let mgmt = PddbBasisRequest {
            name: xous_ipc::String::<BASIS_NAME_LEN>::from_str(basis_name),
            code: PddbRequestCode::Create,
            policy: None,
        };
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
    pub fn unlock_basis(&self, basis_name: &str, policy: Option<BasisRetentionPolicy>) -> Result<()> {
        if basis_name.len() > BASIS_NAME_LEN - 1 {
            return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
        }
        let mgmt = PddbBasisRequest {
            name: xous_ipc::String::<BASIS_NAME_LEN>::from_str(basis_name),
            code: PddbRequestCode::Open,
            policy,
        };
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
    pub fn lock_basis(&self, basis_name: &str) -> Result<()> {
        if basis_name.len() > BASIS_NAME_LEN - 1 {
            return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
        }
        let mgmt = PddbBasisRequest {
            name: xous_ipc::String::<BASIS_NAME_LEN>::from_str(basis_name),
            code: PddbRequestCode::Close,
            policy: None,
        };
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
    pub fn delete_basis(&self, basis_name: &str) -> Result<()> {
        if basis_name.len() > BASIS_NAME_LEN - 1 {
            return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
        }
        let mgmt = PddbBasisRequest {
            name: xous_ipc::String::<BASIS_NAME_LEN>::from_str(basis_name),
            code: PddbRequestCode::Delete,
            policy: None,
        };
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

    /// If the `create_*` flags are set, creates the asset if they do not exist, otherwise if false, returns
    /// an error if the asset does not exist.
    /// `alloc_hint` is an optional field to guide the PDDB allocator to put the key in the right pool. Setting it to `None`
    /// is perfectly fine, it just has a potential performance impact, especially for very large keys.
    /// `key_changed_cb` is a static function meant to initiate a message to a server in case the key in question
    /// goes away due to a basis locking.
    pub fn get(&mut self, dict_name: &str, key_name: &str, basis_name: Option<&str>,
        create_dict: bool, create_key: bool, alloc_hint: Option<usize>, key_changed_cb: Option<impl Fn() + 'static + Send>) -> Result<PddbKey> {
        if key_name.len() > (KEY_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "key name too long"));
        }
        if dict_name.len() > (DICT_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "dictionary name too long"));
        }
        let bname = if let Some(bname) = basis_name {
            if bname.len() > BASIS_NAME_LEN - 1 {
                return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
            }
            xous_ipc::String::<BASIS_NAME_LEN>::from_str(bname)
        } else {
            xous_ipc::String::<BASIS_NAME_LEN>::new()
        };

        if key_changed_cb.is_some() {
            self.ensure_async_responder();
        }
        let cb_sid = if let Some((sid, _handle)) = &self.cb {
            Some(sid.to_array())
        } else {
            None
        };
        let request = PddbKeyRequest {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
            key: xous_ipc::String::<KEY_NAME_LEN>::from_str(key_name),
            create_dict,
            create_key,
            token: None,
            result: PddbRequestCode::Uninit,
            cb_sid,
            alloc_hint: if let Some(a) = alloc_hint {Some(a as u64)} else {None},
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::KeyRequest.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbKeyRequest, _>().unwrap();

        // we probably should never remove this check -- the code may compile correctly and
        // "work" without this being an even page size, but it's pretty easy to get this wrong,
        // and if it's wrong we can lose a lot in terms of efficiency of execution.
        assert!(core::mem::size_of::<PddbBuf>() == 4096, "PddBuf record has the wrong size");
        match response.result {
            PddbRequestCode::NoErr => {
                if let Some(token) = response.token {
                    if let Some(cb) = key_changed_cb {
                        self.keys.lock().unwrap().insert(token, Box::new(cb));
                    }
                    REFCOUNT.fetch_add(1, Ordering::Relaxed);
                    let pk = PddbKey {
                        // i think these fields are redundant, let's save the storage and remove them for now...
                        //dict: String::from(dict_name),
                        //key: String::from(key_name),
                        //basis: if basis_name.is_some() {Some(String::from(bname.as_str().unwrap()))} else {None},
                        pos: 0,
                        token,
                        buf: Buffer::new(core::mem::size_of::<PddbBuf>()),
                        conn: self.conn,
                    };
                    Ok(pk)
                } else {
                    Err(Error::new(ErrorKind::PermissionDenied, "Dict/Key access denied"))
                }
            }
            PddbRequestCode::AccessDenied => Err(Error::new(ErrorKind::PermissionDenied, "Dict/Key access denied")),
            PddbRequestCode::NoFreeSpace => Err(Error::new(ErrorKind::OutOfMemory, "No more space on disk")),
            PddbRequestCode::NotMounted => Err(Error::new(ErrorKind::ConnectionReset, "PDDB was unmounted")),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Dictionary or key was not found")),
            _ => Err(Error::new(ErrorKind::Other, "Internal error"))
        }
    }

    /// deletes a key within the dictionary
    pub fn delete_key(&mut self, dict_name: &str, key_name: &str, basis_name: Option<&str>) -> Result<()> {
        if key_name.len() > (KEY_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "key name too long"));
        }
        if dict_name.len() > (DICT_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "dictionary name too long"));
        }
        let bname = if let Some(bname) = basis_name {
            if bname.len() > BASIS_NAME_LEN - 1 {
                return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
            }
            xous_ipc::String::<BASIS_NAME_LEN>::from_str(bname)
        } else {
            xous_ipc::String::<BASIS_NAME_LEN>::new()
        };

        let cb_sid = if let Some((sid, _handle)) = &self.cb {
            Some(sid.to_array())
        } else {
            None
        };
        let request = PddbKeyRequest {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
            key: xous_ipc::String::<KEY_NAME_LEN>::from_str(key_name),
            create_dict: false,
            create_key: false,
            token: None,
            result: PddbRequestCode::Uninit,
            cb_sid,
            alloc_hint: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::DeleteKey.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbKeyRequest, _>().unwrap();
        match response.result {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Dictionary or key was not found")),
            _ => Err(Error::new(ErrorKind::Other, "Internal error"))
        }
    }
    /// deletes the entire dictionary
    pub fn delete_dict(&mut self, dict_name: &str, basis_name: Option<&str>) -> Result<()> {
        if dict_name.len() > (DICT_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "dictionary name too long"));
        }
        let bname = if let Some(bname) = basis_name {
            if bname.len() > BASIS_NAME_LEN - 1 {
                return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
            }
            xous_ipc::String::<BASIS_NAME_LEN>::from_str(bname)
        } else {
            xous_ipc::String::<BASIS_NAME_LEN>::new()
        };

        let cb_sid = if let Some((sid, _handle)) = &self.cb {
            Some(sid.to_array())
        } else {
            None
        };
        let request = PddbKeyRequest {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
            key: xous_ipc::String::<KEY_NAME_LEN>::new(),
            create_dict: false,
            create_key: false,
            token: None,
            result: PddbRequestCode::Uninit,
            cb_sid,
            alloc_hint: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::DeleteDict.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbKeyRequest, _>().unwrap();
        match response.result {
            PddbRequestCode::NoErr => Ok(()),
            PddbRequestCode::NotFound => Err(Error::new(ErrorKind::NotFound, "Dictionary or key was not found")),
            _ => Err(Error::new(ErrorKind::Other, "Internal error"))
        }
    }

    pub fn sync(&mut self) -> Result<()> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WriteKeyFlush.to_usize().unwrap(), 0, 0, 0, 0)
        ).or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        if let xous::Result::Scalar1(rcode) = response {
            match FromPrimitive::from_u8(rcode as u8) {
                Some(PddbRetcode::Ok) => Ok(()),
                Some(PddbRetcode::BasisLost) => Err(Error::new(ErrorKind::BrokenPipe, "Basis lost")),
                Some(PddbRetcode::DiskFull) => Err(Error::new(ErrorKind::OutOfMemory, "Out of disk space, some data lost on sync")),
                _ => Err(Error::new(ErrorKind::Interrupted, "Flush failed for unspecified reasons")),
            }
        } else {
            Err(Error::new(ErrorKind::Other, "Xous internal error"))
        }
    }

    pub fn list_keys(&mut self, dict_name: &str, basis_name: Option<&str>) -> Result<Vec::<String>> {
        if dict_name.len() > (DICT_NAME_LEN - 1) {
            return Err(Error::new(ErrorKind::InvalidInput, "dictionary name too long"));
        }
        let bname = if let Some(bname) = basis_name {
            if bname.len() > BASIS_NAME_LEN - 1 {
                return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
            }
            xous_ipc::String::<BASIS_NAME_LEN>::from_str(bname)
        } else {
            xous_ipc::String::<BASIS_NAME_LEN>::new()
        };
        // this is a two-phase query, because it's quite likely that the number of keys can be very large in a dict.
        let token = [self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap()];
        let request = PddbDictRequest {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
            key: xous_ipc::String::<KEY_NAME_LEN>::new(),
            index: 0,
            code: PddbRequestCode::Uninit,
            token,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::KeyCountInDict.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbDictRequest, _>().unwrap();
        let count = match response.code {
            PddbRequestCode::NoErr => response.index,
            PddbRequestCode::NotFound => return Err(Error::new(ErrorKind::NotFound, "dictionary not found")),
            _ => return Err(Error::new(ErrorKind::Other, "Internal error")),
        };
        // very non-optimal, slow way of doing this, but let's just get it working first and optimize later.
        // it's absolutely important that you access every entry, and the highest index last, because
        // that is how the server knows you've finished with the list-out.
        let mut key_list = Vec::<String>::new();
        for index in 0..count {
            let request = PddbDictRequest {
                basis_specified: basis_name.is_some(),
                basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
                dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
                key: xous_ipc::String::<KEY_NAME_LEN>::new(),
                index,
                code: PddbRequestCode::Uninit,
                token,
            };
            let mut buf = Buffer::into_buf(request)
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            buf.lend_mut(self.conn, Opcode::GetKeyNameAtIndex.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            let response = buf.to_original::<PddbDictRequest, _>().unwrap();
            match response.code {
                PddbRequestCode::NoErr => key_list.push(String::from(response.key.as_str().expect("utf-8 parse error in key name"))),
                _ => return Err(Error::new(ErrorKind::Other, "Internal error")),
            }
        }
        Ok(key_list)
    }


    pub fn list_dict(&mut self, basis_name: Option<&str>) -> Result<Vec::<String>> {
        let bname = if let Some(bname) = basis_name {
            if bname.len() > BASIS_NAME_LEN - 1 {
                return Err(Error::new(ErrorKind::InvalidInput, "basis name too long"));
            }
            xous_ipc::String::<BASIS_NAME_LEN>::from_str(bname)
        } else {
            xous_ipc::String::<BASIS_NAME_LEN>::new()
        };
        // this is a two-phase query, because it's quite likely that the number of keys can be very large in a dict.
        let token = [self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap()];
        let request = PddbDictRequest {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::new(),
            key: xous_ipc::String::<KEY_NAME_LEN>::new(),
            index: 0,
            code: PddbRequestCode::Uninit,
            token,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::DictCountInBasis.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbDictRequest, _>().unwrap();
        let count = match response.code {
            PddbRequestCode::NoErr => response.index,
            _ => return Err(Error::new(ErrorKind::Other, "Internal error")),
        };
        // very non-optimal, slow way of doing this, but let's just get it working first and optimize later.
        // it's absolutely important that you access every entry, and the highest index last, because
        // that is how the server knows you've finished with the list-out.
        let mut dict_list = Vec::<String>::new();
        for index in 0..count {
            let request = PddbDictRequest {
                basis_specified: basis_name.is_some(),
                basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
                dict: xous_ipc::String::<DICT_NAME_LEN>::new(),
                key: xous_ipc::String::<KEY_NAME_LEN>::new(),
                index,
                code: PddbRequestCode::Uninit,
                token,
            };
            let mut buf = Buffer::into_buf(request)
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            buf.lend_mut(self.conn, Opcode::GetDictNameAtIndex.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            let response = buf.to_original::<PddbDictRequest, _>().unwrap();
            match response.code {
                PddbRequestCode::NoErr => dict_list.push(String::from(response.dict.as_str().expect("utf-8 parse error in key name"))),
                _ => return Err(Error::new(ErrorKind::Other, "Internal error")),
            }
        }
        Ok(dict_list)
    }
    /// Triggers a dump of the PDDB to host disk
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    pub fn dbg_dump(&self, name: &str) -> Result<()> {
        let ipc = PddbDangerousDebug {
            request: DebugRequest::Dump,
            dump_name: xous_ipc::String::from_str(name),
        };
        let buf = Buffer::into_buf(ipc)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend(self.conn, Opcode::DangerousDebug.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error"))).map(|_| ())
    }
    /// Triggers an umount/remount, forcing a read of the PDDB from disk back into the cache structures.
    /// It's a cheesy way to test a power cycle, without having to power cycle.
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    pub fn dbg_remount(&self) -> Result<()> {
        let ipc = PddbDangerousDebug {
            request: DebugRequest::Remount,
            dump_name: xous_ipc::String::new(),
        };
        let buf = Buffer::into_buf(ipc)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend(self.conn, Opcode::DangerousDebug.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error"))).map(|_| ())
    }
}

impl Drop for Pddb {
    fn drop(&mut self) {
        if let Some((cb_sid, handle)) = self.cb.take() {
            let cid = xous::connect(cb_sid).unwrap();
            send_message(cid, Message::new_blocking_scalar(CbOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).ok();}
            handle.join().expect("couldn't terminate callback helper thread");
        }

        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
