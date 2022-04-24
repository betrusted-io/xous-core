#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/// # PDDB - Plausibly Deniable DataBase
///
/// ## Glossary:
/// * Basis - a unionizeable dictionary that can be unlocked with a password-protected key.
/// * Dictionary - similar to a "volume" on a typical filesystem. Contains a group of k/v pairs
///   that share attributes such as access permissions
/// * Key - a unique string that identifies a piece of data in the PDDB
/// * Path - Rust's notion of how to locate a file, except we interpret it as a PDDB key, where
///   the "root" directory is the dictionary, and the remainder is the "key"
/// * Value - the value associated with a key, isomorphic ot a "file" in Rust
/// * Open Basis - A Basis that is decryptable and known to the system.
/// * Closed Basis - A Basis that is not decryptable. It's unknown to the system and potentially treated
///   identically to free disk space, e.g., it could be partially overwritten.
/// * FSCB - Free Space Commit Buffer. A few pages allocated to tracking a subset of free space,
///   meant to accelerate PDDB operations. Creates a side-channel that can reveal that some activity
///   has happened, but without disclosing what and why. Contains FastSpace and SpaceUpdate records.
///   Frequently updated, so the buffer is slightly oversized, and which sector is "hot" is randomized
///   for wear-levelling.
/// * MBBB - Make Before Break Buffer. A set of randomly allocated pages that are a shadow copy
///   of a page table page. If any data exists, its contents override those of a corrupted page table.
/// * FastSpace - a collection of random pages that are known to be empty. The number of pages in FastSpace
///   is reduced from the absolute amount of free space available by at least a factor of FSCB_FILL_COEFFICIENT.
/// * SpaceUpdate - encrypted patches to the FastSpace table. The FastSpace table is "heavyweight", and would
///   be too expensive to update on every page allocation, so SpaceUpdate is used to patch the FastSpace table.
///
/// A `Path` like the following is deconstructed as follows by the PDDB:
///
/// ```Text
///  Dictionary
///    |         Key
///    |          |
///  --+- --------+---------------------
///  logs:matrix/alice/oct30_2021/bob.txt
/// ```
///
/// It could equally have an arbitrary name like `logs:Matrix - alice to bob Oct 30 2021`;
/// as long as the string that identifies a Key is unique, it's stored in the database
/// all the same. Any valid utf-8 unicode characters are acceptable.
///
/// Likewise, something like this:
/// `settings:wifi/Kosagi.json`
/// Would be deconstructed into the "settings" dictionary with a key of `wifi/Kosagi.json`.
///
/// ## Code Organization:
/// Accurate as of Nov 2021. May be subject to charge, ymmv.
///
/// ### `frontend.rs`
/// Defines a set of modules that plug the PDDB into applications (in particular, they
/// attemp to provide a Rust-compatible `read`/`write`/`open` abstraction layer,
/// torturing the notion of `Prefix`, `Path` and `File` in Rust to fit the basis/dict/key
/// format of the PDDB).
///
/// This is the `lib`-facing set of operations.
///
/// ### `backend.rs`
/// Defines a set of modules that implement the PDDB itself. This is the hardware-facing set
/// of operations.
///
/// #### `basis.rs`
/// The set of known `Basis` are tracked in the `BasisCache` structure. This is the "entry point"
/// for most operations on the PDDB, and thus most externally-visible API calls will be revealed
/// on that structure; in fact many calls on that object are just pass-through of lower level calls.
///
/// A `BasisCach`e consists of one or more `BasisCacheEntries`. This structure manages one or more
/// `DictCacheEntry` within a `BasisCacheEntry`'s dictionary cache.
///
/// #### `dictionary.rs`
/// The `DictCacheEntry` is defined in this file, along with the on-disk `Dictionary`
/// storage stucture. Most of the methods on `DictCacheEntry` are concerned with managing
/// keys contained within the dictionary cache.
///
/// #### `keys.rs`
/// The `KeyCacheEntry` is defined in this file. It's a bookkeeping record for the
/// `DictCacheEntry` structure. Keys themselves are split into metadata and data
/// records. The `KeyDescriptor` is the on-disk format for key metadata. The key
/// storage is either described in `KeySmallPool` for keys less than one `VPAGE_SIZE`
/// (just shy of 4kiB), or written directly to disk as fully allocated blocks of
/// `VPAGE_SIZE` for keys larger than one `VPAGE_SIZE` unit.
///
/// ## Threat model:
/// The user is forced to divulge "all the Basis passwords" on the device, through
/// coercion, blackmail, subpoena, customs checkpoint etc. The adversary has physical
/// access to the device, and is able to take a static disk image; they may even have
/// the opportunity to take several disk images over time and diff the images.
/// The adversary may be able to decrypt the root key of the cryptographic enclave.
/// The adversary may also be able to observe the contents of encrypted data within the
/// "system" basis, which includes some of the bookkeeping information for the PDDB,
/// as the user will have at least been forced to divulge the system basis password as
/// this is a password that every device requires and they cannot deny its existence.
///
/// Under these conditions, it should be impossible for the adversary to conclusively prove or
/// disprove that every Basis password has been presented and unlocked for inspection
/// through forensic analysis of the PDDB alone (significantly, we cannot prevent disclosure
/// by poorly constructed end-user applications storing things like "last opened Basis"
/// convenience lists, or if the user themselves wrote a note referring to a secret
/// Basis in a less secret area). Furthermore, if a device is permanently seized by
/// the adversary for extensive analysis, any Basis whose password that has not been
/// voluntarily disclosed should be "as good as deleted".
///
/// The PDDB also cannot protect against key loggers or surveillance cameras recording
/// key strokes as the user operates the device. Resistance to key loggers is instead
/// a problem handled by the OS, and it is up to the user to not type secret passwords
/// in areas that may be under camera surveillance.
///
/// ## Auditor Notices:
/// There's probably a lot of things the PDDB does wrong. Here's a list of some things
/// to worry about:
///  - The device RootKeys shares one salt across all of its keys, instead of a per-key salt.
///    Currently there are only two keys in the RootKey store sharing the one salt. The concern
///    is that a migration to an ASIC base design would make eFuse bits scarce, so, the initial
///    draft of the RootKeys shares one salt to maximize key capacity. The per-key salt is slightly
///    modified by adding the key index to the salt. This isn't meant to be a robust mitigation:
///    it just prevents a naive rainbow attack from re-using its table.
///  - The bcrypt() implementation is vendored in from a Rust bcrypt crate. It hasn't been audited.
///  - The COST of 7 for bcrypt is relatively low by today's standards (should be 10). However,
///    we can't raise the cost to 10 because our CPU is slower than most modern x86 devices. There is
///    an open issue to try to improve this with hardware acceleration. The mitigation is to use
///    a longer passphrase instead of a 12 or 14-character password.
///  - The RootKey is used to decrypt a locally stored System Basis key. The key is encrypted using
///    straight AES-256 with no authentication.
///  - Secret basis keys are not stored anywhere on the device. They are all derived from a password
///    using bcrypt. The salt for the password is drawn from a "salt pool", whose index is derived from
///    a weak hash of the password itself. This means there is a chance that a salt gets re-used. However,
///    we do not store per-password salts because the existence of the salt would betray the existence of
///    a password.
///  - Disk blocks are encrypted & authenticated on a page-by-page basis using AES-GCM-SIV, with a 256-bit key.
///  - Page table entries and space update entries are salted & hinted using a 64-bit nonce + weak checksum.
///    There is a fairly high chance of a checksum collision, thus PTE decrypts are regarded as advisory and
///    not final; the PTE is not accepted as authentic until the AES-GCM-SIV behind it checks out. Free space
///    entries have no such protection, which means there is a slight chance of data loss due to a checksum
///    collision on the free space entries.
///
/// ## General Operation:
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
///
/// ## General flash->key structure
///
/// The basic unit of memory is a page (4k). Keys shorter than 4k will be packed into a single
/// page, but no allocation will ever be smaller than 4k due to the erase block size of the FLASH.
///
/// Because the flash is always encrypted, there is also no such thing as "incremental writing of data"
/// to a page, as the presence of free space (all 1's regions) are considered a side-channel. Thus,
/// the PDDB relies heavily on caching data structures in RAM, with periodic commits of encrypted data
/// back to the flash. This means it is also very easy to lose data when the system has a panic and
/// reboots ungracefully. Thus, the structure of data at rest is journaled, so that we have some
/// chance of recovering an older snapshot of the data if there is an ungraceful power-down event.
///
/// I believe this relative unreliability compared to more traditional storage is just a fact of life for
/// an encrypted, plausibly deniable filesystem, as you have to trade-off the side channel of partially
/// erased blocks versus the finite write lifetime of flash storage.
///
/// When a Basis is created, it starts with 64k of space allocated to it. Most of this is blank (but encrypted
/// in flash), but it is important in a PD filesystem to claim some freespace for day-to-day operations. This
/// is because when you need to allocate more data, you have to make a guess as to what is free, and what
/// previously allocated but currently denied to exist (locked Basis look identical to free space when locked).
///
/// Each Dictionary within a Basis starts with a 4k allocation as well, although most of it is free space.
/// Keys are then populated into dictionaries, with additional pages allocated to dictionaries as the keys
/// grow.
///
/// Blocks on disk are mapped into a virtual PDDB space via a page table. The virtual PDDB space is 64-bit.
/// Each Basis gets its own 48-bit address space, allowing for up to 64k Basis.
/// The PDDB page table guarantees that no matter the order of allocation in FLASH, a Basis can linearly
/// access its blocks through a page table lookup. A reverse-lookup structure is constructed on boot
/// by scanning the entire PDDB, and creating a HashMap of (basis, offset) -> pddb_addr tuples.
///
/// Physical addresses can be 32 or 64 bit based upon the specific implementation. The physical address
/// type is defined in types.rs. For Precursor, it's defined as a u32, which limits the physical address
/// size to 32 bits. This is important because Precursor operates out of a very small amount of RAM,
/// and doubling the size of the overhead bookkeeping structures to handle disk sizes that are well
/// beyond the current implementation has a real impact on the system's free memory footprint. However,
/// the PDDB is coded in a way such that one "should" be able to swap out the u32 for a u64 in the
/// "newtype" definition for a PhysAddr, and the system will work.
///
/// Page table entries don't define themselves with a Basis -- every time a Basis is opened, the entire
/// table must be scanned, brute-force decrypting every entry against the Basis key, and seeing if the
/// checksum in the table entry matches the address. If there's a match, then that entry is recorded as
/// belonging to a given Basis.
///
/// When a system boots, it starts with two default Basis: FastSpace, and System. These Basis are protected
/// with the "device unlock" key. System is a set of low-security data that are used
/// to configure "everyday details". FastSpace Basis is a special-case Basis that is not structured as
/// a Key/Value store. Instead, it tracks a small amount of pre-allocated "free" space, that can be
/// pulled from to grow Dictionaries without requesting that the user unlock every known Basis in the system
/// to ensure none of the valuable data is overwritten.
///
/// The .FastSpace basis should never be equal to the total amount of free space in the system, as it
/// would betray the actual amount of data available and destroy plausible deniability. The system can
/// operate with .FastSpace set to the nil set, but it would also operate without confidentiality of hidden
/// Basis, because in order to allocate new blocks it must prompt the user to enter all known Basis passwords,
/// to avoid accidentally overwriting data that should be retained (as locked Basis would appear as free space
/// and risk being overwritten). However, in order to accelerate bookkeeping, the .FastSpace basis operates
/// with an encrypted record that rotates through a set of "clean" pages (that is, pages that have been set
/// to all 1's) in a circular buffer basis. The state of where the .FastSpace record is in the "clean" pages
/// disclose nothing other than the fact that the system has been used.
///
/// ## Basis Deniability
///
/// The existence of a Basis itself must be confidential; if we stored a list of encrypted Basis passwords
/// somewhere, a rubber-hose attacker would simply need to count the number of encrypted entries and commence
/// the beatings until the number of passwords that have been disclosed under duress match the number of
/// encrypted password entries.
///
/// Herein lies a challenge. Standard password storage techniques demand that each password be stored
/// with a unique salt. The existence of this salt betrays that a password must, in essence, exist.
/// There are at least two methods to counter this:
///
/// 1. Dummy salt entries. In this approach, each system is initialized with a random set of additional, dummy
///    salt/encrypted password combos. In the case of a rubber-hose attack, the attacker cannot know precisely
///    when to stop the beatings, as the few remaining entries could be dummies with absolutely no meaning.
/// 2. A single salt entry. In this approach, a single shared salt is used by /all/ passwords, and there is no
///    encrypted password list stored; instead, the the password as presented is directly used to decrypt
///    the Basis page table and if no valid entries are found we may conclude that the password provided has a typo.
///
/// The advantage of (1) is that the usage of salt falls precisely within the traditional cryptographic
/// specification of bcrypt(). The disadvantages of (1) include: one still has to have a method to match
/// a presented password to a given salt entry. In a traditional user/pass login database, the username is
/// the plaintext key to match the salt. In this case, we would use a nickname used to refer to each Basis
/// to correlate to its salt entry. Thus, we'd have to come up with "garbage" plaintext nicknames
/// for a Basis that are not trivial for a rubber-hose attacker to dismiss as chaffe. I think this is Hard.
/// The alternative is to "brute force" the entire list of salts, guessing each one in sequence until one
/// is found to decrypt entries in the Basis page table. The problem with this is that bcrypt is designed to
/// be computationally slow, even for valid passwords. The complexity parameter is chosen so that it takes
/// about 1 second per iteration through the password function. Note that a faster CPU does not solve this
/// problem: if you CPU gets faster, you should increase your complexity, so one is always suffering the 1
/// second penalty to try a password. This means brute forcing a list of salts necessarily incurs at
/// least a 1 second penalty per entry; thus each dummy basis adds a 1 second minimum overhead to unlocking
/// a new Basis database. This puts a downward pressure on the number of dummies to be included; however, the
/// number of dummies needs to be at least as large as the number of Basis we wish to plausibly deny. This
/// creates a negative incentive to using plausible deniability.
///
/// The advantage of (2) is that when a user presents a password, we only need to run the bcrypt routine
/// once, and then we can immediately start checking the Basis page table for valid entries. The disadvantage
/// of (2) is that we are re-using a salt across all of a given user's passwords. Note that the salt itself
/// is from a TRNG, so between user devices, the salt performs its role. However, it means that an attacker
/// can generate a single rainbow table specific to a given user to reverse all of their passwords; and furthermore,
/// re-used passwords are trivially discoverable with the rainbow table.
///
/// Approach (1) is probably what most crypto purists would adopt: you, dear user, /should/ understand that
/// cryptography is worth the wait. However, a second principle states that "users always pick convenience over
/// security". A 15-second wait is an eternity in the UX world, and would act as an effective deterrent to any
/// user ever using the PD function because it is too slow. Thus for v1 of the PDDB, we're going to try approach
/// (2), where a common salt is used across all the PDDB passwords, but the salt is /not/ re-used between devices.
/// This choice diminishes the overall security of the system in the case that a user chooses weak passwords, or re-uses
/// passwords, but in exchange for a great improvement in responsivity of the implementation.
///
/// The final implementation uses a slight mod on (2), where the 128-bit common salt stored on disk is XOR'd
/// with the user-provided "basis name". Users are of course allowed to pick crappy names for their basis, and
/// re-use the names, but hopefully this adds a modicum of robustness against rainbow table attacks.
///
///
/// ## Basis Unlock Procedure
///
/// Each Basis has a name and a passcode associated with it. The default Basis name is `.System`.
/// In addition to that, a `.FastSpace` structure is unlocked along side the default Basis.
/// These are both associated with the default system unlock passcode.
///
/// A newly created Basis will request a name for the Basis, and a password. It is a requirement
/// that the combination of `(name, password)` be unique; this property is enforced and a system will
/// reject attempts to create identically named, identially passworded Basis. However, one can have
/// same-named Basis that have a different password, or diffently-named Basis with the same password
/// (this is generally not recommended, but it's not prohibited).
///
/// The `name` field is XOR'd with the device-local `salt` field to form the salt for the password,
/// and the plaintext of the password itself is used as the password which is fed into the bcrypt()
/// algorithm to generate a 192-bit encrypted password, which is expanded to 256-bits using SHA-512/256,
/// and then used as the AES key for the given `(name, password)` Basis.
///
///
/// ## Journaling
///
/// A major issue with the implementation is that even a small change in a data structure turns into
/// the mutation of at least an entire sector of data, with a sector being 4k. The reason is that
/// we can't take advantage of FLASH memory's property where 1's (erased) can be set to 0's on a byte-by-byte
/// basis: in order to mask free space signatures, the entire storage area is filled with random bytes.
/// Furthermore, any update to a part of data within a block should necessarily propagate to the entire
/// block changing, in order to avoid attacks where observations on which portion of ciphertext has changed
/// leading to a definitive conclusion about the state of the database records. Therefore, a large portion
/// of the database data will have to persist in RAM, and be updated only at regular, but widely-spaced-out
/// (to avoid FLASH wear-out), intervals.
///
/// In the case that power is lost during an update, the system uses a 32-bit `journal_rev` number at the
/// top of every major database structure. If two competing copies of data are found, the one with the
/// highest `journal_rev` number wins. It may be possible to overflow the 32-bit number, so at some point a
/// garbage collection step needs to be made where any errant, lower journal rev blocks are overwritten
/// with random data; all the journal revs are confirmed to be the same; and then all set to 0 again. Power
/// should not be lost during this process; but it should be rare, and likely never needed.
///
/// When writing to disk, a write-then-erase method is used:
///   1. Required blocks for the update are taken out of the .FastSpace pool
///   2. Detached data sections of structural records are written first (only database structural records can have detached data;
///      all stored data have individual journal revision fields associated with them), and into blocks
///      taken out of the .FastSpace pool.
///   3. The head of the updated structural record is written, with the new latest journal rev noted.
///   4. The page table is updated using the procedure outlined below.
///   5. The old blocks are erased and overwritten with random data.
///   6. The .FastSpace pool is updated with the newly freed blocks
///
/// When updating the page table, it's important not to lose an entire page's worth of entries in case
/// power is lost when updating it. The following process is used to update a page table entry:
///   0. At the end of the page table there is a circular buffer of blank pages (they are truly blank,
///      but this will not leak metadata about the contents of the PDDB, as it only does bookkeeping
///      on page table updates as a whole). This is known as the make-before-break buffer (mbbb).
///   1. The previous mbbb entry is erased, if there is any, and the next entry is noted.
///   2. The new page table entry is written into the mbbb
///   3. The old page table entry in the page table itself is erased.
///   4. The old page table entry is populated with the updated page table records.
///
/// Detecting and Repairing a broken page table:
///   The special case of long run's of 1's (more than 32 bits in a row) is checked during the page table scan.
///   If a long run of 1's is detected, then a suspected corrupted page table update is flagged, and for that page the data
///   is pulled from the mbbb record.
///
/// ## Precursor's Implementation-Specific Flash Memory Organization:
///
/// ```Text
///   offset from |
///   pt_phys_base|  contents  (example for total PDDB len of 0x6f8_0000 or 111 MiB)
///   ------------|---------------------------------------------------
///   0x0000_0000 |  virtual map for page at (0x0000 + data_phys_base)
///   0x0000_0010 |  virtual map for page at (0x1000 + data_phys_base)
///   0x0000_0020 |  virtual map for page at (0x2000 + data_phys_base)
///    ...
///   0x0006_F7F0 |  virtual map for page at (0x06F7_F000 + data_phys_base)
///   0x0006_F800 |  unused
///    ...
///   0x0007_0000 |  key page
///    ...
///   0x0007_1000 |  mbbb start (example of 10 pages)
///    ...
///   0x0007_B000 |  fscb start (example of 10 pages)
///    ...
///   0x0008_5000 |  data_phys_base - start of basis + dictionary + key data region
/// ```

// historical note: 389 hours, 11 mins elapsed since the start of the PDDB coding, and the first attempt at a hardware test -- as evidenced by the uptime of the hardware validation unit.

extern crate bitflags;
extern crate bitfield;

mod api;
use api::*;
mod backend;
use backend::*;
mod ux;
use ux::*;
mod menu;
use menu::*;

#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod tests;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
#[allow(unused_imports)]
use tests::*;

use num_traits::*;
use xous::{send_message, Message, msg_blocking_scalar_unpack};
use xous_ipc::Buffer;
use core::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::collections::HashMap;
use std::io::ErrorKind;
use core::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use locales::t;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct BasisRequestPassword {
    db_name: xous_ipc::String::<{crate::api::BASIS_NAME_LEN}>,
    plaintext_pw: Option<xous_ipc::String::<{crate::api::PASSWORD_LEN}>>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PasswordState {
    Correct,
    Incorrect,
    Uninit,
}

#[derive(Debug)]
struct TokenRecord {
    pub dict: String,
    pub key: String,
    pub basis: Option<String>,
    pub alloc_hint: Option<usize>,
    pub conn: Option<xous::CID>, // callback connection, if one was specified
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let pddb_sid = xns.register_name(api::SERVER_NAME_PDDB, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", pddb_sid);

    log::trace!("ready to accept requests");

    // shared entropy cache across all process-local services (it's more efficient to request entropy in blocks from the TRNG)
    let entropy = Rc::new(RefCell::new(TrngPool::new()));

    // for less-secured user prompts (everything but password entry)
    let modals = modals::Modals::new(&xns).expect("can't connect to Modals server");

    // OS-specific PDDB driver
    let mut pddb_os = PddbOs::new(Rc::clone(&entropy));
    // storage for the basis cache
    let mut basis_cache = BasisCache::new();
    // storage for the token lookup: given an ApiToken, return a dict/key/basis set. Basis can be None or specified.
    let mut token_dict = HashMap::<ApiToken, TokenRecord>::new();

    // mount poller thread
    let is_mounted = Arc::new(AtomicBool::new(false));
    let _ = thread::spawn({
        let is_mounted = is_mounted.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let poller_sid = xns.register_name(api::SERVER_NAME_PDDB_POLLER, None).expect("can't register server");
            loop {
                let msg = xous::receive_message(poller_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PollOp::Poll) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        if is_mounted.load(Ordering::SeqCst) {
                            xous::return_scalar(msg.sender, 1).unwrap();
                        } else {
                            xous::return_scalar(msg.sender, 0).unwrap();
                        }
                    }),
                    Some(PollOp::Quit) => {
                        xous::return_scalar(msg.sender, 0).unwrap();
                        break;
                    }
                    None => log::warn!("got unrecognized message: {:?}", msg),
                }
            }
            xous::destroy_server(poller_sid).ok();
        }
    });

    // run the CI tests if the option has been selected
    #[cfg(all(
        not(any(target_os = "none", target_os = "xous")),
        feature = "ci"
    ))]
    ci_tests(&mut pddb_os).map_err(|e| log::error!("{}", e)).ok();

    if false { // this will re-init the PDDB and do a simple key query. Really useful only for early shake-down testing, eliminate this reminder stub once we have some confidence in the code
        hw_testcase(&mut pddb_os);
    }

    // our very own password modal. Password modals are precious and privately owned, to avoid
    // other processes from crafting them.
    let pw_sid = xous::create_server().expect("couldn't create a server for the password UX handler");
    let pw_cid = xous::connect(pw_sid).expect("couldn't connect to the password UX handler");
    let pw_handle = thread::spawn({
        move || {
            password_ux_manager(
                xous::connect(pddb_sid).unwrap(),
                pw_sid
            )
        }
    });
    // our menu handler
    let my_cid = xous::connect(pddb_sid).unwrap();
    let _ = thread::spawn({
        let my_cid = my_cid.clone();
        move || {
            pddb_menu(my_cid);
        }
    });
    // spawn a delayed mount command, shortly after boot. There's too much going on at boot, and it blocks other things from coming up.
    let _ = thread::spawn({
        let my_cid = my_cid.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            tt.sleep_ms(4000).unwrap(); // wait after boot before attempting to mount, to let the boot screen finish redrawing
            send_message(my_cid,
                Message::new_blocking_scalar(Opcode::TryMount.to_usize().unwrap(), 0, 0, 0, 0)
            ).expect("couldn't send mount request");
        }
    });
    // main server loop
    let mut key_list = Vec::<String>::new(); // storage for key lists
    let mut key_token: Option<[u32; 4]> = None;
    let mut dict_list = Vec::<String>::new(); // storage for dict lists
    let mut dict_token: Option<[u32; 4]> = None;

    // the PDDB resets the hardware RTC to a new random starting point every time it is reformatted
    // it is the only server capable of doing this.
    let time_resetter = xns.request_connection_blocking(crate::TIME_SERVER_PDDB).unwrap();

    // track processes that want a notification of a mount event
    let mut mount_notifications = Vec::<xous::MessageSender>::new();

    // register a suspend/resume listener
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Early), &xns,
        Opcode::SuspendResume as u32, my_cid).expect("couldn't create suspend/resume object");
    loop {
        let mut msg = xous::receive_message(pddb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                basis_cache.suspend(&mut pddb_os);
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
            }),
            Some(Opcode::IsMounted) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if basis_cache.basis_count() > 0 { // if there's anything in the cache, we're mounted.
                    xous::return_scalar(msg.sender, 1).expect("couldn't return scalar");
                } else {
                    mount_notifications.push(msg.sender); // defer response until later
                }
            }),
            Some(Opcode::TryMount) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if basis_cache.basis_count() > 0 {
                    xous::return_scalar(msg.sender, 1).expect("couldn't return scalar");
                } else {
                    if !pddb_os.rootkeys_initialized() {
                        // can't mount if we have no root keys
                        xous::return_scalar(msg.sender, 0).expect("could't return scalar");
                    } else {
                        match ensure_password(&modals, &mut pddb_os, pw_cid) {
                            PasswordState::Correct => {
                                if try_mount_or_format(&modals, &mut pddb_os, &mut basis_cache, PasswordState::Correct, time_resetter) {
                                    is_mounted.store(true, Ordering::SeqCst);
                                    for requester in mount_notifications.drain(..) {
                                        xous::return_scalar(requester, 1).expect("couldn't return scalar");
                                    }
                                    assert!(mount_notifications.len() == 0, "apparently I don't understand what drain() does");
                                    xous::return_scalar(msg.sender, 1).expect("couldn't return scalar");
                                } else {
                                    xous::return_scalar(msg.sender, 0).expect("couldn't return scalar");
                                }
                            },
                            PasswordState::Uninit => {
                                if try_mount_or_format(&modals, &mut pddb_os, &mut basis_cache, PasswordState::Uninit, time_resetter) {
                                    is_mounted.store(true, Ordering::SeqCst);
                                    for requester in mount_notifications.drain(..) {
                                        xous::return_scalar(requester, 1).expect("couldn't return scalar");
                                    }
                                    assert!(mount_notifications.len() == 0, "apparently I don't understand what drain() does");
                                    xous::return_scalar(msg.sender, 1).expect("couldn't return scalar");
                                } else {
                                    xous::return_scalar(msg.sender, 0).expect("couldn't return scalar");
                                }
                            },
                            // user aborted procedure
                            _ => xous::return_scalar(msg.sender, 0).expect("couldn't return scalar"),
                        }
                    }
                }
            }),
            Some(Opcode::ListBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut list_ipc = buffer.to_original::<PddbBasisList, _>().unwrap();
                let basis_list = basis_cache.basis_list();
                for (src, dst) in basis_list.iter().zip(list_ipc.list.iter_mut()) {
                    dst.clear();
                    write!(dst, "{}", src).expect("couldn't write basis name");
                }
                list_ipc.num = basis_list.len() as u32;
                buffer.replace(list_ipc).unwrap();
            }
            Some(Opcode::LatestBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut mgmt = buffer.to_original::<PddbBasisRequest, _>().unwrap();
                if let Some(name) = basis_cache.basis_latest() {
                    mgmt.name.clear();
                    write!(mgmt.name, "{}", name).expect("couldn't write basis name");
                    mgmt.code = PddbRequestCode::NoErr;
                } else {
                    mgmt.code = PddbRequestCode::NotMounted;
                }
                buffer.replace(mgmt).unwrap();
            }
            Some(Opcode::CreateBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut mgmt = buffer.to_original::<PddbBasisRequest, _>().unwrap();
                match mgmt.code {
                    PddbRequestCode::Create => {
                        let request = BasisRequestPassword {
                            db_name: mgmt.name,
                            plaintext_pw: None,
                        };
                        let mut buf = Buffer::into_buf(request).unwrap();
                        buf.lend_mut(pw_cid, PwManagerOpcode::RequestPassword.to_u32().unwrap()).unwrap();
                        let ret = buf.to_original::<BasisRequestPassword, _>().unwrap();
                        if let Some(pw) = ret.plaintext_pw {
                            match basis_cache.basis_create(&mut pddb_os, mgmt.name.as_str().expect("name is not valid utf-8"), pw.as_str().expect("password was not valid utf-8")) {
                                Ok(_) => mgmt.code = PddbRequestCode::NoErr,
                                _ => mgmt.code = PddbRequestCode::InternalError,
                            }
                        } else {
                            mgmt.code = PddbRequestCode::InternalError;
                        }
                    }
                    _ => {
                        mgmt.code = PddbRequestCode::InternalError;
                    }
                }
                buffer.replace(mgmt).unwrap();
            }
            Some(Opcode::OpenBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut mgmt = buffer.to_original::<PddbBasisRequest, _>().unwrap();
                match mgmt.code {
                    PddbRequestCode::Open => {
                        let mut finished = false;
                        while !finished {
                            let request = BasisRequestPassword {
                                db_name: mgmt.name,
                                plaintext_pw: None,
                            };
                            let mut buf = Buffer::into_buf(request).unwrap();
                            buf.lend_mut(pw_cid, PwManagerOpcode::RequestPassword.to_u32().unwrap()).unwrap();
                            let ret = buf.to_original::<BasisRequestPassword, _>().unwrap();
                            if let Some(pw) = ret.plaintext_pw {
                                if let Some(basis) = basis_cache.basis_unlock(
                                    &mut pddb_os, mgmt.name.as_str().expect("name is not valid utf-8"), pw.as_str().expect("password was not valid utf-8"),
                                    mgmt.policy.unwrap_or(BasisRetentionPolicy::Persist)
                                ) {
                                    basis_cache.basis_add(basis);
                                    finished = true;
                                    mgmt.code = PddbRequestCode::NoErr;
                                } else {
                                    modals.add_list_item(t!("pddb.yes", xous::LANG)).expect("couldn't build radio item list");
                                    modals.add_list_item(t!("pddb.no", xous::LANG)).expect("couldn't build radio item list");
                                    match modals.get_radiobutton(t!("pddb.badpass", xous::LANG)) {
                                        Ok(response) => {
                                            if response.as_str() == t!("pddb.yes", xous::LANG) {
                                                finished = false;
                                                // this will cause just another go-around
                                            } else if response.as_str() == t!("pddb.no", xous::LANG) {
                                                finished = true;
                                                mgmt.code = PddbRequestCode::AccessDenied; // this will cause a return of AccessDenied
                                            } else {
                                                panic!("Got unexpected return from radiobutton");
                                            }
                                        }
                                        _ => panic!("get_radiobutton failed"),
                                    }
                                    xous::yield_slice(); // allow a redraw to happen before repeating the request
                                }
                            } else {
                                finished = true;
                                log::error!("internal error in basis unlock, aborting!");
                            }
                        }
                    }
                    _ => {
                        mgmt.code = PddbRequestCode::InternalError;
                    }
                }
                buffer.replace(mgmt).unwrap();
            }
            Some(Opcode::CloseBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut mgmt = buffer.to_original::<PddbBasisRequest, _>().unwrap();
                notify_of_disconnect(&mut pddb_os, &token_dict, &mut basis_cache);
                match mgmt.code {
                    PddbRequestCode::Close => {
                        match basis_cache.basis_unmount(&mut pddb_os, mgmt.name.as_str().expect("name is not valid utf-8")) {
                            Ok(_) => mgmt.code = PddbRequestCode::NoErr,
                            Err(e) => match e.kind() {
                                ErrorKind::NotFound => mgmt.code = PddbRequestCode::NotFound,
                                _ => mgmt.code = PddbRequestCode::InternalError,
                            }
                        }
                    }
                    _ => {
                        mgmt.code = PddbRequestCode::InternalError;
                    }
                }
                buffer.replace(mgmt).unwrap();
            }
            Some(Opcode::DeleteBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut mgmt = buffer.to_original::<PddbBasisRequest, _>().unwrap();
                notify_of_disconnect(&mut pddb_os, &token_dict, &mut basis_cache);
                match mgmt.code {
                    PddbRequestCode::Delete => {
                        match basis_cache.basis_delete(&mut pddb_os, mgmt.name.as_str().expect("name is not valid utf-8")) {
                            Ok(_) => mgmt.code = PddbRequestCode::NoErr,
                            Err(e) => match e.kind() {
                                ErrorKind::NotFound => mgmt.code = PddbRequestCode::NotFound,
                                _ => mgmt.code = PddbRequestCode::InternalError,
                            }
                        }
                    }
                    _ => {
                        mgmt.code = PddbRequestCode::InternalError;
                    }
                }
                buffer.replace(mgmt).unwrap();
            }
            Some(Opcode::KeyRequest) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req: PddbKeyRequest = buffer.to_original::<PddbKeyRequest, _>().unwrap();
                let bname = if req.basis_specified {
                    Some(req.basis.as_str().unwrap())
                } else {
                    None
                };
                let dict = req.dict.as_str().expect("dict utf-8 decode error");
                let key = req.key.as_str().expect("key utf-8 decode error");
                if basis_cache.dict_attributes(&mut pddb_os, dict, bname).is_err() {
                    if req.create_dict {
                        match basis_cache.dict_add(&mut pddb_os, dict, bname) {
                            Ok(_) => (),
                            Err(e) => {
                                match e.kind() {
                                    std::io::ErrorKind::OutOfMemory => {req.result = PddbRequestCode::NoFreeSpace; buffer.replace(req).unwrap(); continue}
                                    std::io::ErrorKind::NotFound => {req.result = PddbRequestCode::NotMounted; buffer.replace(req).unwrap(); continue}
                                    _ => {req.result = PddbRequestCode::InternalError; buffer.replace(req).unwrap(); continue}
                                }
                            }
                        }
                    } else {
                        req.result = PddbRequestCode::NotFound;
                        buffer.replace(req).unwrap(); continue
                    }
                }
                let alloc_hint = if let Some(hint) = req.alloc_hint {Some(hint as usize)} else {None};
                if basis_cache.key_attributes(&mut pddb_os, dict, key, bname).is_err() {
                    if !req.create_key {
                        req.result = PddbRequestCode::NotFound;
                        buffer.replace(req).unwrap(); continue
                    } else {
                        // create an empty key placeholder
                        let empty: [u8; 0] = [];
                        match basis_cache.key_update(&mut pddb_os,
                            dict, key, &empty, None, alloc_hint, bname, true
                        ) {
                            Ok(_) => {},
                            Err(e) => {
                                log::error!("Couldn't allocate key: {:?}", e);
                                match e.kind() {
                                    std::io::ErrorKind::NotFound => req.result = PddbRequestCode::NotMounted,
                                    std::io::ErrorKind::OutOfMemory => req.result = PddbRequestCode::NoFreeSpace,
                                    _ => req.result = PddbRequestCode::InternalError,
                                }
                                buffer.replace(req).unwrap(); continue
                            }
                        }
                    }
                }
                // at this point, we have established a basis/dict/key tuple.
                let token: ApiToken = [pddb_os.trng_u32(), pddb_os.trng_u32(), pddb_os.trng_u32()];
                let cid = if let Some(cb_sid) = req.cb_sid {
                    Some(xous::connect(xous::SID::from_array(cb_sid)).expect("couldn't connect for callback"))
                } else {
                    None
                };
                let token_record = TokenRecord {
                    dict: String::from(dict),
                    key: String::from(key),
                    basis: if let Some(name) = bname {Some(String::from(name))} else {None},
                    conn: cid,
                    alloc_hint,
                };
                token_dict.insert(token, token_record);
                req.token = Some(token);
                req.result = PddbRequestCode::NoErr;
                buffer.replace(req).unwrap();
            }
            Some(Opcode::KeyDrop) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, _, {
                let token: ApiToken = [t0 as u32, t1 as u32, t2 as u32];
                if let Some(rec) = token_dict.remove(&token) {
                    // now check if we can safely disconnect and recycle our connection number.
                    // This is important because we can only have 32 outgoing connections...
                    if let Some(conn_to_remove) = rec.conn {
                        let mut still_needs_cid = false;
                        for r in token_dict.values() {
                            // check through the remaining dictionary values to see if they have a connection that is the same as our number
                            if let Some(existing_conn) = r.conn {
                                if existing_conn == conn_to_remove {
                                    still_needs_cid = true;
                                    break;
                                }
                            }
                        }
                        // if nobody else had my connection number, disconnect it.
                        if !still_needs_cid {
                            unsafe{xous::disconnect(conn_to_remove).expect("couldn't disconnect from callback server")};
                        }
                    } else {
                        // if there was no/never a connection allocated, there's no connection to remove. do nothing.
                    }
                }
                xous::return_scalar(msg.sender, 1).expect("couldn't ack KeyDrop");
            }),
            Some(Opcode::DeleteKey) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req: PddbKeyRequest = buffer.to_original::<PddbKeyRequest, _>().unwrap();
                let bname = if req.basis_specified {
                    Some(req.basis.as_str().unwrap())
                } else {
                    None
                };
                let dict = req.dict.as_str().expect("dict utf-8 decode error");
                let key = req.key.as_str().expect("key utf-8 decode error");
                match basis_cache.key_remove(&mut pddb_os, dict, key, bname, false) {
                    Ok(_) => {
                        let mut evict_list = Vec::<ApiToken>::new();
                        // check to see if we need to eliminate any ApiTokens as a result of this.
                        for (token, rec) in token_dict.iter() {
                            if (rec.dict == dict) && (rec.key == key) {
                                // check the basis union rules
                                let mut matching = false;
                                if rec.basis.is_none() && bname.is_none() {
                                    matching = true;
                                }
                                if let Some(breq) = bname {
                                    if rec.basis.is_none() {
                                        matching = true;
                                    }
                                    if let Some(brec) = &rec.basis {
                                        if brec == breq {
                                            matching = true;
                                        }
                                    }
                                }
                                if matching {
                                    evict_list.push(*token);
                                }
                            }
                        }
                        for token in evict_list {
                            token_dict.remove(&token);
                        }
                        req.result = PddbRequestCode::NoErr;
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => req.result = PddbRequestCode::NotFound,
                            _ => req.result = PddbRequestCode::InternalError,
                        }
                    }
                }
                buffer.replace(req).unwrap();
            }
            Some(Opcode::DeleteDict) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req: PddbKeyRequest = buffer.to_original::<PddbKeyRequest, _>().unwrap();
                let bname = if req.basis_specified {
                    Some(req.basis.as_str().unwrap())
                } else {
                    None
                };
                let dict = req.dict.as_str().expect("dict utf-8 decode error");
                log::debug!("attempting to remove dict {} basis {:?}", dict, bname);
                match basis_cache.dict_remove(&mut pddb_os, dict, bname, false) {
                    Ok(_) => {
                        let mut evict_list = Vec::<ApiToken>::new();
                        // check to see if we need to eliminate any ApiTokens as a result of this.
                        for (token, rec) in token_dict.iter() {
                            if rec.dict == dict {
                                // check the basis union rules
                                let mut matching = false;
                                if rec.basis.is_none() && bname.is_none() {
                                    matching = true;
                                }
                                if let Some(breq) = bname {
                                    if rec.basis.is_none() {
                                        matching = true;
                                    }
                                    if let Some(brec) = &rec.basis {
                                        if brec == breq {
                                            matching = true;
                                        }
                                    }
                                }
                                if matching {
                                    evict_list.push(*token);
                                }
                            }
                        }
                        for token in evict_list {
                            token_dict.remove(&token);
                        }
                        req.result = PddbRequestCode::NoErr;
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => req.result = PddbRequestCode::NotFound,
                            _ => req.result = PddbRequestCode::InternalError,
                        }
                    }
                }
                buffer.replace(req).unwrap();
            }
            Some(Opcode::KeyAttributes) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<PddbKeyAttrIpc, _>().unwrap();
                if let Some(token_record) = token_dict.get(&req.token) {
                    let bname = if let Some(name) = &token_record.basis {
                        Some(name.as_str())
                    } else {
                        None
                    };
                    match basis_cache.key_attributes(&mut pddb_os, &token_record.dict, &token_record.key, bname) {
                        Ok(attr) => {
                            buffer.replace(PddbKeyAttrIpc::from_attributes(attr, req.token)).unwrap();
                        }
                        _ => {
                            req.code = PddbRequestCode::NotFound;
                            buffer.replace(req).unwrap();
                        }
                    }
                }
            }
            Some(Opcode::KeyCountInDict) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<PddbDictRequest, _>().unwrap();
                if key_token.is_some() {
                    log::debug!("key list already in progress");
                    req.code = PddbRequestCode::AccessDenied;
                    buffer.replace(req).unwrap();
                    continue;
                }
                key_token = Some(req.token);
                key_list.clear();
                let bname = if req.basis_specified {
                    Some(req.basis.as_str().unwrap())
                } else {
                    None
                };
                let dict = req.dict.as_str().expect("dict utf-8 decode error");
                log::debug!("counting keys in dict {} basis {:?}", dict, bname);
                match basis_cache.key_list(&mut pddb_os, dict, bname) {
                    Ok(list) => {
                        log::debug!("count: {}", list.len());
                        if list.len() > 0 {
                            req.index = list.len() as u32;
                            for key in list {
                                log::debug!("key list: {}", key);
                                key_list.push(key);
                            }
                        } else {
                            log::debug!("count is 0, resetting state");
                            // no keys to list, so reset the state
                            key_token = None;
                            key_list.clear();
                        }
                        req.code = PddbRequestCode::NoErr;
                    }
                    Err(e) => {
                        key_token = None;
                        key_list.clear();
                        match e.kind() {
                            std::io::ErrorKind::NotFound => req.code = PddbRequestCode::NotFound,
                            _ => req.code = PddbRequestCode::InternalError,
                        }
                    }
                }
                buffer.replace(req).unwrap();
            }
            Some(Opcode::GetKeyNameAtIndex) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<PddbDictRequest, _>().unwrap();
                if let Some(token) = key_token {
                    if req.token != token {
                        req.code = PddbRequestCode::AccessDenied;
                    } else {
                        if req.index >= key_list.len() as u32 {
                            req.code = PddbRequestCode::InternalError;
                        } else {
                            req.key = xous_ipc::String::<KEY_NAME_LEN>::from_str(&key_list[req.index as usize]);
                            req.code = PddbRequestCode::NoErr;
                            log::debug!("fetching key at index {}: {}", req.index, req.key);
                            // the last index requested must be the highest one!
                            if req.index == key_list.len() as u32 - 1 {
                                log::debug!("last key, resetting state");
                                key_token = None;
                                key_list.clear();
                            }
                        }
                    }
                } else {
                    log::debug!("multiple concurrent requests detected, returning error");
                    req.code = PddbRequestCode::AccessDenied;
                }
                buffer.replace(req).unwrap();
            }
            Some(Opcode::DictCountInBasis) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<PddbDictRequest, _>().unwrap();
                if key_token.is_some() {
                    req.code = PddbRequestCode::AccessDenied;
                    buffer.replace(req).unwrap();
                    continue;
                }
                dict_token = Some(req.token);
                dict_list.clear();
                let bname = if req.basis_specified {
                    Some(req.basis.as_str().unwrap())
                } else {
                    None
                };
                let list = basis_cache.dict_list(&mut pddb_os, bname);
                if list.len() > 0 {
                    req.index = list.len() as u32;
                    for dict in list {
                        dict_list.push(dict);
                    }
                } else { // no dicts to list, reset the state
                    dict_token = None;
                    dict_list.clear();
                }
                req.code = PddbRequestCode::NoErr;
                buffer.replace(req).unwrap();
            }
            Some(Opcode::GetDictNameAtIndex) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<PddbDictRequest, _>().unwrap();
                if let Some(token) = dict_token {
                    if req.token != token {
                        req.code = PddbRequestCode::AccessDenied;
                    } else {
                        if req.index >= dict_list.len() as u32 {
                            req.code = PddbRequestCode::InternalError;
                        } else {
                            req.dict = xous_ipc::String::<DICT_NAME_LEN>::from_str(&dict_list[req.index as usize]);
                            req.code = PddbRequestCode::NoErr;
                            // the last index requested must be the highest one!
                            if req.index == dict_list.len() as u32 - 1 {
                                dict_token = None;
                                dict_list.clear();
                            }
                        }
                    }
                } else {
                    req.code = PddbRequestCode::AccessDenied;
                }
                buffer.replace(req).unwrap();
            }
            Some(Opcode::ReadKey) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let pbuf = PddbBuf::from_slice_mut(buffer.as_mut()); // direct translation, no serialization necessary for performance
                let token = pbuf.token;
                if let Some(rec) = token_dict.get(&token) {
                    match basis_cache.key_read(&mut pddb_os,
                        &rec.dict, &rec.key,
                        &mut pbuf.data[..pbuf.len as usize], Some(pbuf.position as usize),
                        if let Some (name) = &rec.basis {Some(&name)} else {None}) {
                        Ok(readlen) => {
                            pbuf.len = readlen as u16;
                            pbuf.retcode = PddbRetcode::Ok;
                        }
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::NotFound => pbuf.retcode = PddbRetcode::BasisLost,
                            std::io::ErrorKind::UnexpectedEof => pbuf.retcode = PddbRetcode::UnexpectedEof,
                            std::io::ErrorKind::OutOfMemory => pbuf.retcode = PddbRetcode::DiskFull,
                            _ => pbuf.retcode = PddbRetcode::InternalError,
                        }
                    }
                } else {
                    pbuf.retcode = PddbRetcode::BasisLost;
                }
                // we don't nede a "replace" operation because all ops happen in-place
            }
            Some(Opcode::WriteKey) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let pbuf = PddbBuf::from_slice_mut(buffer.as_mut()); // direct translation, no serialization necessary for performance
                let token = pbuf.token;
                if let Some(rec) = token_dict.get(&token) {
                    match basis_cache.key_update(&mut pddb_os,
                        &rec.dict, &rec.key,
                        &pbuf.data[..pbuf.len as usize], Some(pbuf.position as usize),
                        rec.alloc_hint, if let Some (name) = &rec.basis {Some(&name)} else {None},
                        false
                    ) {
                        Ok(_) => {
                            pbuf.retcode = PddbRetcode::Ok;
                        }
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::NotFound => pbuf.retcode = PddbRetcode::BasisLost,
                            std::io::ErrorKind::UnexpectedEof => pbuf.retcode = PddbRetcode::UnexpectedEof,
                            std::io::ErrorKind::OutOfMemory => pbuf.retcode = PddbRetcode::DiskFull,
                            _ => pbuf.retcode = PddbRetcode::InternalError,
                        }
                    }
                } else {
                    pbuf.retcode = PddbRetcode::BasisLost;
                }
                // we don't nede a "replace" operation because all ops happen in-place

                // for now, do an expensive sync operation after every write to ensure data integrity
                basis_cache.sync(&mut pddb_os, None).expect("couldn't sync basis");
            }
            Some(Opcode::WriteKeyFlush) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                match basis_cache.sync(&mut pddb_os, None) {
                    Ok(_) => xous::return_scalar(msg.sender, PddbRetcode::Ok.to_usize().unwrap()).unwrap(),
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::OutOfMemory => xous::return_scalar(msg.sender, PddbRetcode::DiskFull.to_usize().unwrap()).unwrap(),
                        std::io::ErrorKind::NotFound => xous::return_scalar(msg.sender, PddbRetcode::BasisLost.to_usize().unwrap()).unwrap(),
                        _ => xous::return_scalar(msg.sender, PddbRetcode::InternalError.to_usize().unwrap()).unwrap(),
                    }
                };
            }),
            Some(Opcode::MenuListBasis) => {
                let bases = basis_cache.basis_list();
                let mut note = String::from(t!("pddb.menu.listbasis_response", xous::LANG));
                for basis in bases.iter() {
                    note.push_str(basis);
                }
                modals.show_notification(&note, None).expect("couldn't show basis list");
            },
            #[cfg(not(any(target_os = "none", target_os = "xous")))]
            Some(Opcode::DangerousDebug) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dbg = buffer.to_original::<PddbDangerousDebug, _>().unwrap();
                match dbg.request {
                    DebugRequest::Dump => {
                        log::info!("dumping pddb to {}", dbg.dump_name.as_str().unwrap());
                        pddb_os.dbg_dump(Some(dbg.dump_name.as_str().unwrap().to_string()), None);
                    },
                    DebugRequest::Remount => {
                        log::info!("attempting remount");
                        basis_cache = BasisCache::new(); // this effectively erases the PDDB from memory
                        if let Some(sys_basis) = pddb_os.pddb_mount() {
                            log::info!("remount successful");
                            basis_cache.basis_add(sys_basis);
                        } else {
                            log::info!("remount failed");
                        }
                    }
                }
            }
            Some(Opcode::Quit) => {
                log::warn!("quitting the PDDB server");
                send_message(
                    pw_cid,
                    Message::new_blocking_scalar(PwManagerOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0)
                ).unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
                break
            }
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    pw_handle.join().expect("password ux manager thread did not join as expected");
    xns.unregister_server(pddb_sid).unwrap();
    xous::destroy_server(pddb_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn ensure_password(modals: &modals::Modals, pddb_os: &mut PddbOs, _pw_cid: xous::CID) -> PasswordState {
    log::info!("Requesting login password");
    loop {
        match pddb_os.try_login() {
            PasswordState::Correct => {
                return PasswordState::Correct
            }
            PasswordState::Incorrect => {
                pddb_os.clear_password(); // clear the bad password entry
                // check if the user wants to re-try or not.
                modals.add_list_item(t!("pddb.yes", xous::LANG)).expect("couldn't build radio item list");
                modals.add_list_item(t!("pddb.no", xous::LANG)).expect("couldn't build radio item list");
                match modals.get_radiobutton(t!("pddb.badpass", xous::LANG)) {
                    Ok(response) => {
                        if response.as_str() == t!("pddb.yes", xous::LANG) {
                            continue;
                        } else if response.as_str() == t!("pddb.no", xous::LANG) {
                            return PasswordState::Incorrect;
                        } else {
                            panic!("Got unexpected return from radiobutton");
                        }
                    }
                    _ => panic!("get_radiobutton failed"),
                }
            }
            PasswordState::Uninit => {
                // check for a migration event
                #[cfg(feature="migration1")]
                {
                    if pddb_os.migration_v1_to_v2(_pw_cid) == PasswordState::Correct {
                        if pddb_os.try_login() == PasswordState::Correct {
                            log::info!("Migration v1->v2 successful");
                            return PasswordState::Correct
                        } else {
                            log::warn!("Migration v1->v2 succeeded, but somehow the v2 remount failed.");
                        }
                    }
                }
                return PasswordState::Uninit;
            }
        }
    }
}
fn try_mount_or_format(modals: &modals::Modals, pddb_os: &mut PddbOs, basis_cache: &mut BasisCache, pw_state: PasswordState, time_resetter: xous::CID) -> bool {
    log::info!("Attempting to mount the PDDB");
    if pw_state == PasswordState::Correct {
        if let Some(sys_basis) = pddb_os.pddb_mount() {
            log::info!("PDDB mount operation finished successfully");
            basis_cache.basis_add(sys_basis);
            return true
        }
    }
    // correct password but no mount -> offer to format; uninit -> offer to format
    if pw_state == PasswordState::Correct || pw_state == PasswordState::Uninit {
        #[cfg(any(target_os = "none", target_os = "xous"))]
        {
            log::debug!("PDDB did not mount; requesting format");
            modals.add_list_item(t!("pddb.okay", xous::LANG)).expect("couldn't build radio item list");
            modals.add_list_item(t!("pddb.cancel", xous::LANG)).expect("couldn't build radio item list");
            let do_format: bool;
            match modals.get_radiobutton(t!("pddb.requestformat", xous::LANG)) {
                Ok(response) => {
                    if response.as_str() == t!("pddb.okay", xous::LANG) {
                        do_format = true;
                    } else if response.as_str() == t!("pddb.cancel", xous::LANG) {
                        log::info!("PDDB format aborted by user");
                        do_format = false;
                    } else {
                        panic!("Got unexpected return from radiobutton");
                    }
                }
                _ => panic!("get_radiobutton failed"),
            }
            if do_format {
                let fast: bool;
                if false {
                    modals.add_list_item(t!("pddb.no", xous::LANG)).expect("couldn't build radio item list");
                    modals.add_list_item(t!("pddb.yes", xous::LANG)).expect("couldn't build radio item list");
                    match modals.get_radiobutton(t!("pddb.devbypass", xous::LANG)) {
                        Ok(response) => {
                            if response.as_str() == t!("pddb.yes", xous::LANG) {
                                fast = true;
                            } else if response.as_str() == t!("pddb.no", xous::LANG) {
                                fast = false;
                            } else {
                                panic!("Got unexpected return from radiobutton");
                            }
                        }
                        _ => panic!("get_radiobutton failed"),
                    }
                } else {
                    fast = false;
                }

                pddb_os.pddb_format(fast, Some(&modals)).expect("couldn't format PDDB");

                // reset the RTC at the point of PDDB format. It is done now because at this point we know that
                // no time offset keys can exist in the PDDB, and as a measure of good hygeine we want to restart
                // our RTC counter from a random duration between epoch and 10 years to give some deniability about
                // how long the device has been in use.
                let _ = xous::send_message(time_resetter,
                    xous::Message::new_blocking_scalar(
                        0, // the ID is "hard coded" using enumerated discriminants
                        0, 0, 0, 0
                    )
                ).expect("couldn't reset time");

                if let Some(sys_basis) = pddb_os.pddb_mount() {
                    log::info!("PDDB mount operation finished successfully");
                    basis_cache.basis_add(sys_basis);
                    true
                } else {
                    log::error!("Despite formatting, no PDDB was found!");
                    let mut err = String::from(t!("pddb.internalerror", xous::LANG));
                    err.push_str(" #1"); // punt and leave an error code, because this "should" be rare
                    modals.show_notification(err.as_str(), None).expect("notification failed");
                    false
                }
            } else {
                false
            }
        }
        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        {
            pddb_os.pddb_format(false, Some(&modals)).expect("couldn't format PDDB");
            let _ = xous::send_message(time_resetter,
                xous::Message::new_blocking_scalar(
                    0, // the ID is "hard coded" using enumerated discriminants
                    0, 0, 0, 0
                )
            ).expect("couldn't reset time");
            pddb_os.dbg_dump(Some("full".to_string()), None);
            if let Some(sys_basis) = pddb_os.pddb_mount() {
                log::info!("PDDB mount operation finished successfully");
                basis_cache.basis_add(sys_basis);
                true
            } else {
                log::error!("Despite formatting, no PDDB was found!");
                let mut err = String::from(t!("pddb.internalerror", xous::LANG));
                err.push_str(" #1"); // punt and leave an error code, because this "should" be rare
                modals.show_notification(err.as_str(), None).expect("notification failed");
                false
            }
        }
    } else {
        // password was incorrect, don't try anything just return false
        false
    }
}

// Test cases that have been coded to run directly on hardware (that is, they do not require host-OS debug features)
#[allow(dead_code)]
pub(crate) fn manual_testcase(hw: &mut PddbOs) {
    log::info!("Initializing disk...");
    hw.pddb_format(true, None).unwrap();
    log::info!("Done initializing disk");

    // it's a vector because order is important: by default access to keys/dicts go into the latest entry first, and then recurse to the earliest
    let mut basis_cache = BasisCache::new();

    log::info!("Attempting to mount the PDDB");
    if let Some(sys_basis) = hw.pddb_mount() {
        log::info!("PDDB mount operation finished successfully");
        basis_cache.basis_add(sys_basis);
    } else {
        log::info!("PDDB did not mount; did you remember to format the PDDB region?");
    }
    log::info!("size of vpage: {}", VPAGE_SIZE);

    // add a "system settings" dictionary to the default basis
    log::info!("adding 'system settings' dictionary");
    basis_cache.dict_add(hw, "system settings", None).expect("couldn't add system settings dictionary");
    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/Kosagi", "my_wpa_key_here".as_bytes(), None, None, None, false).expect("couldn't add a key");
    let mut readback = [0u8; 15];
    match basis_cache.key_read(hw, "system settings", "wifi/wpa_keys/Kosagi", &mut readback, None, None) {
        Ok(readsize) => {
            log::info!("read back {} bytes", readsize);
            log::info!("read data: {}", String::from_utf8_lossy(&readback));
        },
        Err(e) => {
            log::info!("couldn't read data: {:?}", e);
        }
    }
    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/e4200", "12345678".as_bytes(), None, None, None, false).expect("couldn't add a key");

    // add a "big" key
    let mut bigdata = [0u8; 5000];
    for (i, d) in bigdata.iter_mut().enumerate() {
        *d = i as u8;
    }
    basis_cache.key_update(hw, "system settings", "big_pool1", &bigdata, None, None, None, false).expect("couldn't add a key");

    basis_cache.dict_add(hw, "test_dict_2", None).expect("couldn't add test dictionary 2");
    basis_cache.key_update(hw, "test_dict_2", "test key in dict 2", "some data".as_bytes(), None, Some(128), None, false).expect("couldn't add a key to second dict");

    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/e4200", "ABC".as_bytes(), Some(2), None, None, false).expect("couldn't update e4200 key");

    log::info!("test readback of wifi/wpa_keys/e4200");
    match basis_cache.key_read(hw, "system settings", "wifi/wpa_keys/e4200", &mut readback, None, None) {
        Ok(readsize) => {
            log::info!("read back {} bytes", readsize);
            log::info!("read data: {}", String::from_utf8_lossy(&readback));
        },
        Err(e) => {
            log::info!("couldn't read data: {:?}", e);
        }
    }
}

#[allow(dead_code)]
pub(crate) fn hw_testcase(pddb_os: &mut PddbOs) {
    log::info!("Running `hw` test case");
    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    pddb_os.test_reset();

    manual_testcase(pddb_os);

    log::info!("Re-mount the PDDB");
    let mut basis_cache = BasisCache::new();
    if let Some(sys_basis) = pddb_os.pddb_mount() {
        log::info!("PDDB mount operation finished successfully");
        basis_cache.basis_add(sys_basis);
    } else {
        log::info!("PDDB did not mount; did you remember to format the PDDB region?");
    }
    log::info!("test readback of wifi/wpa_keys/e4200");
    let mut readback = [0u8; 16]; // this buffer is bigger than the data in the key, but that's alright...
    match basis_cache.key_read(pddb_os, "system settings", "wifi/wpa_keys/e4200", &mut readback, None, None) {
        Ok(readsize) => {
            log::info!("read back {} bytes", readsize);
            log::info!("read data: {}", String::from_utf8_lossy(&readback));
        },
        Err(e) => {
            log::info!("couldn't read data: {:?}", e);
        }
    }

    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    pddb_os.dbg_dump(Some("manual".to_string()), None);
}

fn notify_of_disconnect(pddb_os: &mut PddbOs, token_dict: &HashMap::<ApiToken, TokenRecord>, basis_cache: &mut BasisCache) {
    // 1. search to see if any of the active tokens are are in our token_dict
    // 2. notify them of the disconnect, if there is a callback set.
    for (api_key, entry) in token_dict.iter() {
        log::debug!("disconnect notify searching {:?}", entry);
        if let Some(cb) = entry.conn {
            match basis_cache.key_attributes(pddb_os, &entry.dict, &entry.key, entry.basis.as_deref()) {
                Ok(_) => {
                    match send_message(cb, Message::new_scalar(
                        pddb::CbOp::Change.to_usize().unwrap(),
                        api_key[0] as _,
                        api_key[1] as _,
                        api_key[2] as _,
                        0,
                    )) {
                        Err(e) => {
                            log::warn!("Callback on {}:{} for basis removal failed: {:?}", &entry.dict, &entry.key, e);
                        },
                        _ => {
                            log::debug!("Callback on {}:{} for basis removal success", &entry.dict, &entry.key);
                        }
                    }
                },
                Err(_) => {
                    // do nothing. It's probably not right that a key doesn't exist that we don't have in our records, but don't crash the system.
                    log::warn!("Disconnect basis inconsistent state, {}:{} not found", &entry.dict, &entry.key);
                }
            }
        }
    }
}
