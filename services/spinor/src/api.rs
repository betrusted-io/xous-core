pub(crate) const SERVER_NAME_SPINOR: &str     = "_SPINOR Hardware Interface Server_";

#[cfg(any(target_os = "none", target_os = "xous"))]
pub const SPINOR_SIZE_BYTES: u32 = 128 * 1024 * 1024; // physical size of the device, used for hardware sanity checks on requests
pub const SPINOR_ERASE_SIZE: u32 = 0x1000; // this is the smallest sector size. 64k sectors also exist, but this implementation does not use them.
// note: logical lengths of regions are in xous::definitions

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// writes are split into multiple transactions. Must acquire exclusive rights before initiation
    AcquireExclusive,
    ReleaseExclusive,
    /// a special token is reserved for writing to the SoC region, only one service is allowed to do that
    RegisterSocToken,
    /// the SocToken holder can allow for writes to the staging area by other processes
    SetStagingWriteProtect,
    ClearStagingWriteProtect,
    /// program a region. Erase is accomplished by calling WriteRegion with all 0xFF's as data.
    WriteRegion,

    /// allow the susres manager to prevent new ops from happening during a suspend
    AcquireSuspendLock,
    ReleaseSuspendLock,

    /// intra-thread messages for suspend and resume
    SuspendInner,
    ResumeInner,

    /// internal interrupt handler ops
    EccError,
}
// Erase/Write are uninterruptable operations. Split suspend/resume
// into a separate server to asynchronously manage this.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum SusResOps {
    /// Suspend/resume callback
    SuspendResume,
    /// exit the thread
    Quit,
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


#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, num_derive::FromPrimitive, num_derive::ToPrimitive)]
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
