pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";
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
#[allow(dead_code)]
pub(crate) const PDDB_VERSION: u32 = 0x00_00_00_01;
#[allow(dead_code)]
// PDDB_A_LEN may be shorter than xous::PDDB_LEN, to speed up testing.
#[allow(dead_code)]
pub(crate) const PDDB_A_LEN: usize = xous::PDDB_LEN as usize;
// pub(crate) const PDDB_A_LEN: usize = 4 * 1024 * 1024;

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

#[allow(dead_code)]
pub(crate) const PDDB_DEFAULT_SYSTEM_BASIS: &'static str = ".System";
// this isn't an "official" basis, but it is used for the AAD for encrypting the FastSpace structure
#[allow(dead_code)]
pub(crate) const PDDB_FAST_SPACE_SYSTEM_BASIS: &'static str = ".FastSpace";

#[allow(dead_code)]
// TODO: add hardware acceleration for BCRYPT so we can hit the OWASP target without excessive UX delay
pub(crate) const BCRYPT_COST: u32 = 7;   // 10 is the minimum recommended by OWASP; takes 5696 ms to verify @ 10 rounds; 804 ms to verify 7 rounds

#[allow(dead_code)]
pub(crate) const PDDB_MODAL_NAME: &'static str = "pddb modal";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    ListBasis,
    LatestBasis,
    /// Note that creating a basis does not automatically open it!
    CreateBasis,
    OpenBasis,
    CloseBasis,
    /// warning, the Delete routine has not been well tested
    DeleteBasis,

    CreateDict,

    KeyRequest,

    ReadKey,
    WriteKey,
    WriteKeyFlush,

    /// quit the server
    Quit,
}

pub type ApiToken = [u32; 3];
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbBasisList {
    /// the first 63 that fit in the list -- generally we anticipate not more than a few being open at a time, so this should be enough.
    pub list: [xous_ipc::String::<BASIS_NAME_LEN>; 63],
    /// total number of basis open. Should be <= 63, but we allow it to be larger to indicate cases where this structure wasn't big enough.
    pub num: u32,
}
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PddbRequestCode {
    Create,
    Open,
    Close,
    Delete,
    NoErr,
    NotMounted,
    NoFreeSpace,
    NotFound,
    InternalError,
    AccessDenied,
    Uninit,
}
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbBasisRequest {
    pub name: xous_ipc::String::<BASIS_NAME_LEN>,
    pub code: PddbRequestCode,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PddbDictRequest {
    /// applications shouldn't specify a basis so that the PD mechanism works as intended, but non-sensitive system config keys will want to generally specify the system basis.
    pub basis_specified: bool,
    pub basis_name: xous_ipc::String::</* BASIS_NAME_LEN */ 64>, // pending https://github.com/rust-lang/rust/issues/90195
    pub dict_name: xous_ipc::String::</* DICT_NAME_LEN] */ 111>, // pending https://github.com/rust-lang/rust/issues/90195
    pub code: PddbRequestCode,
}

/// A structure for requesting a token to access a particular key/value pair
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PddbKeyRequest {
    pub(crate) dict: xous_ipc::String::</*DICT_NAME_LEN*/ 111>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) key: xous_ipc::String::</*KEY_NAME_LEN*/ 95>, // pending https://github.com/rust-lang/rust/issues/90195
    pub(crate) token: Option<ApiToken>,
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
    pub(crate) token: ApiToken,
    /// point in the key stream. 64-bit for future-compatibility; but, can't be larger than 32 bits on a 32-bit target.
    pub(crate) position: u64,
    /// length of the data field
    pub(crate) len: u16,
    /// a field reserved for the return code
    pub(crate) retcode: PddbRetcode,
    pub(crate) reserved: u8,
    pub(crate) data: [u8; 4072],
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
