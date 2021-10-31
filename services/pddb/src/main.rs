#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::*;

/// PDDB - Plausibly Deniable DataBase
///
/// Glossary:
/// Basis - a unionizeable dictionary that can be unlocked with a password-protected key.
/// Dictionary - similar to a "volume" on a typical filesystem. Contains a group of k/v pairs
///   that share attributes such as access permissions
/// Key - a unique string that identifies a piece of data in the PDDB
/// Path - Rust's notion of how to locate a file, except we interpret it as a PDDB key, where
///   the "root" directory is the dictionary, and the remainder is the "key"
/// Value - the value associated with a key, isomorphic ot a "file" in Rust
/// Open Basis - A Basis that is decryptable and known to the system.
/// Closed Basis - A Basis that is not decryptable. It's unknown to the system and potentially treated
///   identically to free disk space, e.g., it could be partially overwritten.
///
/// A `Path` like the following is deconstructed as follows by the PDDB:
///
///  Dictionary
///    |         Key
///    |          |
///  --+- --------+---------------------
/// /logs/matrix/alice/oct30_2021/bob.txt
///
/// It could equally have an arbitrary name like "logs/Matrix - alice to bob Oct 30 2021";
/// as long as the string that identifies a Key is unique, it's stored in the database
/// all the same. Any valid utf-8 unicode characters are acceptable.
///
/// Likewise, something like this:
/// /settings/wifi/Kosagi.json
/// Would be deconstructed into the "settings" dictionary with a key of wifi/Kosagi.json.
///
/// General Operation:
///
/// The initial Basis is known as the "System" Basis. It's a low-security framework basis
/// onto which all other Basis overlay. The initial set of Dictionaries, along with their
/// Key/Value pairs, are available to any and all processes.
///
/// Each new Basis opened will contain zero or more dictionaries. If the dictionary within
/// the new Basis has the same name as an existing dictionary, the Keys are searched in
/// the reverse order of opening. In other words, a new Basis can temporarily override or
/// mask existing Keys. When updating a Key, one may specify the following modes of
/// operation:
///
///  - UpdateLatest: updates only the copy in the latest Basis to be opened
///  - UpdateOpened: updates copies in other currently opened Basis
///
/// Note that `UpdateOpened` is dangerous in the sense that if you have a Basis that is
/// is currently Closed, it cannot update copies in the closed Basis. Thus any global
/// update of database schema requires a user to open any and all knows Basis so that
/// synchronization can be maintained.
///
/// Furthermore, it is possible for a Basis to be closed on a "File" that is currently open.
/// In this case, two things happen:
///  - If a notification callback is registered, it's pinged by the PDDB. The notification
///    of closure callback is an additional feature to the typical Rust File interface.
///  - If the client attempts to read or write to any keys that span a Basis modification,
///    the now-ambiguous key operation will return a `BrokenPipe` error to the caller.


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

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let pddb_sid = xns.register_name(api::SERVER_NAME_PDDB, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", pddb_sid);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(pddb_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(pddb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::KeyRequest) => {
                // placeholder
            }
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                /* pddb.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                pddb.resume(); */
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(pddb_sid).unwrap();
    xous::destroy_server(pddb_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
