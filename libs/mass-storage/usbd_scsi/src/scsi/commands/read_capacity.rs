use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ReadCapacity10Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 0, 2, 5)]
    pub lba: u32,

    #[pkd(7, 0, 9, 9)]
    pub control: Control,
}
impl ParsePackedStruct for ReadCapacity10Command {}