pub(crate) const SERVER_NAME_SPINOR: &str     = "_SPINOR Hardware Interface Server_";

pub const SPINOR_SIZE_BYTES: u32 = (128 * 1024 * 1024);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// erase a region. The whole op is atomic, so no exclusivity is required.
    EraseRegion,

    /// writes are split into multiple transactions. Must acquire exclusive rights before initiation
    AcquireExclusive,
    ReleaseExclusive,
    /// program a region
    WriteRegion,

    /// allow the susres manager to prevent new ops from happening during a suspend
    AcquireSuspendLock,
    ReleaseSuspendLock,

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
pub struct EraseRegion {
    /// start location for the erase
    pub start: u32,
    /// length of the region to erase
    pub len: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub struct WriteRegion {
    /// the exclusive access ID
    pub id: [u32; 4],
    /// start address for the write; address 0 is start of FLASH.
    pub start: u32,
    /// if true, erase the region to write if not already erased; otherwise, if not erased, the routine will error out
    pub autoerase: bool,
    /// data to write - up to one page
    pub data: [u8; 4096],
    /// length of data to write
    pub len: u32,
    /// return code
    pub result: Option<SpinorResult>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub enum SpinorError {
    NoError,
    AbortNotErased,
    EraseFailed,
    WriteFailed,
    VerifyFailed,
    InvalidRequest,
    ImplementationError,
    IpcError,
    BusyTryAgain,
    IdMismatch,
    NoId,
}
