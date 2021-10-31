#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
pub mod frontend;
pub use frontend::*;

use xous::{CID, send_message};
use num_traits::*;


/// This constant maps onto a region that's "unused" by Xous, and claimable by user
/// processes.
const PDDB_BACKING_BASE: usize = 0x8000_0000;
/// This designates the largest contiguous extent that we could allocate for a file.
/// The backing isn't allocated -- it is merely reserved. Accesses to the backing trigger
/// page faults that are handled by the PDDB, and just the pages actively being dereferenced is
/// swapped into physical memory on demand. This does put a pretty hard upper limit on file sizes
/// on a 32-bit system, but PDDB is coded such that we could extend to a 64-bit system and
/// increase this limit my changing the constants here.
const PDDB_BACKING_SIZE: usize = 0x4000_0000;

fn handle_exception(exception: xous::Exception) -> isize {
    use xous::Exception::*;
    match exception {
        IllegalInstruction(epc, instruction) => {
            println!(
                "Caught illegal instruction {:08x} at {:08x}, just skipping it...",
                epc, instruction
            );
            4
        }
        InstructionAddressMisaligned(epc, addr) => {
            println!(
                "Misaligned instruction at {:08x} ({:08x}), trying to nudge forward a bit",
                epc, addr
            );
            1
        }
        InstructionAccessFault(epc, addr)
        | LoadAccessFault(epc, addr)
        | StoreAccessFault(epc, addr) => {
            panic!(
                "Access fault of some sort: ({:08x} {:08x}): {:?}",
                epc, addr, epc
            );
        }
        LoadAddressMisaligned(epc, addr) | StoreAddressMisaligned(epc, addr) => {
            println!(
                "Load was misaligned ({:08x} @ {:08x}). Just skipping the instruction and hoping nobody notices.", epc, addr
            );
            4
        }
        InstructionPageFault(epc, addr) | LoadPageFault(epc, addr) | StorePageFault(epc, addr) => {
            println!("Memory access fault at address 0x{:08x} (pc is at 0x{:08x}), allocating a new page", addr, epc);
            let base = addr & !4095;
            let size = 4096;
            xous::map_memory(
                None,
                xous::MemoryAddress::new(base),
                size,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map memory");
            // Retry the instruction
            0
        }
        Unknown(a1, a2, a3) => panic!("unknown exception: {:08x} {:08x} {:08x}", a1, a2, a3),
    }
}

pub struct Pddb {
    conn: CID,
    backing: MemoryRange,
}
impl Pddb {
    pub fn new(xns: &xous_names::XousNames) -> core::result::Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_PDDB).expect("Can't connect to Pddb server");

        xous::syscall::set_exception_handler(handle_exception).unwrap();

        let backing = unsafe {MemoryRange::new(
            0x8000_0000,
            0x1000_0000,
        ).expect("couldn't reserve pages for file backing region") };

        Ok(Pddb {
            conn,
            backing
        })
    }

    pub(crate) fn conn(&self) -> CID {
        self.conn
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Pddb {
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
use std::io::{Error, ErrorKind, Result};

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

