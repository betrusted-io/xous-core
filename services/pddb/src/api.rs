use rkyv::{Archive, Deserialize, Serialize};

use crate::backend::PAGE_SIZE;

pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";
pub(crate) const PDDB_MAX_BASIS_NAME_LEN: usize = 64;
pub(crate) const PDDB_MAX_DICT_NAME_LEN: usize = 64;
pub(crate) const PDDB_MAX_KEY_NAME_LEN: usize = 128;
pub(crate) const PDDB_MAGIC: [u8; 4] = [0x50, 0x44, 0x44, 0x42];
pub(crate) const PDDB_VERSION: u16 = 0;

pub const PDDB_DEFAULT_SYSTEM_BASIS: &'static str = ".System";
pub(crate) const PDDB_FAST_SPACE_SYSTEM_BASIS: &'static str = ".FastSpace";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    KeyRequest,

    /// read implemented using only scalars -- fast, but limited size
    ReadKeyScalar,
    /// read implemented using a memory buffer -- slower, but just shy of a page in size
    ReadKeyMem,

    WriteKeyScalar1,
    WriteKeyScalar2,
    WriteKeyScalar3,
    WriteKeyScalar4,
    WriteKeyMem,
    WriteKeyFlush,

    /// Suspend/resume callback
    SuspendResume,
}

/// A structure for requesting a token to access a particular key/value pair
#[derive(Archive, Serialize, Deserialize)]
pub(crate) struct PddbKeyRequest {
    pub(crate) dict: xous_ipc::String::</*PDDB_MAX_DICT_NAME_LEN*/ 64>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) key: xous_ipc::String::</*PDDB_MAX_KEY_NAME_LEN*/ 128>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) token: Option<[u32; 3]>,
}

/// Return codes for Read/Write API calls to the main server
#[repr(u8)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PddbRetcode {
    Uninit = 0,
    Ok = 1,
    BasisLost = 2,
    AccessDenied = 3,
}
/// PddbBuf is a C-representation of a page of memory that's used
/// to shuttle data for streaming channels. It must be exactly one
/// page in size, with some overhead specific to the PDDB book-keeping
/// at the top, and the remainder available for shuttling data.
///
/// It does not use rkyv, as the blanket implementation of that tends to
/// incur too many extra copies an/or zeroing operations.
#[repr(C, packed)]
pub(crate) struct PddbBuf {
    /// api token for the given buffer
    pub(crate) token: [u32; 3],
    /// length of the data field
    pub(crate) len: u16,
    /// a field reserved for the return code
    pub(crate) retcode: PddbRetcode,
    pub(crate) reserved: u8,
    pub(crate) data: [u8; 4080],
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_pddbuf_size() {
        assert!(core::mem::size_of::<PddbBuf>() == 4096, "PddBuf record has the wrong size");
    }
}
impl PddbBuf {
    pub(crate) fn from_slice_mut(slice: &mut [u8]) -> &mut PddbBuf {
        // this transforms the slice [u8] into a PddbBuf ref.
        unsafe {core::mem::transmute::<*mut u8, &mut PddbBuf>(slice.as_mut_ptr()) }
    }
}
