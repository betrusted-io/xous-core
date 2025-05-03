pub const SERVER_NAME_SPINOR: &str = "_SPINOR Hardware Interface Server_";

#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
pub const SPINOR_ERASE_SIZE: u32 = 0x1000; // this is the smallest sector size.

#[allow(dead_code)]
pub const SPINOR_BULK_ERASE_SIZE: u32 = 0x1_0000; // this is the bulk erase size.

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// writes are split into multiple transactions. Must acquire exclusive rights before initiation
    AcquireExclusive = 0,
    ReleaseExclusive = 1,

    /// program a region. Erase is accomplished by calling WriteRegion with all 0xFF's as data.
    WriteRegion = 5,
    /// bulk erase a region. Has fewer safety checks, used for accelerating the bulk clear of the PDDB for
    /// init.
    BulkErase = 6,

    Invalid = 65536,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub(crate) struct WriteRegion {
    /// the exclusive access ID
    pub id: [u32; 4],
    /// start address for the write; address 0 is start of FLASH.
    pub start: u32,
    /// set if the sector was checked to be erased already
    pub clean_patch: bool,
    /// length of data to write
    pub len: u32,
    /// return code
    pub result: Option<SpinorError>,
    /// data to write - up to one page
    pub data: [u8; 4096],
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub(crate) struct BulkErase {
    /// the exclusive access ID
    pub id: [u32; 4],
    /// start address for the erase; address 0 is start of FLASH. WARNING: This will get rounded down to the
    /// nearest 64kiB boundary.
    pub start: u32,
    /// length of data to erase. WARNING: This will always get rounded up to the nearest 64kiB.
    pub len: u32,
    /// return code
    pub result: Option<SpinorError>,
}

#[derive(
    Debug,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Clone,
    Copy,
    num_derive::FromPrimitive,
    num_derive::ToPrimitive,
)]
pub enum SpinorError {
    NoError,
    AbortNotErased,
    EraseFailed,
    WriteFailed,
    VerifyFailed,
    InvalidRequest,
    ImplementationError,
    AlignmentError,
    IpcError,
    BusyTryAgain,
    IdMismatch,
    NoId,
    AccessDenied,
}
