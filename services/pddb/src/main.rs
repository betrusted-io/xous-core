#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate bitflags;
extern crate bitfield;

mod api;
use api::*;
mod backend;
use backend::*;

use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use core::cell::RefCell;
use std::rc::Rc;

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
/// FSCB - Free Space Commit Buffer. A few pages allocated to tracking a subset of free space,
///   meant to accelerate PDDB operations. Creates a side-channel that can reveal that some activity
///   has happened, but without disclosing what and why. Contains FastSpace and SpaceUpdate records.
///   Frequently updated, so the buffer is slightly oversized, and which sector is "hot" is randomized
///   for wear-levelling.
/// MBBB - Make Before Break Buffer. A set of randomly allocated pages that are a shadow copy
///   of a page table page. If any data exists, its contents override those of a corrupted page table.
/// FastSpace - a collection of random pages that are known to be empty. The number of pages in FastSpace
///   is reduced from the absolute amount of free space available by at least a factor of FSCB_FILL_COEFFICIENT.
/// SpaceUpdate - encrypted patches to the FastSpace table. The FastSpace table is "heavyweight", and would
///   be too expensive to update on every page allocation, so SpaceUpdate is used to patch the FastSpace table.
///
/// A `Path` like the following is deconstructed as follows by the PDDB:
///
///  Dictionary
///    |         Key
///    |          |
///  --+- --------+---------------------
///  logs:matrix/alice/oct30_2021/bob.txt
///
/// It could equally have an arbitrary name like "logs:Matrix - alice to bob Oct 30 2021";
/// as long as the string that identifies a Key is unique, it's stored in the database
/// all the same. Any valid utf-8 unicode characters are acceptable.
///
/// Likewise, something like this:
/// settings:wifi/Kosagi.json
/// Would be deconstructed into the "settings" dictionary with a key of wifi/Kosagi.json.
///
///
/// Threat model:
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
/// Auditor Notices:
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
/// General Operation:
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

/// General flash->key structure
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
///
///
/// Basis Deniability
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
/// Basis Unlock Procedure
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
/// Journaling
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
/// Precursor's Implementation-Specific Flash Memory Organization:
///
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
    let mut entropy = Rc::new(RefCell::new(TrngPool::new()));

    // OS-specific PDDB driver
    let mut pddb_os = PddbOs::new(Rc::clone(&entropy));
    log::info!("Initializing disk...");
    pddb_os.pddb_format().unwrap();
    log::info!("Done initializing disk");

    // register a suspend/resume listener
    let sr_cid = xous::connect(pddb_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(pddb_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::KeyRequest) => {
                // placeholder
            }
            Some(Opcode::ReadKeyScalar) => msg_blocking_scalar_unpack!(msg, tok0, tok1, tok2, len, {
                // placeholder
            }),
            Some(Opcode::ReadKeyMem) => {
                // placeholder
            }
            Some(Opcode::WriteKeyScalar1)
            | Some(Opcode::WriteKeyScalar2)
            | Some(Opcode::WriteKeyScalar3)
            | Some(Opcode::WriteKeyScalar4) => msg_blocking_scalar_unpack!(msg, tok0, tok1, tok2, data, {
                // placeholder
            }),
            Some(Opcode::WriteKeyMem) => {
                // placeholder
            }
            Some(Opcode::WriteKeyFlush) => {
                // placeholder
            }
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
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
