pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";
#[allow(dead_code)]
pub(crate) const BASIS_NAME_LEN: usize = 64; // don't want this too long anyways, because it's not recorded anywhere - users have to type it in.
#[allow(dead_code)]
pub(crate) const DICT_NAME_LEN: usize = 127 - 4 - 4 - 4 - 4; // u32: flags, age, free index, numkeys = 111
#[allow(dead_code)]
pub(crate) const KEY_NAME_LEN: usize = 127 - 8 - 8 - 8 - 4 - 4; // u64: vaddr/len/resvd, u32: flags, age = 95
#[allow(dead_code)]
pub(crate) const PDDB_MAGIC: [u8; 4] = [0x50, 0x44, 0x44, 0x42];
#[allow(dead_code)]
pub(crate) const PDDB_VERSION: u32 = 0x00_00_00_01;
/// PDDB_A_LEN may be shorter than xous::PDDB_LEN, to speed up testing
#[allow(dead_code)]
pub(crate) const PDDB_A_LEN: usize = 4 * 1024 * 1024;

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

#[allow(dead_code)]
pub(crate) const PDDB_DEFAULT_SYSTEM_BASIS: &'static str = ".System";
// this isn't an "official" basis, but it is used for the AAD for encrypting the FastSpace structure
#[allow(dead_code)]
pub(crate) const PDDB_FAST_SPACE_SYSTEM_BASIS: &'static str = ".FastSpace";

#[allow(dead_code)]
// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
pub(crate) const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds


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
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PddbKeyRequest {
    pub(crate) dict: xous_ipc::String::</*DICT_NAME_LEN*/ 111>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) key: xous_ipc::String::</*KEY_NAME_LEN*/ 95>, // pending https://github.com/rust-lang/rust/issues/90195
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
impl PddbBuf {
    pub(crate) fn from_slice_mut(slice: &mut [u8]) -> &mut PddbBuf {
        // this transforms the slice [u8] into a PddbBuf ref.
        unsafe {core::mem::transmute::<*mut u8, &mut PddbBuf>(slice.as_mut_ptr()) }
    }
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
