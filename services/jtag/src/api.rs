pub(crate) const SERVER_NAME_JTAG: &str = "_JTAG Server_";

#[allow(dead_code)]
pub const XCS750_IDCODE: u32 = 0x362F093;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct EfuseRecord {
    pub key: [u8; 32],
    pub user: u32,
    pub cntl: u8,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum EfuseResult {
    Success,
    Failure,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    GetId,
    GetDna,
    EfuseFetch,
    EfuseKeyBurn,
    EfuseUserBurn,
    EfuseCtlBurn,
    WriteIr,
}
