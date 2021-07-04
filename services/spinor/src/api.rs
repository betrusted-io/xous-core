pub(crate) const SERVER_NAME_SPINOR: &str     = "_SPINOR Hardware Interface Server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// erase a region
    EraseRegion,

    /// program a region
    WriteRegion,

    /// Suspend/resume callback
    SuspendResume,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub struct EraseRegion {
    /// start location for the erase
    pub start: u32,
    /// length of the region to erase
    pub len: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub struct WriteRegion<const N: usize> { // wait...can we do const generics with rkyv??
    /// start location for the write
    pub start: u32,
    /// if true, erase the region to write if not already erased; otherwise, if not erased, the routine will error out
    pub autoerase: bool,
    /// data to write
    pub sector: [u8; N],
}
