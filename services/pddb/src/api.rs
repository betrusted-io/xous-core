mod rkyv_enum;
pub use rkyv_enum::*;

use bitfield::bitfield;
use std::num::NonZeroU32;
use core::ops::{Deref, DerefMut};

// on the "[allow(dead_code)]" directives: these constants are used to define the PDDB, and are
// sometimes used by both `bin` (main.rs) and `lib` (lib.rs) views, but also, sometimes used
// by only one. The API view is included in both, and if a constant is not used in both views it
// triggers an unused code warning. However, it feels really weird to split these constants into
// bin/lib/api views and shuffle them back and forth depending upon any given particular need
// to access them (especially given that some are more useful for debugging and testing and thus
// get configured out in some builds). Thus, we tell clippy to just shut up and stick them all
// here, because sometimes, clippy just can't see the big picture.

// note this name cannot be changed because it is baked into `libstd`
pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";
pub(crate) const SERVER_NAME_PDDB_POLLER: &str     = "_PDDB Mount Poller_";
/// This is the registered name for a dedicated private API channel to the PDDB for doing the time reset
/// Even though nobody but the PDDB should connect to this, we have to share it publicly so the PDDB can
/// depend upon this constant.
pub const TIME_SERVER_PDDB: &'static str = "_dedicated pddb timeserver connection_";

#[allow(dead_code)]
pub(crate) const BASIS_NAME_LEN: usize = 64; // don't want this too long anyways, because it's not recorded anywhere - users have to type it in.
#[allow(dead_code)]
pub(crate) const DICT_NAME_LEN: usize = 127 - 4 - 4 - 4 - 4; // u32: flags, age, free index, numkeys = 111
#[allow(dead_code)]
pub(crate) const KEY_NAME_LEN: usize = 127 - 8 - 8 - 8 - 4 - 4; // u64: vaddr/len/resvd, u32: flags, age = 95
#[allow(dead_code)]
pub(crate) const PASSWORD_LEN: usize = 72; // this is actually set by bcrypt
#[allow(dead_code)]
pub(crate) const PDDB_MAGIC: [u8; 4] = [0x50, 0x44, 0x44, 0x42];
/// migrateable version pairs
/// PDDB_MIGRATE_1:
///   00.00.01.01 - xous 0.9.7 release (original base release)
///   00.00.02.01 - xous 0.9.8 migration -> hkdf added on basis key derivation to make separate PT/data keys
#[allow(dead_code)]
pub(crate) const PDDB_MIGRATE_1: (u32, u32) = (0x00_00_01_01, 0x00_00_02_01);
#[allow(dead_code)]
pub(crate) const PDDB_VERSION: u32 = 0x00_00_02_01;
#[allow(dead_code)]
// PDDB_A_LEN may be shorter than xous::PDDB_LEN, to speed up testing.
#[allow(dead_code)]
#[cfg(not(any(feature="pddbtest",feature="autobasis",feature="ci",feature="smalldb")))]
pub(crate) const PDDB_A_LEN: usize = xous::PDDB_LEN as usize;
#[allow(dead_code)]
#[cfg(any(feature="pddbtest",feature="autobasis",feature="ci",feature="smalldb"))]
pub const PDDB_A_LEN: usize = 4 * 1024 * 1024;

/// range for the starting point of a journal number, picked from a random seed
/// the goal is to reduce info leakage about the age of structures relative to each other
/// in various basis in case of partial disclosure of passwords (especially the system password)
/// The idea is to pick a number that is larger than the wear-out lifetime of the FLASH memory.
/// This memory should wear out after about 100k R/W cycles, so, 100MM is probably a big enough
/// range, while avoiding exhausting a 32-bit count.
#[allow(dead_code)]
pub(crate) const JOURNAL_RAND_RANGE: u32 = 100_000_000;
/// The FSCB has a much smaller journal number (256), so we can't afford to make the starting point as big.
#[allow(dead_code)]
pub(crate) const FSCB_JOURNAL_RAND_RANGE: u8 = 24;

/// A number between (0, 1] that defines how many of the "truly free" pages we
/// should put into the FSCB. A value of 0.0 is not allowed as that leaves no free pages.
/// A value of 1.0 would allow an attacker to deduce the real size of data because all
/// the freee space would be tracked in the FSCB. Thus the trade-off is deniability versus
/// performance: a fill coefficient of 1.0 means we'd never have to do a brute force scan
/// for free pages, but you would have no deniability; a fill coefficient of near-0 means
/// we could plausibly deny a lot of pages as being free space, but every time you wanted
/// to grow a record, you'd have to unlock all your Basis and do a brute force scan, otherwise
/// you risk overwriting hidden data by mistaking it as free space. The initial setting is
/// 0.5: with this setting, we won't be more than a factor of 2 off from the ideal setting!
#[allow(dead_code)]
pub(crate) const FSCB_FILL_COEFFICIENT: f32 = 0.5;
/// This adds some uncertainty to the fill coeffiecient. This adds "noise" to the free space
/// top-up, to try and mitigate analysis patterns of the amount of free space available based
/// on a fixed ratio reduction over time. Expressed as the extents of a random +/- offset
/// from the FILL_COEFFICIENT.
#[allow(dead_code)]
pub(crate) const FSCB_FILL_UNCERTAINTY: f32 = 0.1;

/// This is a number that represents a fraction out of 255 that is the chance of a given
/// block of unused data being recycled with noise. (So 26 would be roughly a 10% chance).
/// This is an optimization that makes PDDB migrations/restores faster, while maintaining
/// some amount of deniability. The higher the chance of rekey, the better the deniability.
///
/// If you set the REKEY_CHANCE to 0, then after a migration or rekey operation,
/// exactly the set of data that is used will have its ciphertext changed, which is a precise
/// leak of the amount of information in the PDDB.
///
/// If you set REKEY_CHANCE to 256 or greater, then, every block is mutated, regardless of
/// the usage state of the PDDB, and full deniability is preserved. However, this will cause
/// the rekey operation to always take about a half hour, even if the PDDB is basically empty.
///
/// The initial 10% threshold gives us sufficient deniability for e.g. small secrets like
/// passwords, TOTP tokens, U2F keys while incurring just a couple extra minutes overhead.
/// However it would probably not be sufficient if you routinely use the PDDB to store
/// large objects like images, audio, or large blocks of text.
#[allow(dead_code)]
pub(crate) const FAST_REKEY_CHANCE: u32 = 26;

#[allow(dead_code)]
pub const PDDB_DEFAULT_SYSTEM_BASIS: &'static str = ".System";
// this isn't an "official" basis, but it is used for the AAD for encrypting the FastSpace structure
#[allow(dead_code)]
pub(crate) const PDDB_FAST_SPACE_SYSTEM_BASIS: &'static str = ".FastSpace";

#[allow(dead_code)]
// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
pub(crate) const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    IsMounted = 0,
    TryMount = 1,

    ListBasis = 2,
    LatestBasis = 3,
    /// Note that creating a basis does not automatically open it!
    CreateBasis = 4,
    OpenBasis = 5,
    CloseBasis = 6,
    /// warning, the Delete routines have not been well tested
    DeleteBasis = 7,
    DeleteKey = 8,
    DeleteDict = 9,
    KeyAttributes = 10,

    // routines to list available resources
    KeyCountInDict = 11,
    // GetKeyNameAtIndex = 12, // superceded by ListKeyV2
    DictCountInBasis = 13,
    GetDictNameAtIndex = 14,

    /// primary method for accessing the database
    KeyRequest = 15,

    // pddbkey methods
    ReadKey = 16,
    WriteKey = 17,
    WriteKeyFlush = 18,

    // GC methods
    PeriodicScrub = 19,

    /// drops any connection state associated with a given key
    KeyDrop = 20,

    /// Menu opcodes
    MenuListBasis = 21,

    /// Security state checks
    IsEfuseSecured = 22,

    /// Suspend/resume callback
    SuspendResume = 23,
    /// quit the server
    Quit = 24,
    /// Write debug dump (only available in hosted mode)
    #[cfg(not(target_os = "xous"))]
    DangerousDebug = 25,
    #[cfg(all(feature="pddbtest", feature="autobasis"))]
    BasisTesting = 26,

    ListBasisStd = 26,
    CreateBasisStd = 27,

    ListDictStd = 28,
    ListKeyStd = 29,

    /// libstd equivalent of `KeyRequest`
    OpenKeyStd = 30,

    /// Read a number of bytes from the current offset.
    ReadKeyStd = 31,

    /// Write a number of bytes at the current offset
    WriteKeyStd = 32,

    /// libstd equivalent of `KeyDrop`
    CloseKeyStd = 34,

    /// Remove a key from the database.
    DeleteKeyStd = 35,

    /// Return the latest basis. Useful for resolving `::` as a path.
    LatestBasisStd = 36,

    /// List all keys and dicts under a given colon-delimited path
    ListPathStd = 37,

    /// Get information about a file path
    StatPathStd = 38,

    /// Seek a file descriptor
    SeekKeyStd = 39,

    /// Create a dict
    CreateDictStd = 40,

    /// Remove an empty dict
    DeleteDictStd = 41,

    /// Rekey the PDDB
    RekeyPddb = 42,

    /// Reset "don't ask to init root keys" flag - pass-through to root keys object - for end of OQC test
    ResetDontAskInit = 43,

    /// Flush the SpaceUpdate log, restoring deniability
    FlushSpaceUpdate = 44,

    /// Optimized key listing
    ListKeyV2 = 45,

    /// change unlock PIN
    MenuChangePin = 46,

    /// run a test or diagnostic command (depends on the build)
    InternalTest = 47,

    /// Clear password cache and ask for it again
    UncacheAndAskPassword = 48,

    /// Unmount the PDDB.
    TryUnmount = 49,

    /// Compute a block of backup hashes
    ComputeBackupHashes = 50,

    /// Halt PDDB operation. This is used to prevent any further actions on the PDDB, i.e.
    /// in prepration for a system shutdown or backup event.
    PddbHalt = 51,

    /// Bulk read of a dictionary
    DictBulkRead = 52,

    /// Mount was attempted
    MountAttempted = 53,

    /// Basis monitoring - reports when the basis order has changed
    BasisMonitor = 54,

    /// Bulk delete of keys within a dictionary; if non-existent keys are specified, no error is returned.
    DictBulkDelete = 55,

    /// Prune the cache. Used mainly for diagnostics.
    Prune = 56,

    /// This key type could not be decoded
    InvalidOpcode = u32::MAX as _,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PollOp {
    Poll = 0,
    Quit = 1,
}

pub type ApiToken = [u32; 3];
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[repr(C)]
pub struct PddbBasisList {
    /// the first 63 that fit in the list -- generally we anticipate not more than a few being open at a time, so this should be enough.
    pub list: [xous_ipc::String::<BASIS_NAME_LEN>; 63],
    /// total number of basis open. Should be <= 63, but we allow it to be larger to indicate cases where this structure wasn't big enough.
    pub num: u32,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub enum PddbRequestCode {
    Create = 0,
    Open = 1,
    Close = 2,
    Delete = 3,
    NoErr = 4,
    NotMounted = 5,
    NoFreeSpace = 6,
    NotFound = 7,
    InternalError = 8,
    AccessDenied = 9,
    Uninit = 10,
    DuplicateEntry = 11,
    BulkRead = 12,
}
// enum def is in rkyv_enum module
impl BasisRetentionPolicy {
    pub fn derive_init_state(&self) -> u32 {
        match self {
            BasisRetentionPolicy::Persist => 0,
            BasisRetentionPolicy::ClearAfterSleeps(sleeps) => *sleeps,
            //BasisRetentionPolicy::TimeOutSecs(secs) => *secs,
        }
    }
}
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbBasisRequest {
    pub name: xous_ipc::String::<BASIS_NAME_LEN>,
    pub code: PddbRequestCode,
    pub policy: Option<BasisRetentionPolicy>,
}
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbDictRequest {
    pub basis_specified: bool,
    pub basis: xous_ipc::String::<BASIS_NAME_LEN>,
    pub dict: xous_ipc::String::<DICT_NAME_LEN>,
    pub key: xous_ipc::String::<KEY_NAME_LEN>,
    pub index: u32,
    pub token: [u32; 4],
    pub code: PddbRequestCode,
    /// used only to specify a readback size limit for bulk-return requests
    pub bulk_limit: Option<usize>,
    /// return value for key_count metadata (informative only)
    pub key_count: u32,
    /// return value for found_key_count metadata (informative only)
    pub found_key_count: u32,
}

/// A structure for requesting a token to access a particular key/value pair
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbKeyRequest {
    pub basis_specified: bool,
    pub basis: xous_ipc::String::<BASIS_NAME_LEN>,
    pub dict: xous_ipc::String::<DICT_NAME_LEN>,
    pub key: xous_ipc::String::<KEY_NAME_LEN>,
    pub token: Option<ApiToken>,
    pub create_dict: bool,
    pub create_key: bool,
    pub alloc_hint: Option<u64>, // this is a usize but for IPC we must have defined memory sizes, so we pick the big option.
    pub cb_sid: Option<[u32; 4]>,
    pub result: PddbRequestCode,
}

pub(crate) const MAX_PDDBKLISTLEN: usize = 4064;
/// A structure for requesting a token to access a particular key/value pair
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub (crate) struct PddbKeyList {
    pub token: [u32; 4],
    pub data: [u8; MAX_PDDBKLISTLEN],
    pub retcode: PddbRetcode,
    pub end: bool,
}

/// A structure for bulk deletion of keys
pub(crate) const MAX_PDDB_DELETE_LEN: usize = 3800; // approximate limit, might be a little higher if we cared to calculate it out
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub (crate) struct PddbDeleteList {
    pub basis_specified: bool,
    pub basis: xous_ipc::String::<BASIS_NAME_LEN>,
    pub dict: xous_ipc::String::<DICT_NAME_LEN>,
    pub data: [u8; MAX_PDDB_DELETE_LEN],
    pub retcode: PddbRetcode,
}

/// Return codes for Read/Write API calls to the main server
#[repr(u8)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq)]
pub(crate) enum PddbRetcode {
    Uninit = 0,
    Ok = 1,
    BasisLost = 2,
    AccessDenied = 3,
    UnexpectedEof = 4,
    InternalError = 5,
    DiskFull = 6,
}

pub(crate) const PDDB_BUF_DATA_LEN: usize = 4072;
/// PddbBuf is a C-representation of a page of memory that's used
/// to shuttle data for streaming channels. It must be exactly one
/// page in size, with some overhead specific to the PDDB book-keeping
/// at the top, and the remainder available for shuttling data.
#[repr(C, align(4096))]
pub(crate) struct PddbBuf {
    /// api token for the given buffer
    pub(crate) token: ApiToken,
    /// a field reserved for the return code
    pub(crate) retcode: PddbRetcode,
    reserved: u8,
    /// length of the data field
    pub(crate) len: u16,
    /// point in the key stream. 64-bit for future-compatibility; but, can't be larger than 32 bits on a 32-bit target.
    pub(crate) position: u64,
    pub(crate) data: [u8; PDDB_BUF_DATA_LEN],
}

/// Returns key name, data, and attributes in a single record. For bulk reads.
/// The `dict` field is not specified, because, this had to be defined for the bulk read.
/// The `basis` field, however, must be specified because a dictionary can be composed of
/// data from multiple Bases.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug)]
pub struct PddbKeyRecord {
    pub name: String,
    pub len: usize,
    pub reserved: usize,
    pub age: usize,
    pub index: NonZeroU32,
    pub basis: String,
    /// If this is None, it means either that the data read exceeded the bulk read limit
    /// and the record must be explicitly re-read with a non-bulk call to fetch its data,
    /// or the key has zero length. Check the `len` field to differentiate the two.
    /// (Turns out that `rkyv` crashes when deserializing a zero-length Vec)
    pub data: Option<Vec::<u8>>,
}

/// Return codes for Read/Write API calls to the main server
#[repr(u32)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PddbBulkReadCode {
    /// Uninitialized value
    Uninit = 0,
    /// call already in progress or timed out (e.g. token does not match what's on record)
    Busy = 1,
    /// last return data structure
    Last = 2,
    /// return data structure, with more data to come
    Streaming = 3,
    /// dictionary not found
    NotFound = 4,
    /// PDDB internal error
    InternalError = 5,
}

#[derive(Default, Debug)]
#[repr(C)]
pub struct BulkReadHeader {
    pub code: u32,
    pub starting_key_index: u32,
    pub len: u32,
    pub total: u32,
    pub token: [u32; 4],
}
impl Deref for BulkReadHeader {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const BulkReadHeader as *const u8, core::mem::size_of::<BulkReadHeader>())
                as &[u8]
        }
    }
}
impl DerefMut for BulkReadHeader {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut BulkReadHeader as *mut u8, core::mem::size_of::<BulkReadHeader>())
                as &mut [u8]
        }
    }
}

#[allow(dead_code)]
/// Ensure that the `PddbBuf` struct is exactly one page big
const fn _assert_pddbbuf_is_4096_bytes() {
    unsafe { core::mem::transmute::<_, PddbBuf>([0u8; 4096]); }
}

impl PddbBuf {
    pub(crate) fn from_slice_mut(slice: &mut [u8]) -> &mut PddbBuf {
        // this transforms the slice [u8] into a PddbBuf ref.
        unsafe {core::mem::transmute::<*mut u8, &mut PddbBuf>(slice.as_mut_ptr()) }
    }
}

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq)]
    pub struct KeyFlags(u32);
    impl Debug;
    /// set if the entry is valid -- in the cache, an invalid entry means it was previously allocated but then deleted, and needs a sync
    pub valid, set_valid: 0;
    /// resolved indicates that the "start" address isn't fully resolved yet in the cache
    pub unresolved, set_unresolved: 1;
}

/// A structure for passing around key metadata
#[derive(Debug)]
pub struct KeyAttributes {
    /// actual length of data in the key
    pub len: usize,
    /// pre-reserved storage space for the key (growable to this bound "at no cost")
    pub reserved: usize,
    /// access count
    pub age: usize,
    /// owning dictionary
    pub dict: String,
    /// the specific basis from which this key's metadata was found
    pub basis: String,
    /// flags
    pub flags: KeyFlags,
    /// descriptor index
    pub index: NonZeroU32,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
/// serializeable version of the attributes structure
pub struct PddbKeyAttrIpc {
    pub len: u64,
    pub reserved: u64,
    pub age: u64,
    pub dict: xous_ipc::String::<DICT_NAME_LEN>,
    pub basis: xous_ipc::String::<BASIS_NAME_LEN>,
    pub flags: u32,
    pub index: u32,
    pub token: ApiToken,
    pub code: PddbRequestCode,
}
impl PddbKeyAttrIpc {
    pub fn new(token: ApiToken) -> PddbKeyAttrIpc {
        PddbKeyAttrIpc {
            len: 0,
            reserved: 0,
            age: 0,
            dict: xous_ipc::String::<DICT_NAME_LEN>::new(),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::new(),
            flags: 0,
            index: 0,
            token,
            code: PddbRequestCode::Uninit,
        }
    }
    #[allow(dead_code)]
    pub fn to_attributes(&self) -> KeyAttributes {
        KeyAttributes {
            len: self.len as usize,
            reserved: self.reserved as usize,
            age: self.age as usize,
            dict: String::from(self.dict.as_str().unwrap()),
            basis: String::from(self.basis.as_str().unwrap()),
            flags: KeyFlags(self.flags),
            index: NonZeroU32::new(self.index).unwrap(),
        }
    }
    pub fn from_attributes(attr: KeyAttributes, token: ApiToken) -> PddbKeyAttrIpc {
        PddbKeyAttrIpc {
            len: attr.len as u64,
            reserved: attr.reserved as u64,
            age: attr.age as u64,
            dict: xous_ipc::String::<DICT_NAME_LEN>::from_str(&attr.dict),
            basis: xous_ipc::String::<BASIS_NAME_LEN>::from_str(&attr.basis),
            flags: attr.flags.0,
            index: attr.index.get(),
            token,
            code: PddbRequestCode::NoErr,
        }
    }
}

/// Debugging commands, available only in hosted mode
#[cfg(not(target_os = "xous"))]
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbDangerousDebug {
    pub request: DebugRequest,
    pub dump_name: xous_ipc::String::<128>,
}
#[cfg(not(target_os = "xous"))]
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum DebugRequest {
    Dump = 0,
    Remount = 1,
    Prune = 2,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_pddbuf_size() {
        assert!(core::mem::size_of::<PddbBuf>() == 4096, "PddBuf record has the wrong size");
    }
    #[test]
    fn test_pddb_len() {
        assert!(PDDB_A_LEN <= xous::PDDB_LEN as usize, "PDDB_A_LEN is larger than the maximum extents available in the hardware");
    }
}
