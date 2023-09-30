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
use std::cell::RefCell;

use core::ops::DerefMut;
use core::mem::size_of;
use std::convert::TryInto;
use rkyv::{
    archived_value,
    de::deserializers::AllocDeserializer,
    ser::{Serializer, serializers::WriteSerializer},
    AlignedVec,
    Deserialize,
};

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
    cb: RefCell<Option<SID>>,
    cb_handle: RefCell<Option<JoinHandle::<()>>>,
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
    /// These are temporary fields only to be used by the consistency check feature.
    key_count: RefCell<u32>,
    found_key_count: RefCell<u32>,
}
impl Pddb {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("can't connect to Pddb server");
        let keys: Arc<Mutex<HashMap<ApiToken, Box<dyn Fn() + 'static + Send> >>> = Arc::new(Mutex::new(HashMap::new()));
        Pddb {
            conn,
            cb: RefCell::new(None),
            cb_handle: RefCell::new(None),
            keys,
            trng: trng::Trng::new(&xns).unwrap(),
            /// These are record the result of the most recent call to list_keys()
            key_count: RefCell::new(0),
            found_key_count: RefCell::new(0),
        }
    }
    fn ensure_async_responder(&self) {
        if self.cb.borrow().is_none() {
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
            self.cb.replace(Some(sid));
            self.cb_handle.replace(Some(handle));
        }
    }
    /// This blocks until the PDDB is mounted by the end user. It blocks using the deferred-response pattern,
    /// so the blocking does not spin-wait; the caller does not consume CPU cycles.
    ///
    /// This is typically the API call one would use to hold execution of a service until the PDDB is mounted.
    pub fn is_mounted_blocking(&self) {
        loop {
            let ret = send_message(self.conn, Message::new_blocking_scalar(
                Opcode::IsMounted.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute IsMounted query");
            match ret {
                xous::Result::Scalar2(code, _count) => {
                    if code == 0 { // mounted successfully
                        break;
                    }
                },
                _ => panic!("Internal error"),
            }
        }
    }
    pub fn mount_attempted_blocking(&self) {
        loop {
            let ret = send_message(self.conn, Message::new_blocking_scalar(
                Opcode::MountAttempted.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute IsMounted query");
            match ret {
                xous::Result::Scalar2(code, _count) => {
                    if code == 0 { // mounted successfully
                        break;
                    }
                },
                _ => panic!("Internal error"),
            }
        }
    }
    /// Attempts to mount the system basis. Returns `true` on success, `false` on failure.
    /// This call may cause a password request box to pop up, in the case that the boot PIN is not currently cached.
    ///
    /// Returns:
    ///   Ok(true) on successful mount
    ///   Ok(false) on user-directed abort of mount
    ///   Ok(usize) on system-forced abort of mount, with `count` failures
    pub fn try_mount(&self) -> (bool, usize) {
        let ret = send_message(self.conn, Message::new_blocking_scalar(
            Opcode::TryMount.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute TryMounted query");
        match ret {
            xous::Result::Scalar2(code, count) => {
                if code == 0 {
                    // mounted successfully
                    (true, count)
                } else if code == 1 {
                    // user aborted
                    (true, count)
                } else {
                    // system aborted with `count` retries
                    (false, count)
                }
            },
            _ => panic!("TryMount unexpected return result"),
        }
    }
    /// Unmounts the PDDB. First attempts to unmount any open secret bases, and then finally unmounts
    /// the system basis. Returns `true` on success, `false` on failure.
    pub fn try_unmount(&self) -> bool {
        let ret = send_message(self.conn, Message::new_blocking_scalar(
            Opcode::TryUnmount.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't execute TryUnmount query");
        match ret {
            xous::Result::Scalar1(code) => {
                if code == 0 {false} else {true}
            },
            _ => panic!("Internal error"),
        }
    }
    /// This is a non-blocking message that permanently halts the PDDB server by wedging it in an infinite loop.
    /// It is meant to be called prior to system shutdows or backups, to ensure that other auto-mounting processes
    /// don't undo the shutdown procedure because of an ill-timed "cron" job (the system doesn't literally have
    /// a cron daemon, but it does have the notion of long-running background jobs that might do something like
    /// trigger an NTP update, which would then try to write the updated time to the PDDB).
    pub fn pddb_halt(&self) {
        send_message(self.conn, Message::new_scalar(
            Opcode::PddbHalt.to_usize().unwrap(), 0, 0, 0, 0)).expect("Couldn't halt the PDDB");
    }
    /// Computes checksums on the entire PDDB database. This operation can take some time and causes a progress
    /// bar to pop up. This should be called only after the PDDB has been unmounted, to ensure that the disk
    /// contents do not change after the checksums have been computed.
    pub fn compute_checksums(&self) -> root_keys::api::Checksums {
        let alloc = root_keys::api::Checksums::default();
        let mut buf = Buffer::into_buf(alloc).expect("Couldn't convert memory structure");
        buf.lend_mut(self.conn, Opcode::ComputeBackupHashes.to_u32().unwrap()).expect("Couldn't execute ComputeBackupHashes");
        buf.to_original::<root_keys::api::Checksums, _>().expect("Couldn't convert IPC structure")
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
    /// The caller of this function will block and return only if the order of Bases has changed.
    pub fn monitor_basis(&self) -> Vec::<String> {
        let list_alloc = PddbBasisList {
            list: [xous_ipc::String::<BASIS_NAME_LEN>::default(); 63],
            num: 0
        };
        let mut buf = Buffer::into_buf(list_alloc).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.conn, Opcode::BasisMonitor.to_u32().unwrap()).expect("Couldn't execute ListBasis opcode");
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
            PddbRequestCode::DuplicateEntry => Err(Error::new(ErrorKind::AlreadyExists, "Basis already exists")),
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
    pub fn get(&self, dict_name: &str, key_name: &str, basis_name: Option<&str>,
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

        let maybe_cb = self.cb.take();
        let cb_sid = if let Some(sid) = maybe_cb {
            Some(sid.to_array())
        } else {
            None
        };
        self.cb.replace(maybe_cb);

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
            _ => Err(Error::new(ErrorKind::Other, format!("Unhandled return code: {:?}", response.result))),
        }
    }

    /// deletes a key within the dictionary
    pub fn delete_key(&self, dict_name: &str, key_name: &str, basis_name: Option<&str>) -> Result<()> {
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

        let maybe_cb = self.cb.take();
        let cb_sid = if let Some(sid) = maybe_cb {
            Some(sid.to_array())
        } else {
            None
        };
        self.cb.replace(maybe_cb);

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
    /// deletes a list of keys from a dictionary. The list of keys must be less than MAX_PDDB_DELETE_LEN characters long.
    pub fn delete_key_list(&self, dict_name: &str, key_list: Vec::<String>, basis_name: Option<&str>) -> Result<()> {
        if key_list.len() == 0 {
            return Ok(())
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
        let mut request = PddbDeleteList {
            basis_specified: basis_name.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&bname),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict_name),
            retcode: PddbRetcode::Uninit,
            data: [0u8; MAX_PDDB_DELETE_LEN]
        };
        let mut index = 0;
        for keyname in key_list {
            if keyname.len() > (KEY_NAME_LEN - 1) {
                return Err(Error::new(ErrorKind::InvalidInput, "one of the key names is too long"));
            }
            if keyname.len() + 1 + index < MAX_PDDB_DELETE_LEN {
                assert!(keyname.len() < u8::MAX as usize); // this should always be true due to other limits in the PDDB
                request.data[index] = keyname.len() as u8;
                index += 1;
                request.data[index..index + keyname.len()].copy_from_slice(keyname.as_bytes());
                index += keyname.len();
            } else {
                return Err(Error::new(ErrorKind::OutOfMemory, "Key list total size exceeds MAX_PDDBKLISTLEN"));
            }
        }
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::DictBulkDelete.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        let response = buf.as_flat::<PddbDeleteList, _>().unwrap();
        match response.retcode {
            ArchivedPddbRetcode::Ok => Ok(()),
            ArchivedPddbRetcode::AccessDenied => Err(Error::new(ErrorKind::NotFound, "Dictionary not found, or inaccessible")),
            ArchivedPddbRetcode::Uninit => Err(Error::new(ErrorKind::ConnectionAborted, "Return code not set processing bulk delete, server aborted?")),
            _ => Err(Error::new(ErrorKind::Other, "Internal Error handling bulk delete list")),
        }
    }
    /// deletes the entire dictionary
    pub fn delete_dict(&self, dict_name: &str, basis_name: Option<&str>) -> Result<()> {
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

        let maybe_cb = self.cb.take();
        let cb_sid = if let Some(sid) = maybe_cb {
            Some(sid.to_array())
        } else {
            None
        };
        self.cb.replace(maybe_cb);

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

    pub fn sync(&self) -> Result<()> {
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
    /// cleans up inconsistencies in the PDDB. fsck-like.
    pub fn sync_cleanup(&self) -> Result<()> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::WriteKeyFlush.to_usize().unwrap(), 1, 0, 0, 0)
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

    /// Returns a tuple of (recorded, found) keys. If record != found then the dictionary is inconsistent.
    /// Only valid if the most recent call to the pddb object was list_keys() (as called below)
    /// This is meant to be used only for certain maintenance routines, so the ergonomics are crap on it.
    ///
    /// SAFETY: caller must guarantee that list_keys() was called prior to calling this
    pub unsafe fn was_listed_dict_consistent(&self) -> (u32, u32) {
        (*self.key_count.borrow(), *self.found_key_count.borrow())
    }

    pub fn list_keys(&self, dict_name: &str, basis_name: Option<&str>) -> Result<Vec::<String>> {
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
            bulk_limit: None,
            key_count: 0,
            found_key_count: 0,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::KeyCountInDict.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;

        let response = buf.to_original::<PddbDictRequest, _>().unwrap();
        match response.code {
            PddbRequestCode::NoErr => (),
            PddbRequestCode::NotFound => return Err(Error::new(ErrorKind::NotFound, "dictionary not found")),
            PddbRequestCode::AccessDenied => return Err(Error::new(ErrorKind::PermissionDenied, "concurrent operation in progress")),
            PddbRequestCode::Uninit => return Err(Error::new(ErrorKind::ConnectionAborted, "Return code not set getting key count, server aborted?")),
            _ => return Err(Error::new(ErrorKind::Other, "Internal error generating key count")),
        };
        *self.key_count.borrow_mut() = response.key_count;
        *self.found_key_count.borrow_mut() = response.found_key_count;
        // v2 key listing packs the key list into a larger [u8] field that should cut down on the number of messages
        // required to list a large dictionary by about a factor of 50.
        let mut key_list = Vec::<String>::new();
        // just make sure we didn't screw up the sizing of this record
        assert!(core::mem::size_of::<PddbKeyList>() < 4096);
        loop {
            let request = PddbKeyList {
                token,
                end: false,
                retcode: PddbRetcode::Uninit,
                data: [0u8; MAX_PDDBKLISTLEN]
            };
            let mut buf = Buffer::into_buf(request)
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            buf.lend_mut(self.conn, Opcode::ListKeyV2.to_u32().unwrap())
                .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
            let response = buf.as_flat::<PddbKeyList, _>().unwrap();
            match response.retcode {
                ArchivedPddbRetcode::Ok => (),
                ArchivedPddbRetcode::AccessDenied => return Err(Error::new(ErrorKind::PermissionDenied, "KeyList facility locked by another process, try again later")),
                ArchivedPddbRetcode::Uninit => return Err(Error::new(ErrorKind::ConnectionAborted, "Return code not set fetching list, server aborted?")),
                _ => return Err(Error::new(ErrorKind::Other, "Internal Error fetching list")),
            }
            // the [u8] data is structured as a packed list of u8-len + u8 data slice. The max length of
            // a PDDB key name is guaranteed to be shorter than a u8. If the length field is 0, then this
            // particular response has no more data in it to read.
            let mut index = 0;
            while response.data[index] != 0 && index < MAX_PDDBKLISTLEN {
                let strlen = response.data[index] as usize;
                index += 1;
                if strlen + index >= MAX_PDDBKLISTLEN {
                    log::error!("Logic error in key list, index would be out of bounds. Aborting");
                    break;
                }
                if strlen == 0 { // case of an empty dictionary
                    break;
                }
                let key = String::from(std::str::from_utf8(&response.data[index..index+strlen]).unwrap_or("UTF8 error"));
                log::trace!("Returned {}", key);
                key_list.push(key);
                index += strlen;
            }
            if response.end {
                break;
            }

        }
        Ok(key_list)
    }


    pub fn list_dict(&self, basis_name: Option<&str>) -> Result<Vec::<String>> {
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
            bulk_limit: None,
            key_count: 0,
            found_key_count: 0,
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
                bulk_limit: None,
                key_count: 0,
                found_key_count: 0,
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
    /// Public function to query efuse security state. Replicated here to avoid exposing RootKeys full API to the world.
    pub fn is_efuse_secured(&self) -> bool {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::IsEfuseSecured.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't make call to query efuse security state");
        if let xous::Result::Scalar1(result) = response {
            if result == 1 {
                true
            } else {
                false
            }
        } else {
            panic!("Internal error: wrong return code for is_efuse_secured()");
        }
    }
    /// Reset the "don't ask to init root keys" flag. Used primarily by the OQC test routine to reset this in case
    /// a worker accidentally set it during testing.
    pub fn reset_dont_ask_init(&self) {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ResetDontAskInit.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send ResetDontAskInit");
    }
    /// Flush the `SpaceUpdate` journal. SpaceUpdates will leak the last couple hundred or so free space operations,
    /// so periodically flushing this is needed to restore deniability.
    pub fn flush_space_update(&self) {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::FlushSpaceUpdate.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send FlushSpaceUpdate");
    }
    /// Manually prune the PDDB cache.
    /// Mostly provided for force-triggering for testing; normally this is done automatically
    pub fn manual_prune(&self) {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::Prune.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send FlushSpaceUpdate");
    }
    /// Rekey the PDDB. This can be a very long-running blocking operation that will definitely.
    /// interrupt normal user flow.
    pub fn rekey_pddb(&self, op: PddbRekeyOp) -> Result<()> {
        let mut buf = Buffer::into_buf(op)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend_mut(self.conn, Opcode::RekeyPddb.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error"))).map(|_| ())?;
        let result = buf.to_original::<PddbRekeyOp, _>()
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        match result {
            PddbRekeyOp::Success => Ok(()),
            PddbRekeyOp::AuthFail => Err(Error::new(ErrorKind::PermissionDenied, "Permission error")),
            PddbRekeyOp::UserAbort => Err(Error::new(ErrorKind::Interrupted, "User aborted operation")),
            PddbRekeyOp::InternalError => Err(Error::new(ErrorKind::Other, "Xous internal error")),
            PddbRekeyOp::VerifyFail => Err(Error::new(ErrorKind::InvalidData, "Data verification error while rekeying")),
            _ => Err(Error::new(ErrorKind::Unsupported, "Return code was never set")),
        }
    }

    pub fn run_test(&self, args: [u32; 4]) -> Result<(u32, u32)> {
        match send_message(self.conn,
            Message::new_blocking_scalar(Opcode::InternalTest.to_usize().unwrap(),
            args[0] as usize,
            args[1] as usize,
            args[2] as usize,
            args[3] as usize,
        )) {
            Ok(xous::Result::Scalar2(a, b)) => Ok((a as u32, b as u32)),
            Ok(xous::Result::Scalar1(a)) => Ok((a as u32, 0)),
            _ => Err(Error::new(ErrorKind::Other, "Return type not recognized")),
        }
    }

    /// Retrieve an entire dictionary of data in a single call. Will return data records up to but not
    /// over a total of `size_limit`. Keys that exceed the limit are still enumerated, but their
    /// data sections are `None`, instead of `Some(Vec::<u8>)`. Keys that are zero-length are also returned
    /// with a data section of `None`; check the `len` field of the PddbKeyRecord to determine which is which.
    /// Defaults to a size limit of up to 32k of bulk data returned, if it is not explicitly specified.
    pub fn read_dict(&self, dict: &str, basis: Option<&str>, size_limit: Option<usize>) -> Result<Vec::<PddbKeyRecord>> {
        // about the biggest we can move around in Precursor and not break heap.
        const MAX_BUFLEN: usize = 32 * 1024;
        // compromise between memory zeroing time and latency to send a message
        const DEFAULT_LIMIT: usize = 32 * 1024;
        /*
           The operation proceeds in two phases. All operations use the DictBulkRead opcode.

           The first phase establishes the read request. The caller first decides how many
           memory pages it will use to return data from the PDDB main thread, and allocates
           an appropriately-sized buffer.

           It then serializes a `PddbDictRequest` structure, where the token and index fields are disregarded.

           The main server then receives this, and if there are no tokens pending for `DictBulkRead`, it allocates
           a 128-bit token for this transaction and records it, locking out any other potential requests.
           If a token is already allocated, it responds immediately with an error code.

           The main server proceeds to serialize keys out of the dictionary into `PddbKeyRecord` structures.
           It will serialize exactly as many as can fit into the given buffer. In case the data is larger than
           the allocated buffer, the data will always be returned as `None`; in case the data is larger than
           the available space, the serialization stops, and the record is returned.

           Subsequent calls from the client to the main server requires a `PddbDictRequest` structure
           to be in the buffer, but only the "token" field is considered. The rest of the fields are disregarded;
           changing the requested dictionary or other parameters has no impact on the call trajectory.

           The PddbKeyRecords are manually packed into the memory messages with the following format:
           Data slice:
                - `code`: u32 containing the current transaction code
                - `starting_key_index` u32 of the current index-offset that the return data starts at. Used as a sanity check.
                - `len` u32 of number of records expected inside the return data structure.
                - `total`: u32 with total number of keys to be transmitted (shouldn't change over the life of the session)
                - `token`: [u32; 4] confirming or establishing the session token (shouldn't change over the life of the session)
                First record start at data[24]:
                    - `size` of archived struct: u32
                    - `pos` of archived struct, relative to the current start: u32
                    - data[u8] of length `size`
                Next record start at data[24 + size]:
                    - `size` of archived struct: u32
                    - `pos` of archived struct, relative to the current start: u32
                    - data[u8] of length `size`
                If `size` and `pos` are both zero, then, there are no more valid records in the return structure.

           The caller shall repeatedly call `DictBulkRead` until the `code` is `Last`, at which point the token
           is deleted from the server, freeing it to service a new transaction. The caller should then collate the
           results into the corresponding return vector.

           If the caller fails to drain all the data, the server eventually times out and forgets the token.
           The client will then get a "Busy" return code or "NotFound", depending on if they had fully initialized the
           request structure for the subsequent calls.
         */
        // allocate buffer, which is shuffled back and forth to the PDDB server
        let alloc_target = if let Some(size_limit) = size_limit {
            if size_limit < 4096 {
                4096
            } else {
                if size_limit > MAX_BUFLEN {
                    MAX_BUFLEN
                } else {
                    // rounds *down* to the nearest page size, since it is a "guaranteed not to exceed" limit
                    size_limit & !(4096 - 1)
                }
            }
        } else {
            DEFAULT_LIMIT
        };
        let mut msg_mem = xous::map_memory(
            None,
            None,
            alloc_target,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Likely OOM in allocating read_dict buffer");

        // setup the request arguments
        let mut request = PddbDictRequest {
            basis_specified: basis.is_some(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(basis.unwrap_or("")),
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(dict),
            key: xous_ipc::String::<KEY_NAME_LEN>::new(),
            index: 0,
            token: [0u32; 4],
            code: PddbRequestCode::BulkRead,
            bulk_limit: Some(size_limit.unwrap_or(DEFAULT_LIMIT)),
            key_count: 0,
            found_key_count: 0,
        };
        // main loop
        let mut ret = Vec::new();
        let mut check_total_keys;
        let mut check_key_index = 0;
        loop {
            let mut serializer = WriteSerializer::new(AlignedVec::new());
            let pos = serializer.serialize_value(&request).unwrap();
            let buf = serializer.into_inner();
            // Safety: `u8` contains no undefined values
            unsafe { msg_mem.as_slice_mut()[..buf.len()].copy_from_slice(buf.as_slice()) };
            // initiate the request: assemble a MemoryMessage
            let msg = xous::MemoryMessage {
                id: Opcode::DictBulkRead.to_usize().unwrap(),
                buf: msg_mem,
                offset: xous::MemoryAddress::new(pos),
                valid: xous::MemorySize::new(buf.len()),
            };
            // do a mutable lend to the server
            let (_pos, _valid) = match xous::send_message(
                self.conn,
                Message::MutableBorrow(msg)
            ) {
                Ok(xous::Result::MemoryReturned(offset, valid)) => (offset, valid),
                _ => return Err(Error::new(ErrorKind::Other, "Xous internal error"))
            };
            // unpack the return code. The result is not a single rkyv struct, it's hand-packed binary data. Unpack it.
            let mut index = 0;
            let mut header = BulkReadHeader::default();
            // Safety: `u8` contains no undefined values
            header.deref_mut().copy_from_slice(
                unsafe { &msg_mem.as_slice()[index..index + size_of::<BulkReadHeader>()] }
            );
            index += size_of::<BulkReadHeader>();
            match FromPrimitive::from_u32(header.code).unwrap_or(PddbBulkReadCode::InternalError) {
                PddbBulkReadCode::NotFound => {
                    return Err(Error::new(ErrorKind::NotFound, format!("Bulk Read: dictionary '{}' not found", dict)))
                }
                PddbBulkReadCode::Busy => {
                    return Err(Error::new(ErrorKind::TimedOut, "PDDB server busy or request timed out"))
                }
                PddbBulkReadCode::Streaming | PddbBulkReadCode::Last => {
                    // --- unpack the received data
                    // stash the token for future iterations
                    request.token = header.token;
                    check_total_keys = header.total;
                    if check_key_index != header.starting_key_index {
                        log::error!("local key index did not match remote key index: {}, {}", check_key_index, header.starting_key_index);
                    }
                    let mut key_count = 0;
                    log::debug!("header: {:?}", header);
                    while key_count < header.len {
                        if index + size_of::<u32>() * 2 > msg_mem.len() {
                            // quit if we don't have enough space to decode at least another two indices
                            break;
                        }
                        // Safety: `u32` contains no undefined values
                        let size = unsafe { u32::from_le_bytes(msg_mem.as_slice()[index..index + size_of::<u32>()].try_into().unwrap()) };
                        index += size_of::<u32>();
                        // Safety: `u32` contains no undefined values
                        let pos = unsafe { u32::from_le_bytes(msg_mem.as_slice()[index..index + size_of::<u32>()].try_into().unwrap()) };
                        index += size_of::<u32>();
                        log::trace!("unpacking message at {}({})", size, pos);
                        if size != 0 && pos != 0 {
                            //log::info!("extract archive: {}, {}, {}, {}", index, size, pos, msg_mem.len());
                            //log::info!("{:x?}", &msg_mem.as_slice::<u8>()[index..index + (size as usize)]);
                            let archived = unsafe {
                                archived_value::<PddbKeyRecord>(&msg_mem.as_slice()[index..index + (size as usize)], pos as usize)
                            };
                            //log::info!("increment index");
                            index += size as usize;
                            //log::info!("new index: {}", index);
                            let key = match archived.deserialize(&mut AllocDeserializer) {
                                Ok(r) => r,
                                Err(e) => {
                                    log::error!("deserialization error: {:?}", e);
                                    panic!("deserializer error");
                                },
                            };
                            //log::info!("pushing result");
                            ret.push(key);
                            key_count += 1;
                            check_key_index += 1;
                        } else {
                            // we encountered a nil field, stop decoding
                            log::info!("nil field");
                            break;
                        }
                    }
                    if key_count != header.len {
                        log::error!("key count did not match number in header {}, {}", key_count, header.len);
                    }
                    // if this is the last block, quit the loop
                    if header.code == (PddbBulkReadCode::Last as u32) {
                        break;
                    }
                }
                _ => {
                    return Err(Error::new(ErrorKind::Other, "Bulk read invalid return value"))
                }
            }
        }
        if check_total_keys != check_key_index {
            log::error!("Number of keys read does not match expected value: {}, {}", check_total_keys, check_key_index);
        }
        xous::unmap_memory(msg_mem).unwrap();
        Ok(ret)
    }

    /// Triggers a dump of the PDDB to host disk
    #[cfg(not(target_os = "xous"))]
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
    #[cfg(not(target_os = "xous"))]
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
    /// Forces a basis pruning, for testing the pruning and prune recovery code.
    #[cfg(not(target_os = "xous"))]
    pub fn dbg_prune(&self) -> Result<()> {
        let ipc = PddbDangerousDebug {
            request: DebugRequest::Prune,
            dump_name: xous_ipc::String::new(),
        };
        let buf = Buffer::into_buf(ipc)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend(self.conn, Opcode::DangerousDebug.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error"))).map(|_| ())
    }
    /// Turns on debug spew
    #[cfg(not(target_os = "xous"))]
    pub fn dbg_set_debug(&self) -> Result<()> {
        let ipc = PddbDangerousDebug {
            request: DebugRequest::SetDebug,
            dump_name: xous_ipc::String::new(),
        };
        let buf = Buffer::into_buf(ipc)
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error")))?;
        buf.lend(self.conn, Opcode::DangerousDebug.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "Xous internal error"))).map(|_| ())
    }

    #[cfg(all(feature="pddbtest", feature="autobasis"))]
    pub fn basis_testing(&self, config: &[Option<bool>; 32]) {
        let mut op = 0;
        let mut valid = 0;
        for (index, &c) in config.iter().enumerate() {
            if let Some(opt) = c {
                valid |= 1 << index;
                op |= (if opt {1} else {0}) << index;
            }
        }
        send_message(self.conn,
            Message::new_scalar(Opcode::BasisTesting.to_usize().unwrap(), op, valid, 0, 0)
        ).expect("couldn't send basis test message");
    }
}

impl Drop for Pddb {
    fn drop(&mut self) {
        if let Some(cb_sid) = self.cb.take() {
            let handle = self.cb_handle.take().unwrap(); // we guarantee this is always set when cb is set
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
